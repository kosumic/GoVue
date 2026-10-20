package io.github.kaminarios.govue.coremodule

import android.content.Context
import android.graphics.Color
import android.graphics.Canvas
import android.graphics.Paint
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewTreeObserver
import android.widget.EditText
import android.widget.PopupWindow
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView

/** Native read-only text selection. Rust maps its UTF-16 offsets to caption words. */
class CaptionSelectionView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
  override val shouldUseAndroidLayout = true
  val onSelectionChange by EventDispatcher<Map<String, Int>>()
  val onContentSizeChange by EventDispatcher<Map<String, Float>>()
  var captionText = ""
  var initialStart = 0
  var initialEnd = 0
  var fontSize = 15f
  var lineHeight = 24f
  var textColor = "#FFFFFF"
  var selectionColor = "#3EA6FF"
  private var initialized = false
  private var applying = false
  private var reportedHeight = 0f
  private val scrollListener = ViewTreeObserver.OnScrollChangedListener { positionHandles() }
  private val text = object : EditText(context) {
    override fun onSelectionChanged(start: Int, end: Int) {
      super.onSelectionChanged(start, end)
      if (initialized && !applying && start >= 0 && end >= 0) {
        onSelectionChange(mapOf("selectionStart" to minOf(start, end), "selectionEnd" to maxOf(start, end)))
        positionHandles()
      }
    }
  }.apply {
    setBackgroundColor(Color.TRANSPARENT)
    setPadding(0, 0, 0, 0)
    includeFontPadding = false
    gravity = Gravity.TOP or Gravity.START
    setSingleLine(false)
    setHorizontallyScrolling(false)
    showSoftInputOnFocus = false
    keyListener = null
    setTextIsSelectable(true)
    isCursorVisible = false
    // The card supplies actions, and the two handles below own range gestures.
    // Avoid starting a second platform selection action mode on this text.
    isLongClickable = false
    setOnTouchListener { _, _ -> true }
  }
  private val beginning = SelectionHandle(true)
  private val ending = SelectionHandle(false)

  init {
    addView(text, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
  }

  fun update() {
    text.setTextColor(Color.parseColor(textColor))
    text.highlightColor = Color.parseColor(selectionColor) and 0x00ffffff or 0x66000000
    text.setTextSize(TypedValue.COMPLEX_UNIT_DIP, fontSize)
    val metrics = text.paint.fontMetricsInt
    text.setLineSpacing(lineHeight * resources.displayMetrics.density - (metrics.descent - metrics.ascent), 1f)
    if (!initialized && captionText.isNotEmpty() && initialStart >= 0 && initialEnd > initialStart && initialEnd <= captionText.length) {
      applying = true
      text.setText(captionText)
      text.requestFocus()
      text.setSelection(initialStart, initialEnd)
      initialized = true
    }
    requestLayout()
  }

  override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
    super.onLayout(changed, left, top, right, bottom)
    val layout = text.layout ?: return
    applying = false
    post { positionHandles() }
    val height = (layout.height + text.compoundPaddingTop + text.compoundPaddingBottom) / resources.displayMetrics.density
    if (height != reportedHeight) {
      reportedHeight = height
      onContentSizeChange(mapOf("height" to height))
    }
  }

  override fun onAttachedToWindow() {
    super.onAttachedToWindow()
    viewTreeObserver.addOnScrollChangedListener(scrollListener)
    post { positionHandles() }
  }

  override fun onDetachedFromWindow() {
    viewTreeObserver.removeOnScrollChangedListener(scrollListener)
    beginning.popup.dismiss()
    ending.popup.dismiss()
    super.onDetachedFromWindow()
  }

  override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
    super.onWindowFocusChanged(hasWindowFocus)
    // Defer until this view's fields exist if focus changes during construction.
    post { positionHandles() }
  }

  private fun positionHandles() {
    if (!initialized || !isAttachedToWindow || !hasWindowFocus() || windowVisibility != VISIBLE) {
      beginning.popup.dismiss()
      ending.popup.dismiss()
      return
    }
    beginning.position(text.selectionStart)
    ending.position(text.selectionEnd)
  }

  /** Small native popups keep the drag targets outside the caption's text bounds. */
  private inner class SelectionHandle(val start: Boolean) : View(context) {
    private val density = resources.displayMetrics.density
    private val radius = 10f * density
    private val size = (44f * density).toInt()
    private val stemX = (if (start) 32f else 12f) * density
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var touchOffsetX = 0f
    private var touchOffsetY = 0f
    private val location = IntArray(2)
    val popup = PopupWindow(this, size, size, false).apply {
      inputMethodMode = PopupWindow.INPUT_METHOD_NOT_NEEDED
      isClippingEnabled = false
      isSplitTouchEnabled = true
    }

    init {
      contentDescription = if (start) "Beginning of selection" else "End of selection"
      isClickable = true
    }

    override fun onDraw(canvas: Canvas) {
      super.onDraw(canvas)
      paint.color = Color.parseColor(selectionColor)
      val center = 22f * density
      canvas.drawCircle(center, radius, radius, paint)
      canvas.drawRect(if (start) center else center - radius, 0f,
        if (start) center + radius else center, radius, paint)
    }

    fun position(offset: Int) {
      val layout = text.layout ?: return
      if (offset < 0 || offset > text.length() || text.width == 0) return
      val line = layout.getLineForOffset(offset)
      text.getLocationInWindow(location)
      val x = location[0] + text.totalPaddingLeft + layout.getPrimaryHorizontal(offset) - text.scrollX
      val y = location[1] + text.totalPaddingTop + layout.getLineBottom(line) - text.scrollY
      invalidate()
      if (popup.isShowing) popup.update((x - stemX).toInt(), y, size, size)
      else popup.showAtLocation(text, Gravity.TOP or Gravity.LEFT, (x - stemX).toInt(), y)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
      when (event.actionMasked) {
        MotionEvent.ACTION_DOWN -> {
          val layout = text.layout ?: return false
          val offset = if (start) text.selectionStart else text.selectionEnd
          text.getLocationOnScreen(location)
          touchOffsetX = event.rawX - (location[0] + text.totalPaddingLeft + layout.getPrimaryHorizontal(offset) - text.scrollX)
          touchOffsetY = event.rawY - (location[1] + text.totalPaddingTop + layout.getLineBottom(layout.getLineForOffset(offset)) - text.scrollY - 1)
          return true
        }
        MotionEvent.ACTION_MOVE -> {
          text.getLocationOnScreen(location)
          val offset = text.getOffsetForPosition(event.rawX - location[0] - touchOffsetX, event.rawY - location[1] - touchOffsetY)
          if (start) text.setSelection(offset.coerceIn(0, text.selectionEnd - 1), text.selectionEnd)
          else text.setSelection(text.selectionStart, offset.coerceIn(text.selectionStart + 1, text.length()))
          return true
        }
        MotionEvent.ACTION_UP -> { performClick(); return true }
        MotionEvent.ACTION_CANCEL -> return true
      }
      return super.onTouchEvent(event)
    }

    override fun performClick(): Boolean { super.performClick(); return true }
  }
}
