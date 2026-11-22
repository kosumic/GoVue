import Foundation

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
