import ExpoModulesCore
import UIKit

/// The system Save As adapter. Owned-artifact selection and names come from Rust.
@MainActor
final class FileExport: NSObject, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {
  private let promise: Promise
  private let folder: URL
  private let finished: () -> Void
  private var completed = false
  let controller: UIDocumentPickerViewController

  struct Prepared: Sendable {
    let folder: URL
    let file: URL
  }

  nonisolated static func prepare(source: URL, name: String) throws -> Prepared {
    let manager = FileManager.default
    let canonical = source.resolvingSymlinksInPath().standardizedFileURL
    let roots = [manager.urls(for: .documentDirectory, in: .userDomainMask)[0],
                 manager.urls(for: .cachesDirectory, in: .userDomainMask)[0],
                 manager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("LingoPlay/transcripts/media")]
    guard source.isFileURL, roots.contains(where: { canonical.path.hasPrefix($0.resolvingSymlinksInPath().path + "/") }),
          (try? canonical.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true,
          !name.isEmpty, name.count <= 255, name != ".", name != "..",
          name.range(of: #"[\\/:*?"<>|\r\n]"#, options: .regularExpression) == nil else {
      throw NSError(domain: "LingoPlay", code: 1, userInfo: [NSLocalizedDescriptionKey: "Only saved app files can be exported."])
    }
    let exportFolder = roots[1].appendingPathComponent("lingoplay-exports/" + UUID().uuidString, isDirectory: true)
    try manager.createDirectory(at: exportFolder, withIntermediateDirectories: true)
    let copy = exportFolder.appendingPathComponent(name)
    do { try manager.copyItem(at: canonical, to: copy) }
    catch { try? manager.removeItem(at: exportFolder); throw error }
    return Prepared(folder: exportFolder, file: copy)
  }

  init(prepared: Prepared, promise: Promise, finished: @escaping () -> Void) {
    self.promise = promise
    self.finished = finished
    folder = prepared.folder
    controller = UIDocumentPickerViewController(forExporting: [prepared.file], asCopy: true)
    super.init()
    controller.delegate = self
    controller.presentationController?.delegate = self
  }

  private func complete(_ saved: Bool) {
    guard !completed else { return }
    completed = true
    try? FileManager.default.removeItem(at: folder)
    promise.resolve(saved)
    finished()
  }
  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { complete(!urls.isEmpty) }
  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { complete(false) }
  func presentationControllerDidDismiss(_ presentationController: UIPresentationController) { complete(false) }
}
