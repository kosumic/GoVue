import Foundation

private func expect(_ value: Bool, _ message: String = "Check failed", file: StaticString = #file, line: UInt = #line) {
  precondition(value, message, file: file, line: line)
}
private func expectEqual<T: Equatable>(_ actual: T, _ expected: T, _ message: String = "Values differ", file: StaticString = #file, line: UInt = #line) {
  expect(actual == expected, message, file: file, line: line)
}
private func expectDifferent<T: Equatable>(_ actual: T, _ expected: T, file: StaticString = #file, line: UInt = #line) {
  expect(actual != expected, "Values should differ", file: file, line: line)
}
private func expectNil<T>(_ value: T?, file: StaticString = #file, line: UInt = #line) {
  expect(value == nil, "Expected nil", file: file, line: line)
}

// UniFFI callback shapes only: exercise the production adapters without linking Rust.
struct MediaRangeReply { let status: Int32; let body: Data; let location: String? }
struct MediaStreamReply { let status: Int32; let handle: UInt64; let length: Int64; let location: String? }
protocol MediaHttp: AnyObject, Sendable {
  func fetchRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaRangeReply
}
protocol MediaStreamHttp: AnyObject, Sendable {
  func openRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaStreamReply
  func read(handle: UInt64, maxBytes: UInt32) -> Data
  func close(handle: UInt64)
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
  private var media: NativeMediaHttp!
  func setUp() {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [HttpFixture.self]
    configuration.httpMaximumConnectionsPerHost = 24
    session = URLSession(configuration: configuration)
    media = NativeMediaHttp(session: session)
  }
  func tearDown() { session.invalidateAndCancel(); media = nil; session = nil }
  private func open(_ path: String = "media", length: UInt64 = 4096) -> MediaStreamReply {
    media.openRange(url: "https://media.example/\(path)", userAgent: "fixture", origin: "https://m.youtube.com",
      method: "GET", start: 0, end: length - 1)
  }

  func streamsBeforeTheResponseFinishes() {
    let release = DispatchSemaphore(value: 0)
    HttpFixture.handle = { request in
      request.headers(["Content-Length": "4096"])
      request.send(Data(repeating: 1, count: 1024))
      DispatchQueue.global().async {
        guard release.wait(timeout: .now() + 5) == .success else { return }
        request.send(Data(repeating: 2, count: 3072))
        request.finish()
      }
    }
    let head = open()
    expectEqual(head.status, 200)
    expectDifferent(head.handle, 0)
    expectEqual(media.read(handle: head.handle, maxBytes: 1024), Data(repeating: 1, count: 1024))
    release.signal()
    expectEqual(media.read(handle: head.handle, maxBytes: 65_536), Data(repeating: 2, count: 3072))
    expect(media.read(handle: head.handle, maxBytes: 65_536).isEmpty)
    media.close(handle: head.handle)
  }

  func cancellationWakesAReaderAndReleasesCapacity() {
    HttpFixture.handle = { request in
      request.headers(["Content-Length": "4096"])
      request.send(Data(repeating: 1, count: 4096)); request.finish()
    }
    let heads = (0..<24).map { _ in open() }
    expect(heads.allSatisfy { $0.handle != 0 })
    expectEqual(open().handle, 0, "Open upstream requests must be bounded")
    for head in heads { media.close(handle: head.handle) }
    HttpFixture.handle = { request in
      request.headers(["Content-Length": "4096"])
      request.send(Data(repeating: 1, count: 1024))
    }
    let next = open()
    expectDifferent(next.handle, 0)
    expectEqual(media.read(handle: next.handle, maxBytes: 1024).count, 1024)
    let waiting = DispatchSemaphore(value: 0)
    let adapter = media!
    DispatchQueue.global().async {
      expect(adapter.read(handle: next.handle, maxBytes: 1024).isEmpty)
      waiting.signal()
    }
    media.close(handle: next.handle)
    expect(waiting.wait(timeout: .now() + 2) == .success, "Cancelled read did not return")
  }

  func aBufferedStreamDoesNotBlockAnotherRequest() {
    HttpFixture.handle = { request in
      let count = request.request.url!.path == "/large" ? 262_144 : 1024
      request.headers(["Content-Length": String(count)])
      request.send(Data(repeating: 3, count: count)); request.finish()
    }
    let large = open("large", length: 262_144)
    let small = open("small", length: 1024)
    expectEqual(media.read(handle: small.handle, maxBytes: 1024).count, 1024)
    media.close(handle: small.handle)
    var bytes = 0
    while true {
      let part = media.read(handle: large.handle, maxBytes: 16_384)
      if part.isEmpty { break }
      bytes += part.count
    }
    expectEqual(bytes, 262_144)
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
      } else { request.headers(status: 403); request.finish() }
    }
    let redirect = open("redirect")
    expectEqual(redirect.status, 302)
    expectEqual(redirect.handle, 0)
    expectEqual(redirect.location, "https://untrusted.example/destination")
    let forbidden = open("forbidden")
    expectEqual(forbidden.status, 403)
    expectEqual(forbidden.handle, 0)
    expectEqual(requests, 2)
  }

  func rejectsOversizedAndEncodedBodies() {
    HttpFixture.handle = { request in
      if request.request.url!.path == "/encoded" {
        request.headers(["Content-Encoding": "gzip"])
      } else {
        request.headers()
        request.send(Data(repeating: 1, count: 4097))
      }
      request.finish()
    }
    let oversized = open("oversized")
    expect(media.read(handle: oversized.handle, maxBytes: 65_536).isEmpty)
    expectEqual(open("encoded").handle, 0)
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
    let result = media.fetchRange(url: "https://media.example/file?sig=fixture%2Fvalue%2B%3D",
      userAgent: "fixture", origin: "https://m.youtube.com", method: "POST", start: 100, end: 103)
    expectEqual(result.status, 200)
    expectEqual(result.body, Data([1, 2, 3, 4]))
    for (url, start, end) in [("http://media.example/file", UInt64(0), UInt64(3)),
      ("https://media.example/file", 4, 3), ("https://media.example/file", 0, 1_048_576)] {
      expectEqual(media.openRange(url: url, userAgent: "fixture", origin: "https://m.youtube.com",
        method: "GET", start: start, end: end).handle, 0)
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
    let head = open(length: 8192)
    var body = Data()
    while true {
      let bytes = media.read(handle: head.handle, maxBytes: 1024)
      if bytes.isEmpty { break }
      body.append(bytes)
    }
    expectEqual(body, Data(repeating: 7, count: 8192))
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
      ("streamsBeforeTheResponseFinishes", checks.streamsBeforeTheResponseFinishes),
      ("cancellationWakesAReaderAndReleasesCapacity", checks.cancellationWakesAReaderAndReleasesCapacity),
      ("aBufferedStreamDoesNotBlockAnotherRequest", checks.aBufferedStreamDoesNotBlockAnotherRequest),
      ("forwardsRedirectsAndErrorsWithoutFollowing", checks.forwardsRedirectsAndErrorsWithoutFollowing),
      ("rejectsOversizedAndEncodedBodies", checks.rejectsOversizedAndEncodedBodies),
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
