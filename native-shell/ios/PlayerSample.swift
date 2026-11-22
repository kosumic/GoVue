import AVFoundation
import UIKit

func govuePlayerLayer(in view: UIView, identifier: String) -> AVPlayerLayer? {
    if view.accessibilityIdentifier == identifier {
        return view.layer.sublayers?.compactMap { $0 as? AVPlayerLayer }.first
    }
    for child in view.subviews {
        if let layer = govuePlayerLayer(in: child, identifier: identifier) { return layer }
    }
    return nil
}

@_cdecl("govue_ios_player_sample")
func govueIOSPlayerSample(
    _ generation: UInt64,
    _ offset: UnsafeMutablePointer<Double>,
    _ videoBitrate: UnsafeMutablePointer<Double>,
    _ audioBitrate: UnsafeMutablePointer<Double>
) {
    guard Thread.isMainThread else { return }
    let identifier = "govue-video-\(generation)"
    let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
    guard let layer = windows.lazy.compactMap({ govuePlayerLayer(in: $0, identifier: identifier) }).first,
          let item = layer.player?.currentItem else { return }
    let position = item.currentTime().seconds
    if position.isFinite,
       let end = item.seekableTimeRanges.map({ CMTimeRangeGetEnd($0.timeRangeValue).seconds })
            .filter({ $0.isFinite }).max() {
        offset.pointee = max(0, end - position)
    }
    if let last = item.accessLog()?.events.last {
        videoBitrate.pointee = last.averageVideoBitrate
        audioBitrate.pointee = last.averageAudioBitrate
    }
}
