import Foundation

/// Platform byte transport only. Rust owns URL policy, media planning and caches.
final class BoundedHttp: NSObject, URLSessionDataDelegate, @unchecked Sendable {
  private var session: URLSession?
  private var task: URLSessionDataTask?
  private let limit: Int
  private let prefixOnly: Bool
  private var prefixComplete = false
  private let destination: URL?
  private let completion: (Int, Data, String?) -> Void
  private let responseHeaders: ((HTTPURLResponse) -> Void)?
  private var file: FileHandle?
  private var body = Data()
  private var received = 0
  private var status = 0
  private var location: String?
  private var failed = false
  private var rejectedResponse = false

  init(request: URLRequest, limit: Int, destination: URL? = nil, prefixOnly: Bool = false, timeout: TimeInterval = 15,
       responseHeaders: ((HTTPURLResponse) -> Void)? = nil,
       completion: @escaping (Int, Data, String?) -> Void) {
    self.limit = limit
    self.prefixOnly = prefixOnly
    self.destination = destination
    self.completion = completion
    self.responseHeaders = responseHeaders
    super.init()
    let configuration = URLSessionConfiguration.ephemeral
    configuration.httpCookieStorage = nil
    configuration.httpShouldSetCookies = false
    configuration.urlCredentialStorage = nil
    configuration.timeoutIntervalForRequest = timeout
    configuration.timeoutIntervalForResource = timeout + 2
    let queue = OperationQueue()
    queue.maxConcurrentOperationCount = 1
    session = URLSession(configuration: configuration, delegate: self, delegateQueue: queue)
    task = session?.dataTask(with: request)
    task?.resume()
  }

  func cancel() { task?.cancel() }

  func urlSession(_ session: URLSession, task: URLSessionTask,
                  willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                  completionHandler: @escaping (URLRequest?) -> Void) {
    // Return redirects to Rust; never follow an unvalidated destination.
    status = response.statusCode
    location = response.value(forHTTPHeaderField: "Location").map { String($0.prefix(8192)) }
    completionHandler(nil)
  }

  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                  completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
    guard let response = response as? HTTPURLResponse else {
      failed = true; completionHandler(.cancel); return
    }
    status = response.statusCode
    location = response.value(forHTTPHeaderField: "Location").map { String($0.prefix(8192)) }
    guard (200..<300).contains(status) else {
      rejectedResponse = true
      completionHandler(.cancel)
      return
    }
    responseHeaders?(response)
    do {
      guard prefixOnly || response.expectedContentLength <= Int64(limit) else {
        failed = true; completionHandler(.cancel); return
      }
      if let destination {
        do {
          guard FileManager.default.createFile(atPath: destination.path, contents: nil) else {
            throw CocoaError(.fileWriteUnknown)
          }
          file = try FileHandle(forWritingTo: destination)
        } catch { failed = true; completionHandler(.cancel); return }
      }
    }
    completionHandler(.allow)
  }

  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
    guard (200..<300).contains(status), !failed else { return }
    if prefixOnly {
      guard !prefixComplete else { return }
      body.append(data.prefix(max(0, limit - body.count)))
      if body.count == limit { prefixComplete = true; dataTask.cancel() }
      return
    }
    received += data.count
    guard received <= limit else { failed = true; dataTask.cancel(); return }
    do {
      if let file { try file.write(contentsOf: data) } else { body.append(data) }
    } catch { failed = true; dataTask.cancel() }
  }

  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    do { try file?.close() } catch { failed = true }
    file = nil
    if error != nil || failed || !(200..<300).contains(status) {
      if let destination { try? FileManager.default.removeItem(at: destination) }
    }
    completion((error == nil || rejectedResponse || prefixComplete) && !failed ? status : 0, body, location)
    body.removeAll()
    session.finishTasksAndInvalidate()
    self.task = nil
    self.session = nil
  }
}
