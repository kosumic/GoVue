import AVFoundation
import UIKit
import Vision

typealias ProviderQrEvent = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>, UnsafePointer<CChar>) -> Void

private final class QrPreview: UIView {
    override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
    var video: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
}

private final class ProviderQrScanner: NSObject, AVCaptureMetadataOutputObjectsDelegate, @unchecked Sendable {
    private let context: UnsafeRawPointer
    private var emit: ProviderQrEvent?
    private let preview = QrPreview()
    private let session = AVCaptureSession()
    private let capture = DispatchQueue(label: "govue.provider-qr.camera")
    private var device: AVCaptureDevice?
    private var enabled = false
    private var handled = false
    private var torch = false
    private var requesting = false

    init(root: UIView, context: UnsafeRawPointer, emit: @escaping ProviderQrEvent) {
        self.context = context
        self.emit = emit
        super.init()
        preview.frame = root.bounds
        preview.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        preview.video.videoGravity = .resizeAspectFill
        preview.video.session = session
        root.addSubview(preview)
    }

    func setCamera(_ value: Bool) {
        guard enabled != value else { return }
        enabled = value
        handled = false
        preview.isHidden = !value
        if value { prepare(ask: true) }
        else { capture.async { [self] in stop() } }
    }

    func prepare(ask: Bool) {
        let status = AVCaptureDevice.authorizationStatus(for: .video)
        event("permission", ["granted": status == .authorized, "canAskAgain": status == .notDetermined])
        guard enabled else { return }
        if status == .authorized {
            let light = torch
            capture.async { [self] in start(torch: light) }
        } else if status == .notDetermined && ask && !requesting {
            requesting = true
            AVCaptureDevice.requestAccess(for: .video) { [weak self] _ in
                DispatchQueue.main.async {
                    self?.requesting = false
                    self?.prepare(ask: false)
                }
            }
        }
    }

    func permission() {
        if AVCaptureDevice.authorizationStatus(for: .video) == .denied {
            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
        } else {
            prepare(ask: true)
        }
    }

    func setTorch(_ value: Bool) {
        torch = value
        capture.async { [self] in applyTorch(value) }
    }

    private func start(torch: Bool) {
        if session.isRunning { return }
        if device == nil {
            do {
                try configure()
            } catch {
                DispatchQueue.main.async { [self] in
                    if enabled { event("error", "Could not start the camera. Close the scanner and try again.") }
                }
                return
            }
        }
        session.startRunning()
        applyTorch(torch)
    }

    private func configure() throws {
        let unavailable = NSError(domain: "GoVueProviderQr", code: 1)
        guard let camera = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back) else {
            throw unavailable
        }
        let input = try AVCaptureDeviceInput(device: camera)
        let output = AVCaptureMetadataOutput()
        session.beginConfiguration()
        defer { session.commitConfiguration() }
        guard session.canAddInput(input), session.canAddOutput(output) else { throw unavailable }
        session.addInput(input)
        session.addOutput(output)
        guard output.availableMetadataObjectTypes.contains(.qr) else {
            session.removeOutput(output)
            session.removeInput(input)
            throw unavailable
        }
        output.setMetadataObjectsDelegate(self, queue: .main)
        output.metadataObjectTypes = [.qr]
        device = camera
    }

    private func applyTorch(_ value: Bool) {
        guard let device, device.hasTorch, (try? device.lockForConfiguration()) != nil else { return }
        if !value || session.isRunning { device.torchMode = value ? .on : .off }
        device.unlockForConfiguration()
    }

    private func stop() {
        applyTorch(false)
        if session.isRunning { session.stopRunning() }
    }

    func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput metadataObjects: [AVMetadataObject], from connection: AVCaptureConnection) {
        guard enabled, !handled else { return }
        let codes = metadataObjects.compactMap { ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }
            .filter { $0.utf8.count <= 1000 }.prefix(10)
        if !codes.isEmpty {
            handled = true
            event("codes", ["values": Array(codes)])
        }
    }

    func readImage(_ input: [String: Any]) {
        guard let request = input["request"] as? String, let path = input["path"] as? String else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let url = URL(fileURLWithPath: path).resolvingSymlinksInPath()
            let home = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true).resolvingSymlinksInPath().path
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            let result: [String: Any]
            do {
                guard url.path.hasPrefix(home + "/"), (1...20_000_000).contains(size) else {
                    throw NSError(domain: "GoVueProviderQr", code: 2)
                }
                let scan = VNDetectBarcodesRequest()
                scan.symbologies = [.qr]
                try VNImageRequestHandler(url: url).perform([scan])
                let codes = (scan.results ?? []).compactMap(\.payloadStringValue)
                    .filter { $0.utf8.count <= 1000 }.prefix(10)
                result = ["request": request, "values": Array(codes)]
            } catch {
                result = ["request": request, "error": "Could not read the selected QR image."]
            }
            DispatchQueue.main.async { self?.event("codes", result) }
        }
    }

    private func event(_ name: String, _ data: Any) {
        guard let emit, let bytes = try? JSONSerialization.data(withJSONObject: data, options: .fragmentsAllowed),
              let json = String(data: bytes, encoding: .utf8) else { return }
        name.withCString { name in json.withCString { emit(context, name, $0) } }
    }

    func close() {
        emit = nil
        enabled = false
        preview.removeFromSuperview()
        capture.async { [self] in stop() }
    }
}

@_cdecl("govue_provider_qr_create")
func govueProviderQrCreate(_ view: UnsafeRawPointer, _ context: UnsafeRawPointer, _ emit: @escaping ProviderQrEvent) -> UnsafeMutableRawPointer {
    let view = Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue()
    return Unmanaged.passRetained(ProviderQrScanner(root: view, context: context, emit: emit)).toOpaque()
}

@_cdecl("govue_provider_qr_command")
func govueProviderQrCommand(_ handle: UnsafeMutableRawPointer, _ operation: Int32, _ bytes: UnsafePointer<UInt8>, _ length: Int) {
    guard length > 0 && length <= 3000,
          let value = try? JSONSerialization.jsonObject(with: Data(bytes: bytes, count: length), options: .fragmentsAllowed) else { return }
    let scanner = Unmanaged<ProviderQrScanner>.fromOpaque(handle).takeUnretainedValue()
    switch operation {
    case 0: scanner.setCamera(value as? Bool == true)
    case 1: scanner.setTorch(value as? Bool == true)
    case 2: scanner.permission()
    case 3: if let image = value as? [String: Any] { scanner.readImage(image) }
    default: break
    }
}

@_cdecl("govue_provider_qr_close")
func govueProviderQrClose(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<ProviderQrScanner>.fromOpaque(handle).takeRetainedValue().close()
}
