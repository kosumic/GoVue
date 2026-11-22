import AVKit
import UIKit

typealias PipEvent = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>, UnsafePointer<CChar>) -> Void

private final class PlayerPip: NSObject, AVPictureInPictureControllerDelegate {
    weak var view: UIView?
    private let context: UnsafeRawPointer
    private var emit: PipEvent?
    private var generation: UInt64 = 0
    private var video = ""
    private var autoEnter = false
    private var controller: AVPictureInPictureController?
    private var playback: NSKeyValueObservation?
    private var retry: DispatchWorkItem?

    init(view: UIView, context: UnsafeRawPointer, emit: @escaping PipEvent) {
        self.view = view
        self.context = context
        self.emit = emit
    }

    func ready() {
        event("supported", AVPictureInPictureController.isPictureInPictureSupported())
    }

    func update(_ config: [String: Any]) {
        guard config["available"] as? Bool == true,
              let token = (config["generation"] as? NSNumber)?.uint64Value, token > 0,
              let video = config["videoId"] as? String, video.utf8.count == 11 else {
            retire()
            generation = 0
            self.video = ""
            return
        }
        if token != generation {
            retire()
            generation = token
            self.video = video
        }
        autoEnter = config["autoEnter"] as? Bool == true
        if let controller {
            controller.canStartPictureInPictureAutomaticallyFromInline = autoEnter
        } else if retry == nil {
            attach(token, attempts: 16)
        }
    }

    func start(_ token: UInt64) {
        guard token == generation, token > 0 else { return }
        retry?.cancel()
        retry = nil
        attach(token, attempts: 0)
        guard let controller, controller.isPictureInPicturePossible else {
            event("error", "Picture in Picture is unavailable for this video.")
            return
        }
        if !controller.isPictureInPictureActive { controller.startPictureInPicture() }
    }

    private func attach(_ token: UInt64, attempts: Int) {
        guard token == generation, controller == nil, AVPictureInPictureController.isPictureInPictureSupported() else { return }
        let identifier = "govue-video-\(token)"
        if let root = view?.window, let layer = govuePlayerLayer(in: root, identifier: identifier),
           let player = layer.player, let controller = AVPictureInPictureController(playerLayer: layer) {
            controller.delegate = self
            controller.canStartPictureInPictureAutomaticallyFromInline = autoEnter
            self.controller = controller
            playback = player.observe(\.timeControlStatus, options: [.new]) { [weak self] player, _ in
                DispatchQueue.main.async { [weak self, weak player] in
                    guard let self, let player, self.generation == token,
                          self.controller?.isPictureInPictureActive == true,
                          player.timeControlStatus != .waitingToPlayAtSpecifiedRate else { return }
                    self.event("playing", ["videoId": self.video, "generation": token,
                                           "playing": player.timeControlStatus == .playing])
                }
            }
            return
        }
        if attempts > 0 {
            let next = DispatchWorkItem { [weak self] in
                guard let self, self.generation == token else { return }
                self.retry = nil
                self.attach(token, attempts: attempts - 1)
            }
            retry = next
            DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(125), execute: next)
        }
    }

    func pictureInPictureControllerWillStartPictureInPicture(_ controller: AVPictureInPictureController) {
        if controller === self.controller { event("mode", true) }
    }

    func pictureInPictureControllerDidStopPictureInPicture(_ controller: AVPictureInPictureController) {
        if controller === self.controller { event("mode", false) }
    }

    func pictureInPictureController(_ controller: AVPictureInPictureController, failedToStartPictureInPictureWithError error: Error) {
        if controller === self.controller {
            event("mode", false)
            event("error", "Could not start Picture in Picture.")
        }
    }

    func pictureInPictureController(_ controller: AVPictureInPictureController, restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completion: @escaping (Bool) -> Void) {
        completion(controller === self.controller && view?.window != nil)
    }

    private func event(_ name: String, _ value: Any) {
        guard let emit, let bytes = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed]),
              let data = String(data: bytes, encoding: .utf8) else { return }
        name.withCString { name in data.withCString { emit(context, name, $0) } }
    }

    private func retire() {
        retry?.cancel()
        retry = nil
        playback?.invalidate()
        playback = nil
        guard let controller else { return }
        controller.canStartPictureInPictureAutomaticallyFromInline = false
        controller.delegate = nil
        if controller.isPictureInPictureActive { controller.stopPictureInPicture() }
        self.controller = nil
        event("mode", false)
    }

    func close() {
        emit = nil
        retire()
    }
}

@_cdecl("govue_ios_pip_create")
func govueIOSPipCreate(_ view: UnsafeRawPointer, _ context: UnsafeRawPointer, _ emit: @escaping PipEvent) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(PlayerPip(view: Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue(), context: context, emit: emit)).toOpaque()
}

@_cdecl("govue_ios_pip_update")
func govueIOSPipUpdate(_ handle: UnsafeMutableRawPointer, _ data: UnsafePointer<UInt8>, _ length: Int) {
    let pip = Unmanaged<PlayerPip>.fromOpaque(handle).takeUnretainedValue()
    let decoded = try? JSONSerialization.jsonObject(with: Data(bytes: data, count: length))
    pip.update(decoded as? [String: Any] ?? [:])
}

@_cdecl("govue_ios_pip_ready")
func govueIOSPipReady(_ handle: UnsafeMutableRawPointer) { Unmanaged<PlayerPip>.fromOpaque(handle).takeUnretainedValue().ready() }

@_cdecl("govue_ios_pip_start")
func govueIOSPipStart(_ handle: UnsafeMutableRawPointer, _ generation: UInt64) {
    Unmanaged<PlayerPip>.fromOpaque(handle).takeUnretainedValue().start(generation)
}

@_cdecl("govue_ios_pip_close")
func govueIOSPipClose(_ handle: UnsafeMutableRawPointer) { Unmanaged<PlayerPip>.fromOpaque(handle).takeRetainedValue().close() }
