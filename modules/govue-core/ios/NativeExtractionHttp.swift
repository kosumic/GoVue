import Foundation

/// Byte transport only. Rust supplies the validated URL, headers and protocol.
final class NativeExtractionHttp: ExtractionHttp, @unchecked Sendable {
  func request(request: ExtractionRequest) -> ExtractionReply {
    guard let url = URL(string: request.url), request.maxBytes > 0, request.maxBytes <= 8_000_000,
      ["GET", "POST"].contains(request.method) else { return ExtractionReply(status: 0, body: Data(), headers: []) }
    var input = URLRequest(url: url)
    input.httpMethod = request.method
    if request.method == "POST" { input.httpBody = Data(request.body.utf8) }
    for header in request.headers { input.setValue(header.value, forHTTPHeaderField: header.name) }
    let signal = DispatchSemaphore(value: 0)
    var headers: [ExtractionHeader] = []
    var reply = ExtractionReply(status: 0, body: Data(), headers: [])
    let task = BoundedHttp(request: input, limit: Int(request.maxBytes) + 1, timeout: 15, responseHeaders: { response in
      if let value = response.value(forHTTPHeaderField: "Set-Cookie") { headers = [ExtractionHeader(name: "Set-Cookie", value: value)] }
    }) { status, body, _ in
      reply = ExtractionReply(status: Int32(status), body: body, headers: headers)
      signal.signal()
    }
    if signal.wait(timeout: .now() + 18) == .timedOut { task.cancel(); return ExtractionReply(status: 0, body: Data(), headers: []) }
    return reply
  }
}
