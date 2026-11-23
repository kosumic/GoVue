import Foundation

private func expect(_ value: Bool, _ message: String = "Check failed", file: StaticString = #file, line: UInt = #line) {
  precondition(value, message, file: file, line: line)
}
private func expectEqual<T: Equatable>(_ actual: T, _ expected: T, _ message: String = "Values differ", file: StaticString = #file, line: UInt = #line) {
  expect(actual == expected, message, file: file, line: line)
}
private func expectNil<T>(_ value: T?, file: StaticString = #file, line: UInt = #line) {
  expect(value == nil, "Expected nil", file: file, line: line)
}

private final class HttpFixture: URLProtocol, @unchecked Sendable {
  static var handle: (HttpFixture) -> Void = { _ in preconditionFailure("Unexpected request") }
  override class func canInit(with request: URLRequest) -> Bool { true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  override func startLoading() { Self.handle(self) }
  override func stopLoading() {}
  func headers(_ fields: [String: String] = [:], status: Int = 200) {
    client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status,
      httpVersion: "HTTP/1.1", headerFields: fields)!, cacheStoragePolicy: .notAllowed)
  }
  func send(_ data: Data) { client?.urlProtocol(self, didLoad: data) }
  func finish() { client?.urlProtocolDidFinishLoading(self) }
}

final class IosHttpTests {
  private var session: URLSession!
  private var downloads: IosDownloadHttp!
  func setUp() {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [HttpFixture.self]
    configuration.httpMaximumConnectionsPerHost = 24
    session = URLSession(configuration: configuration)
    downloads = IosDownloadHttp(session: session)
  }
  func tearDown() { session.invalidateAndCancel(); downloads = nil; session = nil }
  private func fetch(_ path: String) -> IosDownloadReply {
    downloads.fetch(address: "https://media.example/\(path)", userAgent: "fixture", origin: "https://m.youtube.com",
      method: "GET", range: nil, wifiOnly: false)
  }

  func forwardsRedirectsAndErrorsWithoutFollowing() {
    var requests = 0
    HttpFixture.handle = { request in
      requests += 1
      if request.request.url!.path == "/redirect" {
        let destination = URL(string: "https://untrusted.example/destination")!
        let response = HTTPURLResponse(url: request.request.url!, statusCode: 302,
          httpVersion: "HTTP/1.1", headerFields: ["Location": destination.absoluteString])!
        request.client?.urlProtocol(request, wasRedirectedTo: URLRequest(url: destination), redirectResponse: response)
        request.finish()
      } else { request.headers(status: 403); request.finish() }
    }
    let redirect = fetch("redirect")
    expectEqual(redirect.status, 302)
    expect(redirect.body.isEmpty)
    expectEqual(redirect.location, "https://untrusted.example/destination")
    let forbidden = fetch("forbidden")
    expectEqual(forbidden.status, 403)
    expect(forbidden.body.isEmpty)
    expectEqual(requests, 2)
  }

  func boundsAndSignedQueriesSurviveTheAdapter() {
    HttpFixture.handle = { request in
      expectEqual(URLComponents(url: request.request.url!, resolvingAgainstBaseURL: false)!.percentEncodedQuery,
        "sig=fixture%2Fvalue%2B%3D&range=100-103")
      expectEqual(request.request.httpMethod, "POST")
      var upload = request.request.httpBody
      if upload == nil, let stream = request.request.httpBodyStream {
        stream.open()
        defer { stream.close() }
        var bytes = [UInt8](repeating: 0, count: 3)
        let count = stream.read(&bytes, maxLength: bytes.count)
        expect(count >= 0)
        upload = Data(bytes.prefix(count))
      }
      expectEqual(upload, Data([0x78, 0]))
      expectEqual(request.request.value(forHTTPHeaderField: "Accept-Encoding"), "identity")
      request.headers(["Content-Length": "4"])
      request.send(Data([1, 2, 3, 4])); request.finish()
    }
    let result = downloads.fetch(address: "https://media.example/file?sig=fixture%2Fvalue%2B%3D",
      userAgent: "fixture", origin: "https://m.youtube.com", method: "POST", range: 100...103, wifiOnly: false)
    expectEqual(result.status, 200)
    expectEqual(result.body, Data([1, 2, 3, 4]))
    for (url, range) in [("http://media.example/file", UInt64(0)...3),
      ("https://media.example/file", UInt64(0)...1_048_576)] {
      expectEqual(downloads.fetch(address: url, userAgent: "fixture", origin: "https://m.youtube.com",
        method: "GET", range: range, wifiOnly: false).status, 0)
    }
  }

  func prefixCancellationDoesNotInvalidateOtherTasks() {
    HttpFixture.handle = { request in
      request.headers(["Content-Length": "8192"])
      request.send(Data(repeating: 7, count: 8192)); request.finish()
    }
    let prefix = DispatchSemaphore(value: 0)
    _ = BoundedHttp(request: URLRequest(url: URL(string: "https://media.example/prefix")!),
      limit: 1024, prefixOnly: true, session: session) { status, body, _ in
      expectEqual(status, 200)
      expectEqual(body, Data(repeating: 7, count: 1024))
      prefix.signal()
    }
    expect(prefix.wait(timeout: .now() + 2) == .success, "Prefix read timed out")
    let bounded = DispatchSemaphore(value: 0)
    _ = BoundedHttp(request: URLRequest(url: URL(string: "https://media.example/oversized")!),
      limit: 1024, session: session) { status, _, _ in
      expectEqual(status, 0)
      bounded.signal()
    }
    expect(bounded.wait(timeout: .now() + 2) == .success, "Bounded response timed out")
    expectEqual(fetch("other").body, Data(repeating: 7, count: 8192))
  }

  func sessionHasNoPersistentOrAmbientCredentials() {
    let configuration = NativeHttpSession.shared.configuration
    expectNil(configuration.urlCache)
    expectNil(configuration.httpCookieStorage)
    expectNil(configuration.urlCredentialStorage)
    expect(!configuration.httpShouldSetCookies)
  }
}

@main enum IosHttpChecks {
  static func main() {
    let checks = IosHttpTests()
    let cases: [(String, () -> Void)] = [
      ("forwardsRedirectsAndErrorsWithoutFollowing", checks.forwardsRedirectsAndErrorsWithoutFollowing),
      ("boundsAndSignedQueriesSurviveTheAdapter", checks.boundsAndSignedQueriesSurviveTheAdapter),
      ("prefixCancellationDoesNotInvalidateOtherTasks", checks.prefixCancellationDoesNotInvalidateOtherTasks),
      ("sessionHasNoPersistentOrAmbientCredentials", checks.sessionHasNoPersistentOrAmbientCredentials),
    ]
    for (name, run) in cases {
      FileHandle.standardOutput.write(Data("Running: \(name)\n".utf8))
      checks.setUp()
      run()
      checks.tearDown()
      print("Passed: \(name)")
    }
    print("\(cases.count) iOS HTTP checks passed")
  }
}
