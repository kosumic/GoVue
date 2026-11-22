import Foundation
import Network

/// OS connectivity and bounded bytes. Rust owns policy waits and retries.
final class IosDownloadHttp: @unchecked Sendable {
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

  func fetch(address url: String, userAgent: String, origin: String, method: String,
             range: ClosedRange<UInt64>?, wifiOnly: Bool) -> IosDownloadReply {
    let empty = IosDownloadReply(status: 0, body: Data(), location: nil)
    if wifiOnly && !wifiConnected() { return IosDownloadReply(status: -1, body: Data(), location: nil) }
    guard var components = URLComponents(string: url), components.scheme == "https",
      ["GET", "POST"].contains(method), capacity.wait(timeout: .now()) == .success else { return empty }
    defer { capacity.signal() }
    if let range = range {
      guard range.upperBound - range.lowerBound < 1_048_576 else { return empty }
      let query = components.percentEncodedQuery ?? ""
      components.percentEncodedQuery = query + (query.isEmpty ? "" : "&") + "range=\(range.lowerBound)-\(range.upperBound)"
    }
    guard let address = components.url else { return empty }
    var input = URLRequest(url: address)
    input.httpMethod = method
    // Enforced by URLSession even if Wi-Fi disappears after the path check or
    // while Rust / React Native is running in the background.
    input.allowsCellularAccess = !wifiOnly
    if method == "POST" { input.httpBody = Data([0x78, 0]) }
    input.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
    input.setValue(userAgent, forHTTPHeaderField: "User-Agent")
    input.setValue(origin, forHTTPHeaderField: "Origin")
    input.setValue(origin + "/", forHTTPHeaderField: "Referer")
    let completed = DispatchSemaphore(value: 0)
    var reply = empty
    let task = BoundedHttp(request: input, limit: 1_048_576, session: session) { status, body, location in
      reply = IosDownloadReply(status: Int32(status), body: body, location: location)
      completed.signal()
    }
    guard completed.wait(timeout: .now() + 20) == .success else { task.cancel(); return empty }
    if reply.status == 0 && wifiOnly && !wifiConnected() {
      return IosDownloadReply(status: -1, body: Data(), location: nil)
    }
    return reply
  }
}

struct IosDownloadReply {
  let status: Int32
  let body: Data
  let location: String?
}
