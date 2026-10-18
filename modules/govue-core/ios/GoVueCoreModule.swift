import ExpoModulesCore
import Foundation
import AVFoundation
import MediaPlayer
import UIKit
import WebKit

private final class NativeMediaHttp: MediaHttp, @unchecked Sendable {
  func fetchRange(url: String, userAgent: String, origin: String, method: String, start: UInt64, end: UInt64) -> MediaRangeReply {
    guard end >= start, end - start < 1048576, var components = URLComponents(string: url), components.scheme == "https", components.user == nil, components.password == nil, components.port == nil || components.port == 443 else { return MediaRangeReply(status:0,body:Data()) }
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
    let task = BoundedHttp(request: request, limit: Int(end - start + 2), timeout: 12) { status, data, _ in
      reply = MediaRangeReply(status: Int32(status), body: data)
      signal.signal()
    }
    if signal.wait(timeout: .now() + 15) == .timedOut { task.cancel(); return MediaRangeReply(status: 0, body: Data()) }
    return reply
  }
}

public final class GoVueCoreModule: Module {
  private var pendingExport: FileExport?
  private var preparingExport = false
  private let galleryLock = NSLock()
  private var galleryActive = 0
  public func definition() -> ModuleDefinition {
    Name("GoVueCore")
    AsyncFunction("getBrowserUserAgent") { (promise: Promise) in
      let view = WKWebView(frame: .zero)
      view.evaluateJavaScript("navigator.userAgent") { [view] result, _ in
        _ = view
        if let agent = result as? String { promise.resolve(agent) }
        else { promise.reject("BROWSER_IDENTITY_UNAVAILABLE", "The browser identity is unavailable.") }
      }
    }.runOnQueue(.main)
    AsyncFunction("saveFile") { (uri: String, name: String, mime: String, promise: Promise) in
      guard pendingExport == nil, !preparingExport else {
        throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "A file picker is already open."])
      }
      guard let source = URL(string: uri) else {
        throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "Save As is unavailable."])
      }
      preparingExport = true
      DispatchQueue.global(qos: .userInitiated).async { [self] in
        do {
          let prepared = try FileExport.prepare(source: source, name: name)
          DispatchQueue.main.async { [self] in
            preparingExport = false
            guard let presenter = appContext?.utilities?.currentViewController(), presenter.viewIfLoaded?.window != nil else {
              try? FileManager.default.removeItem(at: prepared.folder)
              promise.reject("EXPORT_UNAVAILABLE", "Save As is unavailable.")
              return
            }
            let export = FileExport(prepared: prepared, promise: promise) { [weak self] in self?.pendingExport = nil }
            pendingExport = export
            presenter.present(export.controller, animated: true)
          }
        } catch {
          DispatchQueue.main.async { [self] in
            preparingExport = false
            promise.reject("EXPORT_FAILED", "The saved file could not be exported.")
          }
        }
      }
    }.runOnQueue(.main)
    AsyncFunction("galleryBytes") { (url: String, key: String, promise: Promise) in
      guard url.utf8.count <= 8192, let address = URL(string: url), address.scheme == "https",
        address.user == nil, address.password == nil, address.port == nil || address.port == 443,
        let host = address.host?.lowercased(), ["ggpht.com", "ytimg.com", "googleusercontent.com"].contains(where: { host == $0 || host.hasSuffix("." + $0) }),
        key.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else {
        throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "Unsupported image request."])
      }
      galleryLock.lock()
      guard galleryActive < 3 else {
        galleryLock.unlock()
        throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "Too many image requests."])
      }
      galleryActive += 1
      galleryLock.unlock()
      do {
        let root = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
          .appendingPathComponent("LingoPlay/transcripts/media/gallery", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let request = URLRequest(url: address)
        _ = BoundedHttp(request: request, limit: 20 * 1024 * 1024, destination: root.appendingPathComponent(key + ".part")) { [self] status, _, location in
          galleryLock.lock(); galleryActive -= 1; galleryLock.unlock()
          var result: [String: Any] = ["status": status]
          if let location { result["location"] = location }
          let data = (try? JSONSerialization.data(withJSONObject: result)) ?? Data("{\"status\":0}".utf8)
          promise.resolve(String(decoding: data, as: UTF8.self))
        }
      } catch {
        galleryLock.lock(); galleryActive -= 1; galleryLock.unlock()
        promise.resolve("{\"status\":0}")
      }
    }
    AsyncFunction("getSystemVolume") { () -> Double in Double(AVAudioSession.sharedInstance().outputVolume) }
    AsyncFunction("setSystemVolume") { (value: Double) async throws in
      guard value.isFinite else { throw NSError(domain: "GoVue", code: 1) }
      await MainActor.run {
        let volume = MPVolumeView(frame: .zero)
        volume.showsRouteButton = false
        if let slider = volume.subviews.compactMap({ $0 as? UISlider }).first {
          slider.setValue(Float(max(0, min(1, value))), animated: false)
          slider.sendActions(for: .touchUpInside)
        }
      }
    }
    AsyncFunction("supportedMediaFormats") { (raw: String) throws -> String in
      guard raw.utf8.count <= 200000, let data = raw.data(using: .utf8),
            let formats = try JSONSerialization.jsonObject(with: data) as? [[String: Any]], formats.count <= 500 else {
        throw NSError(domain: "GoVue", code: 1, userInfo: [NSLocalizedDescriptionKey: "Invalid media capabilities request."])
      }
      let keys = formats.compactMap { format -> String? in
        let mime = format["mimeType"] as? String ?? ""
        return (mime.hasPrefix("audio/mp4") && mime.contains("mp4a")) || (mime.hasPrefix("video/mp4") && ["avc1", "hvc1", "hev1"].contains(where: mime.contains)) ? format["key"] as? String : nil
      }
      return String(data: try JSONSerialization.data(withJSONObject: keys), encoding: .utf8)!
    }
    OnCreate {
      installMediaHttp(backend: NativeMediaHttp())
      installExtractionHttp(backend: NativeExtractionHttp())
      installAiPlatform(platform: NativeAiPlatform())
    }
    Function("dictation") { (request: String) -> String in dictationExecute(request: request) }
    AsyncFunction("ai") { (request: String) -> String in
      let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("LingoPlay/transcripts", isDirectory: true)
      return aiExecute(request: request, storagePath: directory.path)
    }.runOnQueue(.global(qos: .userInitiated))
    AsyncFunction("execute") { (request: String) -> String in
      let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("LingoPlay/transcripts", isDirectory: true)
      return execute(request: request, storagePath: directory.path)
    }.runOnQueue(.global(qos: .userInitiated))
    AsyncFunction("mux") { (video: String, audio: String, output: String) async throws -> String in
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
}
