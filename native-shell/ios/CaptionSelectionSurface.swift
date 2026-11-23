import UIKit

/// Native read-only selection; Rust validates and maps the UTF-16 range.
final class CaptionSelectionSurface: UIView, UITextViewDelegate {
  var selectionChange: ((Int, Int) -> Void)?
  var contentSizeChange: ((CGFloat) -> Void)?
  var captionText = ""
  var initialStart = 0
  var initialEnd = 0
  var fontSize: Double = 15
  var lineHeight: Double = 24
  var textColor = "#FFFFFF"
  var selectionColor = "#3EA6FF"
  private let text = UITextView()
  private var initialized = false
  private var applying = false
  private var reportedHeight: CGFloat = 0
  private var appliedStyle = ""

  override init(frame: CGRect) {
    super.init(frame: frame)
    text.isEditable = false
    text.isSelectable = true
    text.isScrollEnabled = false
    text.backgroundColor = .clear
    text.textContainerInset = .zero
    text.textContainer.lineFragmentPadding = 0
    text.delegate = self
    addSubview(text)
  }

  required init?(coder: NSCoder) { nil }

  private func color(_ hex: String) -> UIColor {
    let value = UInt32(hex.replacingOccurrences(of: "#", with: ""), radix: 16) ?? 0xFFFFFF
    return UIColor(red: CGFloat((value >> 16) & 255) / 255,
      green: CGFloat((value >> 8) & 255) / 255, blue: CGFloat(value & 255) / 255, alpha: 1)
  }

  func update() {
    guard initialStart >= 0, initialEnd > initialStart,
      initialEnd <= captionText.utf16.count else { return }
    applying = true
    let style = "\(fontSize):\(lineHeight):\(textColor)"
    if appliedStyle != style {
      let range = text.selectedRange
      let paragraph = NSMutableParagraphStyle()
      paragraph.minimumLineHeight = lineHeight
      paragraph.maximumLineHeight = lineHeight
      text.attributedText = NSAttributedString(string: captionText, attributes: [
        .font: UIFont.systemFont(ofSize: fontSize), .foregroundColor: color(textColor), .paragraphStyle: paragraph,
      ])
      if initialized { text.selectedRange = range }
      appliedStyle = style
    }
    text.tintColor = color(selectionColor)
    if initialized { applying = false; return }
    initialized = true
    setNeedsLayout()
    DispatchQueue.main.async { [weak self] in
      guard let self, self.window != nil else { return }
      self.text.becomeFirstResponder()
      self.text.selectedRange = NSRange(location: self.initialStart, length: self.initialEnd - self.initialStart)
      self.applying = false
    }
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    text.frame = bounds
    guard bounds.width > 0 else { return }
    let height = ceil(text.sizeThatFits(CGSize(width: bounds.width, height: .greatestFiniteMagnitude)).height)
    if height > 0 && height != reportedHeight {
      reportedHeight = height
      contentSizeChange?(height)
    }
  }

  func textViewDidChangeSelection(_ textView: UITextView) {
    let range = textView.selectedRange
    if initialized && !applying && range.location != NSNotFound {
      selectionChange?(range.location, NSMaxRange(range))
    }
  }
}
