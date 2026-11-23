import Foundation
import AVFoundation

enum DownloadMux {
  private final class MuxResult: @unchecked Sendable {
    let semaphore = DispatchSemaphore(value: 0)
    var success = false // The semaphore orders the writer and reader.
  }

  static func write(video: String, audio: String, output: String) -> Bool {
    let result = MuxResult()
    Task {
      do { _ = try await combine(video: video, audio: audio, output: output); result.success = true } catch {}
      result.semaphore.signal()
    }
    result.semaphore.wait()
    return result.success
  }
  static func combine(video: String, audio: String, output: String) async throws -> String {
      func local(_ value: String) throws -> URL {
        guard let url = URL(string: value), url.isFileURL,
          url.resolvingSymlinksInPath().path.hasPrefix(NSHomeDirectory() + "/") else {
          throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "Media must be in app storage."])
        }
        return url
      }
      let target = try local(output)
      guard !FileManager.default.fileExists(atPath: target.path) else {
        throw NSError(domain: "GoVue", code: 2, userInfo: [NSLocalizedDescriptionKey: "Output already exists."])
      }
      let composition = AVMutableComposition()
      for (path, type) in [(video, AVMediaType.video), (audio, AVMediaType.audio)] {
        let asset = AVURLAsset(url: try local(path))
        guard let source = try await asset.loadTracks(withMediaType: type).first,
          let track = composition.addMutableTrack(withMediaType: type, preferredTrackID: kCMPersistentTrackID_Invalid) else {
          throw NSError(domain: "GoVue", code: 3, userInfo: [NSLocalizedDescriptionKey: "Missing media track."])
        }
        let duration = try await asset.load(.duration)
        try track.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: source, at: .zero)
      }
      guard let exporter = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough) else {
        throw NSError(domain: "GoVue", code: 4, userInfo: [NSLocalizedDescriptionKey: "Could not combine media."])
      }
      exporter.outputURL = target
      exporter.outputFileType = .mp4
      await withCheckedContinuation { continuation in
        exporter.exportAsynchronously { continuation.resume() }
      }
      guard exporter.status == .completed else {
        try? FileManager.default.removeItem(at: target)
        throw exporter.error ?? NSError(domain: "GoVue", code: 5, userInfo: [NSLocalizedDescriptionKey: "Combining media failed."])
      }
      return target.absoluteString
  }
}
