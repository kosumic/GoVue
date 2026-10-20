import Foundation
import Network

/// OS connectivity and bounded bytes. Rust owns policy waits and retries.
final class NativeDownloadHttp: DownloadHttp, @unchecked Sendable {
  private let monitor = NWPathMonitor()
  private let capacity = DispatchSemaphore(value: 24)
  private let session: URLSession

  init(session: URLSession = NativeHttpSession.shared) {
    self.session = session
    monitor.start(queue: DispatchQueue(label: "GoVue.downloadNetwork"))
  }

  deinit { monitor.cancel() }

  func wifiConnected() -> Bool {
    let path = monitor.currentPath
    return path.status == .satisfied && path.usesInterfaceType(.wifi) && !path.usesInterfaceType(.cellular)
  }

  func fetch(request: DownloadRequest) -> MediaRangeReply {
    let empty = MediaRangeReply(status: 0, body: Data(), location: nil)
    if request.wifiOnly && !wifiConnected() { return MediaRangeReply(status: -1, body: Data(), location: nil) }
    guard var components = URLComponents(string: request.url), components.scheme == "https",
      ["GET", "POST"].contains(request.method), capacity.wait(timeout: .now()) == .success else { return empty }
    defer { capacity.signal() }
    if let range = request.range {
      guard range.end >= range.start, range.end - range.start < 1_048_576 else { return empty }
      let query = components.percentEncodedQuery ?? ""
      components.percentEncodedQuery = query + (query.isEmpty ? "" : "&") + "range=\(range.start)-\(range.end)"
    }
    guard let address = components.url else { return empty }
    var input = URLRequest(url: address)
    input.httpMethod = request.method
    // Enforced by URLSession even if Wi-Fi disappears after the path check or
    // while Rust / React Native is running in the background.
    input.allowsCellularAccess = !request.wifiOnly
    if request.method == "POST" { input.httpBody = Data([0x78, 0]) }
    input.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
    input.setValue(request.userAgent, forHTTPHeaderField: "User-Agent")
    input.setValue(request.origin, forHTTPHeaderField: "Origin")
    input.setValue(request.origin + "/", forHTTPHeaderField: "Referer")
    let completed = DispatchSemaphore(value: 0)
    var reply = empty
    let task = BoundedHttp(request: input, limit: 1_048_576, session: session) { status, body, location in
      reply = MediaRangeReply(status: Int32(status), body: body, location: location)
      completed.signal()
    }
    guard completed.wait(timeout: .now() + 20) == .success else { task.cancel(); return empty }
    if reply.status == 0 && request.wifiOnly && !wifiConnected() {
      return MediaRangeReply(status: -1, body: Data(), location: nil)
    }
    return reply
  }
}
