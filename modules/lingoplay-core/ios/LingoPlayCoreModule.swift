import ExpoModulesCore
import Foundation
import AVFoundation

private final class NativeMediaHttp: MediaHttp, @unchecked Sendable {
  private let session: URLSession = {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.timeoutIntervalForRequest = 12
    configuration.httpCookieStorage = nil
    return URLSession(configuration: configuration)
  }()
  func fetchRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaRangeReply {
    guard var components = URLComponents(string: url) else { return MediaRangeReply(status:0,body:Data()) }
    // Keep signed query bytes unchanged when appending the bounded range.
    let query = components.percentEncodedQuery ?? ""
    components.percentEncodedQuery = query + (query.isEmpty ? "" : "&") + "range=\(start)-\(end)"
    guard let address = components.url else { return MediaRangeReply(status:0,body:Data()) }
    var request = URLRequest(url:address)
    request.httpMethod = method
    if method == "POST" { request.httpBody = Data([0x78,0]) }
    request.setValue("identity",forHTTPHeaderField:"Accept-Encoding")
    request.setValue(userAgent,forHTTPHeaderField:"User-Agent")
    request.setValue(origin,forHTTPHeaderField:"Origin")
    request.setValue(origin+"/",forHTTPHeaderField:"Referer")
    let signal = DispatchSemaphore(value:0)
    var reply = MediaRangeReply(status:0,body:Data())
    let task = session.dataTask(with:request) { data,response,error in
      if error == nil, let response = response as? HTTPURLResponse, let data, UInt64(data.count) <= end-start+2 {
        reply = MediaRangeReply(status:Int32(response.statusCode),body:data)
      }
      signal.signal()
    }
    task.resume()
    if signal.wait(timeout:.now()+15) == .timedOut { task.cancel(); return MediaRangeReply(status:0,body:Data()) }
    return reply
  }
}

public final class LingoPlayCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("LingoPlayCore")
    OnCreate {
      installMediaHttp(backend: NativeMediaHttp())
      installAiPlatform(platform: NativeAiPlatform())
    }
    AsyncFunction("ai") { (request: String) -> String in
      let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("LingoPlay/transcripts", isDirectory: true)
      return aiExecute(request: request, storagePath: directory.path)
    }
    AsyncFunction("execute") { (request: String) -> String in
      let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("LingoPlay/transcripts", isDirectory: true)
      return execute(request: request, storagePath: directory.path)
    }
    AsyncFunction("mux") { (video: String, audio: String, output: String) async throws -> String in
      func local(_ value: String) throws -> URL {
        guard let url = URL(string: value), url.isFileURL,
          url.resolvingSymlinksInPath().path.hasPrefix(NSHomeDirectory() + "/") else {
          throw NSError(domain: "LingoPlay", code: 1, userInfo: [NSLocalizedDescriptionKey: "Media must be in app storage."])
        }
        return url
      }
      let target = try local(output)
      guard !FileManager.default.fileExists(atPath: target.path) else {
        throw NSError(domain: "LingoPlay", code: 2, userInfo: [NSLocalizedDescriptionKey: "Output already exists."])
      }
      let composition = AVMutableComposition()
      for (path, type) in [(video, AVMediaType.video), (audio, AVMediaType.audio)] {
        let asset = AVURLAsset(url: try local(path))
        guard let source = try await asset.loadTracks(withMediaType: type).first,
          let track = composition.addMutableTrack(withMediaType: type, preferredTrackID: kCMPersistentTrackID_Invalid) else {
          throw NSError(domain: "LingoPlay", code: 3, userInfo: [NSLocalizedDescriptionKey: "Missing media track."])
        }
        let duration = try await asset.load(.duration)
        try track.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: source, at: .zero)
      }
      guard let exporter = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough) else {
        throw NSError(domain: "LingoPlay", code: 4, userInfo: [NSLocalizedDescriptionKey: "Could not combine media."])
      }
      exporter.outputURL = target
      exporter.outputFileType = .mp4
      await withCheckedContinuation { continuation in
        exporter.exportAsynchronously { continuation.resume() }
      }
      guard exporter.status == .completed else {
        try? FileManager.default.removeItem(at: target)
        throw exporter.error ?? NSError(domain: "LingoPlay", code: 5, userInfo: [NSLocalizedDescriptionKey: "Combining media failed."])
      }
      return target.absoluteString
    }
  }
}
