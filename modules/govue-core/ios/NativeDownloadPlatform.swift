import Foundation

// The immutable callback is invoked only on the main queue.
final class NativeCoreEvents: CoreEvents, @unchecked Sendable {
  private let notify: (String) -> Void
  init(_ notify: @escaping (String) -> Void) { self.notify = notify }
  func changed(channel: String) { DispatchQueue.main.async { [self] in notify(channel) } }
}

final class NativeDownloadPlatform: DownloadPlatform {
  func directory() -> String {
    FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("downloads", isDirectory: true).path
  }
  func exportDirectory() -> String {
    FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("download-exports", isDirectory: true).path
  }
  func supportedFormats(formats: String) -> String {
    guard let data = formats.data(using: .utf8), let items = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return "[]" }
    let keys = NativeMediaCapabilities.supportedKeys(items.prefix(500))
    return String(decoding: (try? JSONSerialization.data(withJSONObject: keys)) ?? Data("[]".utf8), as: UTF8.self)
  }
  func mux(video: String, audio: String, output: String) -> Bool {
    DownloadMux.write(video: video, audio: audio, output: output)
  }
  func publish(id: String) -> Int32 { 0 }
  func deletePublished(uri: String) -> Bool { false }
  func activity(count: UInt32, title: String, received: UInt64, total: UInt64) {}
  static func combine(video: String, audio: String, output: String) async throws -> String {
    try await DownloadMux.combine(video: video, audio: audio, output: output)
  }
}
