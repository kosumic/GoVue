import AVFoundation
import MediaPlayer

typealias PlaybackCallback = @convention(c) (
  UnsafeRawPointer, UnsafePointer<CChar>, UInt64, UnsafePointer<CChar>, Double
) -> Void

private struct PlayingItem: Decodable {
  let generation: UInt64
  let video: String
  let title: String
  let author: String
  let position: Double
  let duration: Double?
  let rate: Double
  let playing: Bool
  let next: Bool
  let previous: Bool
}

@MainActor
private final class PlaybackControls {
  private let context: UnsafeRawPointer
  private let callback: PlaybackCallback
  private var observers: [(MPRemoteCommand, Any)] = []
  private var active: PlayingItem?
  private var usingAudio = false

  init(context: UnsafeRawPointer, callback: PlaybackCallback) {
    self.context = context
    self.callback = callback
    let commands = MPRemoteCommandCenter.shared()
    bind(commands.playCommand, action: "play")
    bind(commands.pauseCommand, action: "pause")
    bind(commands.togglePlayPauseCommand, action: "toggle")
    bind(commands.stopCommand, action: "pause")
    bind(commands.nextTrackCommand, action: "next")
    bind(commands.previousTrackCommand, action: "previous")
    bind(commands.changePlaybackPositionCommand, action: "seek")
  }

  private func bind(_ command: MPRemoteCommand, action: String) {
    let token = command.addTarget { [weak self] event in
      let position = (event as? MPChangePlaybackPositionCommandEvent)?.positionTime ?? 0
      DispatchQueue.main.async { self?.send(action, at: position) }
      return .success
    }
    observers.append((command, token))
  }

  private func send(_ action: String, at position: Double) {
    guard let item = active else { return }
    let name = action == "toggle" ? (item.playing ? "pause" : "play") : action
    name.withCString { name in
      item.video.withCString { video in
        callback(context, name, item.generation, video, position)
      }
    }
  }

  func update(_ raw: Data) {
    let item = try? JSONDecoder().decode(PlayingItem.self, from: raw)
    active = item
    let commands = MPRemoteCommandCenter.shared()
    commands.playCommand.isEnabled = item != nil
    commands.pauseCommand.isEnabled = item != nil
    commands.togglePlayPauseCommand.isEnabled = item != nil
    commands.stopCommand.isEnabled = item != nil
    commands.changePlaybackPositionCommand.isEnabled = (item?.duration ?? 0) > 0
    commands.nextTrackCommand.isEnabled = item?.next == true
    commands.previousTrackCommand.isEnabled = item?.previous == true
    guard let item else {
      MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
      audio(false)
      return
    }
    if item.playing { audio(true) }
    var info: [String: Any] = [
      MPMediaItemPropertyTitle: item.title,
      MPMediaItemPropertyArtist: item.author,
      MPNowPlayingInfoPropertyElapsedPlaybackTime: item.position,
      MPNowPlayingInfoPropertyPlaybackRate: item.playing ? item.rate : 0,
      MPNowPlayingInfoPropertyDefaultPlaybackRate: item.rate,
      MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.video.rawValue,
    ]
    if let duration = item.duration { info[MPMediaItemPropertyPlaybackDuration] = duration }
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
  }

  private func audio(_ enabled: Bool) {
    guard enabled != usingAudio else { return }
    let session = AVAudioSession.sharedInstance()
    do {
      if enabled {
        try session.setCategory(.playback, mode: .moviePlayback)
        try session.setActive(true)
      } else {
        try session.setActive(false, options: .notifyOthersOnDeactivation)
      }
      usingAudio = enabled
    } catch {}
  }

  func stop() {
    active = nil
    for (command, observer) in observers {
      command.removeTarget(observer)
      command.isEnabled = false
    }
    observers.removeAll()
    MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    audio(false)
  }
}

@_cdecl("govue_now_playing_create")
@MainActor
func createNowPlaying(context: UnsafeRawPointer, callback: PlaybackCallback) -> UnsafeMutableRawPointer {
  let controls = PlaybackControls(context: context, callback: callback)
  return Unmanaged.passRetained(controls).toOpaque()
}

@_cdecl("govue_now_playing_update")
@MainActor
func updateNowPlaying(handle: UnsafeMutableRawPointer, bytes: UnsafeRawPointer, count: Int) {
  guard count > 0, count <= 16_000 else { return }
  Unmanaged<PlaybackControls>.fromOpaque(handle).takeUnretainedValue().update(Data(bytes: bytes, count: count))
}

@_cdecl("govue_now_playing_close")
@MainActor
func closeNowPlaying(handle: UnsafeMutableRawPointer) {
  Unmanaged<PlaybackControls>.fromOpaque(handle).takeRetainedValue().stop()
}
