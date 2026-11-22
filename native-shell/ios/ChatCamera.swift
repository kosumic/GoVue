import AVFoundation
import UIKit

typealias CaptureReply = @convention(c) (UnsafeRawPointer, UInt64, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Void

private final class ChatCamera: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
    weak var view: UIView?
    private let context: UnsafeRawPointer
    private var reply: CaptureReply?
    private var pending: UInt64?
    private var picker: UIImagePickerController?

    init(view: UIView, context: UnsafeRawPointer, reply: @escaping CaptureReply) {
        self.view = view
        self.context = context
        self.reply = reply
    }

    func takePhoto(_ request: UInt64) {
        guard pending == nil else { return }
        pending = request
        guard UIImagePickerController.isSourceTypeAvailable(.camera) else {
            finish(error: "The camera is unavailable.")
            return
        }
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: showCamera()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { [weak self] allowed in
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.pending == request else { return }
                    if allowed { self.showCamera() }
                    else { self.finish(error: "Allow camera access in Settings to take a photo.") }
                }
            }
        default:
            finish(error: "Allow camera access in Settings to take a photo.")
        }
    }

    private func showCamera() {
        var presenter = view?.window?.rootViewController
        while let next = presenter?.presentedViewController { presenter = next }
        guard let presenter else {
            finish(error: "The camera is unavailable.")
            return
        }
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.delegate = self
        picker.modalPresentationStyle = .fullScreen
        self.picker = picker
        presenter.present(picker, animated: true)
    }

    func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
        self.picker = nil
        picker.dismiss(animated: true) { [weak self] in self?.finish() }
    }

    func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
        self.picker = nil
        let image = info[.originalImage] as? UIImage
        picker.dismiss(animated: true) { [weak self] in
            guard let self, let image else {
                self?.finish(error: "Could not read the photo.")
                return
            }
            do {
                self.finish(file: try self.stage(image))
            } catch {
                self.finish(error: "Could not read the photo.")
            }
        }
    }

    private func stage(_ image: UIImage) throws -> String {
        let pixels = max(image.size.width, image.size.height)
        guard pixels > 0 else { throw CocoaError(.fileReadCorruptFile) }
        let ratio = min(1600 / pixels, 1)
        let size = CGSize(width: max(1, (image.size.width * ratio).rounded()), height: max(1, (image.size.height * ratio).rounded()))
        let format = UIGraphicsImageRendererFormat()
        format.opaque = true
        format.scale = 1
        let jpeg = UIGraphicsImageRenderer(size: size, format: format).jpegData(withCompressionQuality: 0.86) { _ in
            image.draw(in: CGRect(origin: .zero, size: size))
        }
        let fileManager = FileManager.default
        let cache = try fileManager.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("govue-chat-camera", isDirectory: true)
        try fileManager.createDirectory(at: cache, withIntermediateDirectories: true)
        let name = UUID().uuidString + ".jpg"
        try jpeg.write(to: cache.appendingPathComponent(name), options: .atomic)
        return name
    }

    private func finish(file: String? = nil, error: String? = nil) {
        guard let request = pending, let reply else { return }
        pending = nil
        if let file { file.withCString { reply(context, request, $0, nil) } }
        else if let error { error.withCString { reply(context, request, nil, $0) } }
        else { reply(context, request, nil, nil) }
    }

    func close() {
        pending = nil
        reply = nil
        picker?.delegate = nil
        picker?.dismiss(animated: false)
        picker = nil
    }
}

@_cdecl("govue_chat_camera_create")
func govueChatCameraCreate(_ view: UnsafeRawPointer, _ context: UnsafeRawPointer, _ reply: @escaping CaptureReply) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(ChatCamera(view: Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue(), context: context, reply: reply)).toOpaque()
}

@_cdecl("govue_chat_camera_take")
func govueChatCameraTake(_ session: UnsafeMutableRawPointer, _ request: UInt64) {
    Unmanaged<ChatCamera>.fromOpaque(session).takeUnretainedValue().takePhoto(request)
}

@_cdecl("govue_chat_camera_close")
func govueChatCameraClose(_ session: UnsafeMutableRawPointer) {
    Unmanaged<ChatCamera>.fromOpaque(session).takeRetainedValue().close()
}
