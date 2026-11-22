import Foundation

final class NativeDownloadHttp: DownloadHttp, @unchecked Sendable {
  private let http: IosDownloadHttp

  init(session: URLSession = NativeHttpSession.shared) {
    http = IosDownloadHttp(session: session)
  }

  func wifiConnected() -> Bool { http.wifiConnected() }

  func fetch(request: DownloadRequest) -> MediaRangeReply {
    var range: ClosedRange<UInt64>?
    if let bounds = request.range {
      guard bounds.end >= bounds.start else {
        return MediaRangeReply(status: 0, body: Data(), location: nil)
      }
      range = bounds.start...bounds.end
    }
    let reply = http.fetch(address: request.url, userAgent: request.userAgent, origin: request.origin,
                           method: request.method, range: range, wifiOnly: request.wifiOnly)
    return MediaRangeReply(status: reply.status, body: reply.body, location: reply.location)
  }
}
