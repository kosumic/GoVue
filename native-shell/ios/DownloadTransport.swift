import Foundation
import UIKit

private let downloadHttp = IosDownloadHttp()

@_cdecl("govue_download_wifi_connected")
func downloadWifiConnected() -> Bool {
  downloadHttp.wifiConnected()
}

typealias RangeCallback = @convention(c) (
  UnsafeMutableRawPointer, Int32, UnsafeRawPointer?, Int, UnsafePointer<CChar>?
) -> Void

@_cdecl("govue_download_fetch")
func downloadFetch(
  context: UnsafeMutableRawPointer,
  url: UnsafePointer<CChar>,
  userAgent: UnsafePointer<CChar>,
  origin: UnsafePointer<CChar>,
  method: UnsafePointer<CChar>,
  start: UInt64,
  end: UInt64,
  hasRange: Bool,
  wifiOnly: Bool,
  callback: RangeCallback
) {
  guard !hasRange || start <= end else {
    callback(context, 0, nil, 0, nil)
    return
  }
  let reply = downloadHttp.fetch(
    address: String(cString: url), userAgent: String(cString: userAgent),
    origin: String(cString: origin), method: String(cString: method),
    range: hasRange ? start...end : nil, wifiOnly: wifiOnly
  )
  reply.body.withUnsafeBytes { bytes in
    if let location = reply.location {
      location.withCString { callback(context, reply.status, bytes.baseAddress, bytes.count, $0) }
    } else {
      callback(context, reply.status, bytes.baseAddress, bytes.count, nil)
    }
  }
}

@_cdecl("govue_download_mux")
func downloadMux(
  video: UnsafePointer<CChar>,
  audio: UnsafePointer<CChar>,
  output: UnsafePointer<CChar>
) -> Bool {
  DownloadMux.write(
    video: String(cString: video), audio: String(cString: audio), output: String(cString: output)
  )
}

@MainActor
private enum SaveAs {
  static weak var view: UIView?
  static var active: FileExport?
  static var preparing = false

  static func presenter() -> UIViewController? {
    var controller = view?.window?.rootViewController
    while let presented = controller?.presentedViewController { controller = presented }
    return controller
  }

  static func open(view: UIView, source: URL, filename: String) -> Int32 {
    guard active == nil, !preparing else { return 2 }
    guard view.window != nil else { return 0 }
    self.view = view
    preparing = true
    DispatchQueue.global(qos: .userInitiated).async {
      let prepared = try? FileExport.prepare(source: source, name: filename)
      DispatchQueue.main.async {
        preparing = false
        guard let controller = presenter(), controller.viewIfLoaded?.window != nil else {
          if let prepared { try? FileManager.default.removeItem(at: prepared.folder) }
          return
        }
        guard let prepared else {
          let alert = UIAlertController(title: "GoVue", message: "The saved file could not be exported.", preferredStyle: .alert)
          alert.addAction(UIAlertAction(title: "OK", style: .default))
          controller.present(alert, animated: true)
          return
        }
        let picker = FileExport(prepared: prepared) { _ in active = nil }
        active = picker
        controller.present(picker.controller, animated: true)
      }
    }
    return 1
  }
}

@_cdecl("govue_download_save_file")
@MainActor
func downloadSaveFile(
  anchor: UnsafeRawPointer,
  uri: UnsafePointer<CChar>,
  filename: UnsafePointer<CChar>
) -> Int32 {
  guard let source = URL(string: String(cString: uri)) else { return 0 }
  let view = Unmanaged<UIView>.fromOpaque(anchor).takeUnretainedValue()
  return SaveAs.open(view: view, source: source, filename: String(cString: filename))
}
