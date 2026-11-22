import AVFoundation
import Speech
import UIKit

typealias SpeechReply = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>) -> Void

private final class ChatSpeech {
    private let context: UnsafeRawPointer
    private var reply: SpeechReply?
    private var owner: UInt64?
    private var engine: AVAudioEngine?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var recognizer: SFSpeechRecognizer?
    private var task: SFSpeechRecognitionTask?
    private var tapInstalled = false
    private var previousAudio: (AVAudioSession.Category, AVAudioSession.Mode, AVAudioSession.CategoryOptions)?

    init(context: UnsafeRawPointer, reply: @escaping SpeechReply) {
        self.context = context
        self.reply = reply
    }

    func command(_ name: String, _ values: [String: Any]) {
        if name == "settings" {
            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
            return
        }
        guard let id = (values["id"] as? NSNumber)?.uint64Value, id > 0 else { return }
        switch name {
        case "start":
            guard owner == nil else {
                emit(id, "error", ["message": "Speech recognition is finishing. Tap the microphone to try again."])
                return
            }
            owner = id
            let hints = (values["hints"] as? [String] ?? []).filter { $0.utf16.count <= 128 }.prefix(50)
            authorize(id, hints: Array(hints))
        case "finish":
            guard owner == id else { return }
            stopAudio()
            task?.finish()
        case "cancel":
            if owner == id { clear() }
        default: break
        }
    }

    private func authorize(_ id: UInt64, hints: [String]) {
        switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized:
            microphone(id, hints: hints)
        case .notDetermined:
            SFSpeechRecognizer.requestAuthorization { [weak self] in
                let granted = $0 == .authorized
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.owner == id else { return }
                    if granted { self.microphone(id, hints: hints) }
                    else { self.denied(id) }
                }
            }
        case .restricted, .denied: denied(id)
        @unknown default: denied(id)
        }
    }

    private func microphone(_ id: UInt64, hints: [String]) {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            listen(id, hints: hints)
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] allowed in
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.owner == id else { return }
                    if allowed { self.listen(id, hints: hints) }
                    else { self.denied(id) }
                }
            }
        case .restricted, .denied: denied(id)
        @unknown default: denied(id)
        }
    }

    private func listen(_ id: UInt64, hints: [String]) {
        guard owner == id else { return }
        guard UIApplication.shared.applicationState != .background else { clear(); return }
        guard let recognizer = SFSpeechRecognizer(locale: Locale.autoupdatingCurrent) else {
            fail(id, "Speech recognition does not support your device language.")
            return
        }
        guard recognizer.isAvailable else {
            fail(id, "Speech recognition is unavailable. Tap the microphone to try again.")
            return
        }
        self.recognizer = recognizer
        emit(id, "starting")
        do {
            let session = AVAudioSession.sharedInstance()
            previousAudio = (session.category, session.mode, session.categoryOptions)
            try session.setCategory(.playAndRecord, mode: .measurement, options: [.defaultToSpeaker, .duckOthers])
            try session.setActive(true)
            let engine = AVAudioEngine()
            let input = engine.inputNode
            let format = input.outputFormat(forBus: 0)
            guard format.sampleRate > 0, format.channelCount > 0 else {
                fail(id, "Speech recognition is unavailable. Tap the microphone to try again.")
                return
            }
            let request = SFSpeechAudioBufferRecognitionRequest()
            request.contextualStrings = hints
            request.taskHint = .dictation
            request.addsPunctuation = true
            request.shouldReportPartialResults = true
            self.engine = engine
            self.request = request
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in request.append(buffer) }
            tapInstalled = true
            engine.prepare()
            try engine.start()
            task = recognizer.recognitionTask(with: request) { [weak self] result, error in
                let complete = result?.isFinal ?? false
                let text = result?.bestTranscription.formattedString
                DispatchQueue.main.async { [weak self] in self?.receive(id, text: text, final: complete, failed: error != nil) }
            }
            emit(id, "listening")
        } catch {
            fail(id, "Speech recognition is unavailable. Tap the microphone to try again.")
        }
    }

    private func receive(_ id: UInt64, text: String?, final: Bool, failed: Bool) {
        guard owner == id else { return }
        if let text, text.utf16.count <= 16000 { emit(id, "result", ["text": text, "final": final]) }
        if final || failed {
            emit(id, "end")
            clear()
        }
    }

    private func denied(_ id: UInt64) {
        fail(id, "Allow microphone and speech recognition access in Settings to use voice input.", settings: true)
    }

    private func fail(_ id: UInt64, _ message: String, settings: Bool = false) {
        emit(id, "error", ["message": message, "settings": settings])
        clear()
    }

    private func emit(_ id: UInt64, _ kind: String, _ fields: [String: Any] = [:]) {
        guard let reply else { return }
        var payload = fields
        payload["id"] = id
        payload["kind"] = kind
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let json = String(data: data, encoding: .utf8) else { return }
        json.withCString { reply(context, $0) }
    }

    private func stopAudio() {
        engine?.stop()
        if tapInstalled {
            engine?.inputNode.removeTap(onBus: 0)
            tapInstalled = false
        }
        request?.endAudio()
    }

    private func clear() {
        owner = nil
        stopAudio()
        task?.cancel()
        task = nil
        recognizer = nil
        request = nil
        engine = nil
        if let (category, mode, options) = previousAudio {
            let session = AVAudioSession.sharedInstance()
            if session.category == .playAndRecord && session.mode == .measurement {
                try? session.setCategory(category, mode: mode, options: options)
            }
        }
        previousAudio = nil
    }

    func close() {
        reply = nil
        clear()
    }
}

@_cdecl("govue_chat_speech_create")
func govueChatSpeechCreate(_ context: UnsafeRawPointer, _ reply: @escaping SpeechReply) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(ChatSpeech(context: context, reply: reply)).toOpaque()
}

@_cdecl("govue_chat_speech_command")
func govueChatSpeechCommand(_ handle: UnsafeMutableRawPointer, _ name: UnsafePointer<CChar>, _ json: UnsafePointer<CChar>) {
    let controller = Unmanaged<ChatSpeech>.fromOpaque(handle).takeUnretainedValue()
    let values = (try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8))) as? [String: Any] ?? [:]
    controller.command(String(cString: name), values)
}

@_cdecl("govue_chat_speech_close")
func govueChatSpeechClose(_ handle: UnsafeMutableRawPointer) {
    let controller = Unmanaged<ChatSpeech>.fromOpaque(handle).takeRetainedValue()
    controller.close()
}
