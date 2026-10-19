import Foundation

/// HTTPS bytes only. Rust owns range planning, redirects, retries and URL policy.
final class NativeMediaHttp: MediaHttp, MediaStreamHttp, @unchecked Sendable {
  private let lock = NSLock()
  private let session: URLSession
  private let capacity = DispatchSemaphore(value: 24)
  private var streams: [UInt64: NativeMediaStream] = [:]
  private var sequence: UInt64 = 0

  init(session: URLSession = NativeHttpSession.shared) { self.session = session }

  func fetchRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaRangeReply {
    let head = openRange(url: url, userAgent: userAgent, origin: origin, method: method, start: start, end: end)
    guard head.handle != 0 else { return MediaRangeReply(status: head.status, body: Data(), location: head.location) }
    defer { close(handle: head.handle) }
    var body = Data()
    while true {
      let bytes = read(handle: head.handle, maxBytes: 65_536)
      if bytes.isEmpty { break }
      body.append(bytes)
    }
    return MediaRangeReply(status: head.status, body: body, location: head.location)
  }

  func openRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaStreamReply {
    let empty = MediaStreamReply(status: 0, handle: 0, length: -1, location: nil)
    guard end >= start, end - start < 1_048_576, ["GET", "POST"].contains(method),
      var components = URLComponents(string: url), components.scheme == "https",
      components.user == nil, components.password == nil, components.port == nil || components.port == 443,
      capacity.wait(timeout: .now()) == .success else { return empty }
    // Keep the signed query bytes unchanged when adding Rust's bounded range.
    let query = components.percentEncodedQuery ?? ""
    components.percentEncodedQuery = query + (query.isEmpty ? "" : "&") + "range=\(start)-\(end)"
    guard let address = components.url else { capacity.signal(); return empty }
    var request = URLRequest(url: address, timeoutInterval: 12)
    request.httpMethod = method
    if method == "POST" { request.httpBody = Data([0x78, 0]) }
    request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
    request.setValue(userAgent, forHTTPHeaderField: "User-Agent")
    request.setValue(origin, forHTTPHeaderField: "Origin")
    request.setValue(origin + "/", forHTTPHeaderField: "Referer")
    lock.lock()
    sequence = sequence == UInt64.max ? 1 : sequence + 1
    let handle = sequence
    lock.unlock()
    let stream = NativeMediaStream(session: session, request: request, length: Int(end - start + 1))
    let head = stream.headers(handle: handle)
    guard head.handle != 0 else { stream.close(); capacity.signal(); return head }
    lock.lock()
    streams[handle] = stream
    lock.unlock()
    return head
  }

  func read(handle: UInt64, maxBytes: UInt32) -> Data {
    guard maxBytes > 0, maxBytes <= 65_536 else { close(handle: handle); return Data() }
    lock.lock()
    let stream = streams[handle]
    lock.unlock()
    let bytes = stream?.read(maxBytes: Int(maxBytes)) ?? Data()
    if bytes.isEmpty { close(handle: handle) }
    return bytes
  }

  func close(handle: UInt64) {
    lock.lock()
    let stream = streams.removeValue(forKey: handle)
    lock.unlock()
    if let stream { stream.close(); capacity.signal() }
  }
}

/// A suspended producer never blocks the shared URLSession delegate queue.
/// Buffering is bounded by the range cap; resume below the low water mark.
private final class NativeMediaStream: NSObject, URLSessionDataDelegate, @unchecked Sendable {
  private let condition = NSCondition()
  private let length: Int
  private let deadline = Date(timeIntervalSinceNow: 18)
  private var task: URLSessionDataTask?
  private var response: HTTPURLResponse?
  private var body = Data()
  private var received = 0
  private var finished = false
  private var failed = false
  private var closed = false
  private var suspended = false

  init(session: URLSession, request: URLRequest, length: Int) {
    self.length = length
    super.init()
    task = session.dataTask(with: request)
    task?.delegate = self
    task?.resume()
  }

  func headers(handle: UInt64) -> MediaStreamReply {
    condition.lock()
    defer { condition.unlock() }
    let until = min(deadline, Date(timeIntervalSinceNow: 8))
    while response == nil && !finished && !closed {
      if !condition.wait(until: until) { cancelLocked(); break }
    }
    guard let response, !closed, !failed else {
      return MediaStreamReply(status: 0, handle: 0, length: -1, location: nil)
    }
    return MediaStreamReply(status: Int32(response.statusCode),
      handle: [200, 206].contains(response.statusCode) ? handle : 0,
      length: response.expectedContentLength,
      location: response.value(forHTTPHeaderField: "Location").map { String($0.prefix(30_000)) })
  }

  func read(maxBytes: Int) -> Data {
    condition.lock()
    defer { condition.unlock() }
    let until = min(deadline, Date(timeIntervalSinceNow: 12))
    while body.isEmpty && !finished && !closed {
      if !condition.wait(until: until) { cancelLocked(); break }
    }
    guard !closed, !failed else { return Data() }
    let bytes = Data(body.prefix(maxBytes))
    body.removeFirst(bytes.count)
    if suspended && body.count < 65_536 {
      suspended = false
      task?.resume()
    }
    return bytes
  }

  private func cancelLocked() {
    closed = true
    body.removeAll()
    task?.cancel()
    condition.broadcast()
  }

  func close() {
    condition.lock()
    cancelLocked()
    condition.unlock()
  }

  func urlSession(_ session: URLSession, task: URLSessionTask,
                  willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                  completionHandler: @escaping (URLRequest?) -> Void) {
    condition.lock()
    self.response = response
    finished = true
    condition.broadcast()
    condition.unlock()
    // Only Rust may validate and follow the redirect.
    completionHandler(nil)
  }

  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                  completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
    condition.lock()
    let http = response as? HTTPURLResponse
    self.response = http
    let encoding = http?.value(forHTTPHeaderField: "Content-Encoding")
    let allowed = http.map { [200, 206].contains($0.statusCode) } == true
    failed = http == nil || (allowed && (response.expectedContentLength > Int64(length)
      || (encoding != nil && encoding?.lowercased() != "identity")))
    let accept = allowed && !failed && !closed
    if !accept { finished = true }
    condition.broadcast()
    condition.unlock()
    completionHandler(accept ? .allow : .cancel)
  }

  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
    condition.lock()
    defer { condition.unlock() }
    guard !closed, !failed else { return }
    received += data.count
    guard received <= length else { failed = true; cancelLocked(); return }
    body.append(data)
    if !suspended && body.count >= 131_072 {
      suspended = true
      dataTask.suspend()
    }
    condition.broadcast()
  }

  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    condition.lock()
    finished = true
    if error != nil && response.map({ [200, 206].contains($0.statusCode) }) != false { failed = true }
    self.task = nil
    condition.broadcast()
    condition.unlock()
  }
}
