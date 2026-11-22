import AVFAudio
import Foundation

private final class ChatReadAloud: NSObject, AVSpeechSynthesizerDelegate {
    private let context: UnsafeRawPointer
    private var reply: SpeechReply?
    private let synthesizer = AVSpeechSynthesizer()
    private var active: UInt64?
    private var utterance: AVSpeechUtterance?
    private var parts: ArraySlice<String> = []

    init(context: UnsafeRawPointer, reply: @escaping SpeechReply) {
        self.context = context
        self.reply = reply
        super.init()
        synthesizer.delegate = self
        synthesizer.usesApplicationAudioSession = false
    }

    func command(_ name: String, _ values: [String: Any]) {
        guard let id = (values["id"] as? NSNumber)?.uint64Value, id > 0 else { return }
        if name == "stop" && active == id {
            stop()
        } else if name == "read", let chunks = values["chunks"] as? [String] {
            stop()
            active = id
            guard !chunks.isEmpty, chunks.count <= 4096,
                  chunks.allSatisfy({ !$0.isEmpty && $0.utf16.count <= 4000 }) else {
                finish("Could not read this response. Please try again.")
                return
            }
            guard !AVSpeechSynthesisVoice.speechVoices().isEmpty else {
                finish("No text-to-speech voice is available. Add one in your device’s text-to-speech settings.")
                return
            }
            parts = chunks[...]
            next()
        }
    }

    private func next() {
        guard let text = parts.popFirst() else { finish(); return }
        let utterance = AVSpeechUtterance(string: text)
        self.utterance = utterance
        synthesizer.speak(utterance)
    }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        let identity = ObjectIdentifier(utterance)
        DispatchQueue.main.async { [weak self] in
            guard let self, self.utterance.map(ObjectIdentifier.init) == identity else { return }
            self.next()
        }
    }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
        let identity = ObjectIdentifier(utterance)
        DispatchQueue.main.async { [weak self] in
            guard let self, self.utterance.map(ObjectIdentifier.init) == identity else { return }
            self.finish()
        }
    }

    private func finish(_ error: String? = nil) {
        guard let id = active, let reply else { return }
        stop()
        var payload: [String: Any] = ["id": id]
        if let error { payload["error"] = error }
        guard let bytes = try? JSONSerialization.data(withJSONObject: payload),
              let message = String(data: bytes, encoding: .utf8) else { return }
        message.withCString { reply(context, $0) }
    }

    private func stop() {
        active = nil
        utterance = nil
        parts = []
        synthesizer.stopSpeaking(at: .immediate)
    }

    func close() {
        reply = nil
        stop()
    }
}

@_cdecl("govue_chat_reader_create")
func govueChatReaderCreate(_ context: UnsafeRawPointer, _ reply: @escaping SpeechReply) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(ChatReadAloud(context: context, reply: reply)).toOpaque()
}

@_cdecl("govue_chat_reader_command")
func govueChatReaderCommand(_ handle: UnsafeMutableRawPointer, _ name: UnsafePointer<CChar>, _ json: UnsafePointer<CChar>) {
    let controller = Unmanaged<ChatReadAloud>.fromOpaque(handle).takeUnretainedValue()
    let values = (try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8))) as? [String: Any] ?? [:]
    controller.command(String(cString: name), values)
}

@_cdecl("govue_chat_reader_close")
func govueChatReaderClose(_ handle: UnsafeMutableRawPointer) {
    let controller = Unmanaged<ChatReadAloud>.fromOpaque(handle).takeRetainedValue()
    controller.close()
}
