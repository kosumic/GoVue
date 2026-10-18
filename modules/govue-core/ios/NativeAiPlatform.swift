import Foundation
import Security

private final class AiHttpDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
  let sink: AiResponse
  let done = DispatchSemaphore(value: 0)
  init(_ sink: AiResponse) { self.sink = sink }
  func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse, completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
    guard let http = response as? HTTPURLResponse else { sink.failed(); completionHandler(.cancel); return }
    sink.start(status: Int32(http.statusCode)); completionHandler(.allow)
  }
  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
    if !sink.push(bytes: data) { dataTask.cancel() }
  }
  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    if let error, (error as NSError).code != NSURLErrorCancelled { sink.failed() }
    done.signal()
  }
}

final class NativeAiPlatform: AiPlatform, @unchecked Sendable {
  private let service = "io.github.kaminarios.lingoplay.ai"
  private func query(_ key: String) -> [String: Any] {
    [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: key]
  }
  private func valid(_ key: String) -> Bool {
    key.hasPrefix("ai.") && key.utf8.count <= 100 && key.unicodeScalars.allSatisfy { CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-").contains($0) }
  }
  func secretRead(key: String) -> AiSecret {
    guard valid(key) else { return AiSecret(ok: false, value: "") }
    var q = query(key); q[kSecReturnData as String] = true; q[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(q as CFDictionary, &result)
    if status == errSecItemNotFound { return AiSecret(ok: true, value: "") }
    guard status == errSecSuccess, let data = result as? Data, let text = String(data: data, encoding: .utf8) else { return AiSecret(ok: false, value: "") }
    return AiSecret(ok: true, value: text)
  }
  func secretWrite(key: String, value: String) -> Bool {
    guard valid(key), value.utf8.count <= 100_000 else { return false }
    let q = query(key)
    if value.isEmpty { let status = SecItemDelete(q as CFDictionary); return status == errSecSuccess || status == errSecItemNotFound }
    let attributes: [String: Any] = [kSecValueData as String: Data(value.utf8), kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
    let status = SecItemUpdate(q as CFDictionary, attributes as CFDictionary)
    if status == errSecSuccess { return true }
    guard status == errSecItemNotFound else { return false }
    return SecItemAdd(q.merging(attributes) { _, new in new } as CFDictionary, nil) == errSecSuccess
  }
  func request(request: AiHttpRequest, response: AiResponse) {
    guard let url = URL(string: request.url),
      url.scheme == "https" || (url.scheme == "http" && ["127.0.0.1", "localhost"].contains(url.host ?? "")),
      url.user == nil, url.password == nil, ["GET", "POST"].contains(request.method) else { response.failed(); return }
    var req = URLRequest(url: url); req.httpMethod = request.method
    if request.method == "POST" { req.httpBody = Data(request.body.utf8) }
    for header in request.headers { req.setValue(header.value, forHTTPHeaderField: header.name) }
    let delegate = AiHttpDelegate(response)
    let configuration = URLSessionConfiguration.ephemeral
    configuration.httpCookieStorage = nil; configuration.urlCache = nil
    configuration.timeoutIntervalForRequest = 600; configuration.timeoutIntervalForResource = 900
    let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
    let task = session.dataTask(with: req); task.resume()
    let deadline = Date().addingTimeInterval(905)
    while delegate.done.wait(timeout: .now() + 0.2) == .timedOut {
      if response.isCancelled() { task.cancel(); break }
      if Date() >= deadline { task.cancel(); response.failed(); break }
    }
    session.invalidateAndCancel()
  }
}
