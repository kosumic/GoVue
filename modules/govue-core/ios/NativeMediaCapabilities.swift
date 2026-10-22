enum NativeMediaCapabilities {
  static func supportedKeys<Formats: Sequence>(_ formats: Formats) -> [String] where Formats.Element == [String: Any] {
    formats.compactMap { format -> String? in
      let mime = format["mimeType"] as? String ?? ""
      let audio = mime.hasPrefix("audio/mp4") && mime.contains("mp4a")
      let video = mime.hasPrefix("video/mp4") && ["avc1", "hvc1", "hev1"].contains(where: mime.contains)
      return audio || video ? format["key"] as? String : nil
    }
  }
}
