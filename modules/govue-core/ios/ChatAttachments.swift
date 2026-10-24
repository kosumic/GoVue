import ExpoModulesCore
import UIKit
import UniformTypeIdentifiers

private final class ChatPasteBinding: NSObject, UITextPasteDelegate {
  weak var input: UITextView?
  weak var original: UITextPasteDelegate?
  var configuration: UIPasteConfiguration?
  var pending = Set<URL>()
  var receive: ((NSItemProvider) -> Void)?
  func textPasteConfigurationSupporting(_ text: UITextPasteConfigurationSupporting, transform item: UITextPasteItem) {
    if item.itemProvider.hasItemConformingToTypeIdentifier(UTType.image.identifier)
      || item.itemProvider.canLoadObject(ofClass: UIImage.self) {
      item.setNoResult(); receive?(item.itemProvider)
    } else if let original, original.responds(to: NSSelectorFromString("textPasteConfigurationSupporting:transformPasteItem:")) {
      original.textPasteConfigurationSupporting?(text, transform: item)
    } else { item.setDefaultResult() }
  }
  override func responds(to selector: Selector!) -> Bool {
    super.responds(to: selector) || original?.responds(to: selector) == true
  }
  override func forwardingTarget(for selector: Selector!) -> Any? {
    original?.responds(to: selector) == true ? original : super.forwardingTarget(for: selector)
  }
  func detach() {
    if input?.pasteDelegate === self { input?.pasteDelegate = original; input?.pasteConfiguration = configuration }
    pending.forEach { try? FileManager.default.removeItem(at: $0) }; pending.removeAll()
  }
}

/// UIKit supplies local files and paste events; Rust validates their contents.
final class ChatAttachments {
  private var bindings = [String: ChatPasteBinding]()
  private let emit: ([String: Any]) -> Void
  init(emit: @escaping ([String: Any]) -> Void) { self.emit = emit }
  func available() -> Bool {
    let board = UIPasteboard.general
    return board.hasImages || board.contains(pasteboardTypes: [UTType.fileURL.identifier, UTType.pdf.identifier])
  }
  func clipboard(_ promise: Promise) {
    for provider in UIPasteboard.general.itemProviders {
      if copy(provider, imagesOnly: false, completion: { result in
        switch result {
        case .success(let file): promise.resolve(file)
        case .failure(let error): promise.reject("CHAT_ATTACHMENT", error.localizedDescription)
        }
      }) { return }
    }
    promise.resolve(nil)
  }
  private func textInput(_ view: UIView) -> UITextView? {
    if let input = view as? UITextView { return input }
    return view.subviews.lazy.compactMap { self.textInput($0) }.first
  }
  func bind(_ view: UIView, token: String) throws {
    guard let input = textInput(view) else { throw failure("Could not find the chat input.") }
    for key in Array(bindings.keys) where key == token || bindings[key]?.input === input { unbind(key) }
    let binding = ChatPasteBinding()
    binding.input = input; binding.original = input.pasteDelegate; binding.configuration = input.pasteConfiguration
    binding.receive = { [weak self, weak binding] provider in
      guard let self, let binding else { return }
      let started = self.copy(provider, imagesOnly: true) { [weak self, weak binding] result in
        DispatchQueue.main.async {
          guard let self, let binding, self.bindings[token] === binding, binding.input != nil else {
            if case .success(let file) = result, let uri = file["uri"] as? String, let url = URL(string: uri) { try? FileManager.default.removeItem(at: url) }
            return
          }
          switch result {
          case .success(var file):
            if let uri = file["uri"] as? String, let url = URL(string: uri) { binding.pending.insert(url) }
            file["token"] = token; self.emit(file)
          case .failure(let error): self.emit(["token": token, "error": error.localizedDescription])
          }
        }
      }
      if !started { self.emit(["token": token, "error": "Could not paste this image."]) }
    }
    let configuration = input.pasteConfiguration?.copy() as? UIPasteConfiguration
      ?? UIPasteConfiguration(forAccepting: NSString.self)
    configuration.addTypeIdentifiers(forAccepting: UIImage.self)
    bindings[token] = binding; input.pasteConfiguration = configuration; input.pasteDelegate = binding
  }
  func unbind(_ token: String) { bindings.removeValue(forKey: token)?.detach() }
  func take(_ token: String, uri: String) -> Bool {
    guard let url = URL(string: uri) else { return false }
    return bindings[token]?.pending.remove(url) != nil
  }
  func close() { for token in Array(bindings.keys) { unbind(token) } }
  private func failure(_ message: String) -> NSError {
    NSError(domain: "GoVueChatAttachment", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
  }
  private func stage(_ source: URL, name: String, mime: String) throws -> [String: Any] {
    guard source.isFileURL else { throw failure("Choose a local attachment file.") }
    let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].resolvingSymlinksInPath().path + "/"
    guard !source.resolvingSymlinksInPath().path.hasPrefix(support) else { throw failure("Choose a file from the file picker.") }
    let scoped = source.startAccessingSecurityScopedResource()
    defer { if scoped { source.stopAccessingSecurityScopedResource() } }
    let directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("chat-inputs", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let target = directory.appendingPathComponent(UUID().uuidString)
    do {
      guard (try source.resourceValues(forKeys: [.isRegularFileKey])).isRegularFile == true,
            let input = InputStream(url: source), let output = OutputStream(url: target, append: false) else { throw failure("Could not open this clipboard file.") }
      input.open(); output.open(); defer { input.close(); output.close() }
      var buffer = [UInt8](repeating: 0, count: 65536)
      var total: UInt64 = 0
      while true {
        let count = input.read(&buffer, maxLength: buffer.count)
        if count == 0 { break }
        guard count > 0 else { throw failure("Could not read this clipboard file.") }
        total += UInt64(count)
        guard total <= aiAttachmentSourceLimit() else { throw failure("This attachment is too large.") }
        try buffer.withUnsafeBufferPointer { pointer in
          var offset = 0
          while offset < count {
            let written = output.write(pointer.baseAddress!.advanced(by: offset), maxLength: count - offset)
            guard written > 0 else { throw failure("Could not save this clipboard file.") }
            offset += written
          }
        }
      }
      guard total > 0 else { throw failure("This clipboard file is empty.") }
      return ["uri": target.absoluteString, "name": name, "mimeType": mime]
    } catch { try? FileManager.default.removeItem(at: target); throw error }
  }
  private func copy(_ provider: NSItemProvider, imagesOnly: Bool, completion: @escaping (Result<[String: Any], Error>) -> Void) -> Bool {
    if !imagesOnly && provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) && provider.canLoadObject(ofClass: NSURL.self) {
      provider.loadObject(ofClass: NSURL.self) { [self] object, error in
        do {
          guard let source = object as? URL else { throw error ?? failure("Could not read the clipboard file.") }
          completion(.success(try stage(source, name: source.lastPathComponent, mime: UTType(filenameExtension: source.pathExtension)?.preferredMIMEType ?? "application/octet-stream")))
        } catch { completion(.failure(error)) }
      }
      return true
    }
    for identifier in provider.registeredTypeIdentifiers {
      guard let type = UTType(identifier), type.conforms(to: .image) || !imagesOnly && type.conforms(to: .pdf) else { continue }
      provider.loadFileRepresentation(forTypeIdentifier: identifier) { [self] source, error in
        do {
          guard let source else { throw error ?? failure("Could not read the clipboard image.") }
          completion(.success(try stage(source, name: provider.suggestedName ?? "clipboard.\(type.preferredFilenameExtension ?? "bin")", mime: type.preferredMIMEType ?? "application/octet-stream")))
        } catch { completion(.failure(error)) }
      }
      return true
    }
    if provider.canLoadObject(ofClass: UIImage.self) {
      provider.loadObject(ofClass: UIImage.self) { [self] object, error in
        do {
          guard let image = object as? UIImage, let data = image.pngData() else { throw error ?? failure("Could not read the clipboard image.") }
          guard UInt64(data.count) <= aiAttachmentSourceLimit() else { throw failure("This attachment is too large.") }
          let directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("chat-inputs", isDirectory: true)
          try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
          let target = directory.appendingPathComponent(UUID().uuidString)
          try data.write(to: target, options: .atomic)
          completion(.success(["uri": target.absoluteString, "name": "clipboard.png", "mimeType": "image/png"]))
        } catch { completion(.failure(error)) }
      }
      return true
    }
    return false
  }
}
