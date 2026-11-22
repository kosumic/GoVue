import SwiftUI
import Translation
import UIKit

typealias WordToolEvent = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>, UnsafePointer<CChar>) -> Void

@available(iOS 18.0, *)
private struct WordTranslationTask: View {
    let source: String
    let target: String
    let input: String
    let finish: (String?, String?) -> Void

    var body: some View {
        Color.clear.translationTask(
            source: Locale.Language(identifier: source),
            target: Locale.Language(identifier: target)
        ) { session in
            do {
                let result = try await session.translate(input)
                await MainActor.run { finish(result.targetText, nil) }
            } catch {
                await MainActor.run { finish(nil, "Could not translate this word. Please try again.") }
            }
        }
    }
}

private final class WordToolsHost {
    private weak var root: UIView?
    private let context: UnsafeRawPointer
    private var emit: WordToolEvent?
    private var selection: CaptionSelectionSurface?
    private var task: UIViewController?
    private var request: String?

    init(root: UIView, selector: Bool, context: UnsafeRawPointer, emit: @escaping WordToolEvent) {
        self.root = root
        self.context = context
        self.emit = emit
        if selector {
            let selection = CaptionSelectionSurface(frame: root.bounds)
            selection.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            selection.selectionChange = { [weak self] start, end in
                self?.event("selection", ["selectionStart": start, "selectionEnd": end])
            }
            selection.contentSizeChange = { [weak self] height in self?.event("size", height) }
            root.addSubview(selection)
            self.selection = selection
        }
    }

    func select(_ value: [String: Any]) {
        guard let selection, let text = value["text"] as? String, text.utf16.count <= 50_000,
              let start = value["selectionStart"] as? Int, let end = value["selectionEnd"] as? Int,
              start >= 0, end > start, end <= text.utf16.count else { return }
        selection.captionText = text
        selection.initialStart = start
        selection.initialEnd = end
        selection.textColor = value["dark"] as? Bool == true ? "#FFFFFF" : "#0F0F0F"
        selection.update()
    }

    func translate(_ value: [String: Any]) {
        guard let id = value["request"] as? String, id.utf8.count <= 100,
              let source = value["source"] as? String, source.utf8.count <= 32,
              let target = value["target"] as? String, target.utf8.count <= 32,
              let input = value["input"] as? String, input.utf8.count <= 4_000 else { return }
        retire()
        guard #available(iOS 18.0, *) else {
            event("translation", ["request": id, "error": "Word translation requires iOS 18 or later."])
            return
        }
        guard let root, let parent = controller(for: root) else {
            event("translation", ["request": id, "error": "Could not open word translation."])
            return
        }
        request = id
        let task = UIHostingController(rootView: WordTranslationTask(source: source, target: target, input: input) { [weak self] text, error in
            guard let self, self.request == id else { return }
            self.retire()
            if let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                self.event("translation", ["request": id, "text": text])
            } else {
                self.event("translation", ["request": id, "error": error ?? "No translation was returned. Please try again."])
            }
        })
        task.view.frame = root.bounds
        task.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        task.view.backgroundColor = .clear
        parent.addChild(task)
        root.addSubview(task.view)
        task.didMove(toParent: parent)
        self.task = task
    }

    func cancel(_ id: String) {
        if request == id { retire() }
    }

    private func controller(for view: UIView) -> UIViewController? {
        var responder: UIResponder? = view
        while let current = responder {
            if let controller = current as? UIViewController { return controller }
            responder = current.next
        }
        return view.window?.rootViewController
    }

    private func retire() {
        request = nil
        if let task {
            task.willMove(toParent: nil)
            task.view.removeFromSuperview()
            task.removeFromParent()
        }
        task = nil
    }

    private func event(_ name: String, _ value: Any) {
        guard let emit, let data = try? JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed),
              let raw = String(data: data, encoding: .utf8) else { return }
        name.withCString { name in raw.withCString { emit(context, name, $0) } }
    }

    func close() {
        selection?.selectionChange = nil
        selection?.contentSizeChange = nil
        selection?.removeFromSuperview()
        selection = nil
        retire()
        emit = nil
    }
}

@_cdecl("govue_ios_word_tools_create")
func govueIOSWordToolsCreate(_ view: UnsafeRawPointer, _ selection: Bool, _ context: UnsafeRawPointer, _ emit: @escaping WordToolEvent) -> UnsafeMutableRawPointer {
    let root = Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue()
    return Unmanaged.passRetained(WordToolsHost(root: root, selector: selection, context: context, emit: emit)).toOpaque()
}

@_cdecl("govue_ios_word_tools_request")
func govueIOSWordToolsRequest(_ handle: UnsafeMutableRawPointer, _ action: Int32, _ data: UnsafePointer<UInt8>, _ length: Int) {
    guard length > 0 && length <= 100_000,
          let value = try? JSONSerialization.jsonObject(with: Data(bytes: data, count: length), options: .fragmentsAllowed) else { return }
    let host = Unmanaged<WordToolsHost>.fromOpaque(handle).takeUnretainedValue()
    switch action {
    case 0: if let value = value as? [String: Any] { host.select(value) }
    case 1: if let value = value as? [String: Any] { host.translate(value) }
    case 2: if let id = value as? String { host.cancel(id) }
    default: break
    }
}

@_cdecl("govue_ios_word_tools_close")
func govueIOSWordToolsClose(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WordToolsHost>.fromOpaque(handle).takeRetainedValue().close()
}
