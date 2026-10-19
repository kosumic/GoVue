import Foundation

/// Keep HTTPS connections alive across extraction and bounded media requests.
/// Task delegates isolate responses; cookies, credentials and disk caching stay off.
enum NativeHttpSession {
  static let shared: URLSession = {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.httpCookieStorage = nil
    configuration.httpShouldSetCookies = false
    configuration.urlCredentialStorage = nil
    configuration.urlCache = nil
    configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
    configuration.timeoutIntervalForRequest = 15
    configuration.timeoutIntervalForResource = 18
    let queue = OperationQueue()
    queue.maxConcurrentOperationCount = 1
    queue.qualityOfService = .userInitiated
    return URLSession(configuration: configuration, delegate: nil, delegateQueue: queue)
  }()
}
