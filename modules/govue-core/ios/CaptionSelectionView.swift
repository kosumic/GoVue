import ExpoModulesCore
import UIKit

final class CaptionSelectionView: ExpoView {
  let onSelectionChange = EventDispatcher()
  let onContentSizeChange = EventDispatcher()
  private let surface = CaptionSelectionSurface(frame: .zero)

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    surface.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    surface.selectionChange = { [weak self] start, end in
      self?.onSelectionChange(["selectionStart": start, "selectionEnd": end])
    }
    surface.contentSizeChange = { [weak self] height in
      self?.onContentSizeChange(["height": height])
    }
    addSubview(surface)
  }

  var captionText: String {
    get { surface.captionText }
    set { surface.captionText = newValue }
  }

  var initialStart: Int {
    get { surface.initialStart }
    set { surface.initialStart = newValue }
  }

  var initialEnd: Int {
    get { surface.initialEnd }
    set { surface.initialEnd = newValue }
  }

  var fontSize: Double {
    get { surface.fontSize }
    set { surface.fontSize = newValue }
  }

  var lineHeight: Double {
    get { surface.lineHeight }
    set { surface.lineHeight = newValue }
  }

  var textColor: String {
    get { surface.textColor }
    set { surface.textColor = newValue }
  }

  var selectionColor: String {
    get { surface.selectionColor }
    set { surface.selectionColor = newValue }
  }

  func update() { surface.update() }
}
