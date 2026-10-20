import ExpoModulesCore
import Foundation
import AVFoundation
import MediaPlayer
import UIKit
import WebKit

public final class GoVueCoreModule: Module {
  private var pendingExport: FileExport?
  private var preparingExport = false
  private let galleryLock = NSLock()
  private var galleryActive = 0
  public func definition() -> ModuleDefinition {
    Name("GoVueCore")
    Events("coreUpdate")
    View(CaptionSelectionView.self) {
      Events("onSelectionChange", "onContentSizeChange")
      Prop("text") { (view: CaptionSelectionView, value: String) in view.captionText = value }
      Prop("selectionStart") { (view: CaptionSelectionView, value: Int) in view.initialStart = value }
      Prop("selectionEnd") { (view: CaptionSelectionView, value: Int) in view.initialEnd = value }
      Prop("fontSize") { (view: CaptionSelectionView, value: Double) in view.fontSize = value }
      Prop("lineHeight") { (view: CaptionSelectionView, value: Double) in view.lineHeight = value }
      Prop("textColor") { (view: CaptionSelectionView, value: String) in view.textColor = value }
      Prop("selectionColor") { (view: CaptionSelectionView, value: String) in view.selectionColor = value }
      OnViewDidUpdateProps { (view: CaptionSelectionView) in view.update() }
    }
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
      installDownloadPlatform(platform: NativeDownloadPlatform())
      installCoreEvents(events: NativeCoreEvents { [weak self] channel in self?.sendEvent("coreUpdate", ["channel": channel]) })
      let mediaHttp = NativeMediaHttp()
      installMediaHttp(backend: mediaHttp)
      installDownloadHttp(backend: NativeDownloadHttp())
      installMediaStreamHttp(backend: mediaHttp)
      installExtractionHttp(backend: NativeExtractionHttp())
      installAiPlatform(platform: NativeAiPlatform())
    }
    Function("browserUrlPolicy") { (url: String) -> String in browserUrlPolicy(url: url) }
    Function("state") { (request: String) -> String in stateExecute(request: request) }
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

  }
}
