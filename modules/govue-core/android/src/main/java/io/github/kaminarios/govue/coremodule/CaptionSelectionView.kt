package io.github.kaminarios.govue.coremodule

import android.content.Context
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.viewevent.EventDispatcher
import expo.modules.kotlin.views.ExpoView

class CaptionSelectionView(context: Context, appContext: AppContext) : ExpoView(context, appContext) {
  override val shouldUseAndroidLayout = true
  val onSelectionChange by EventDispatcher<Map<String, Int>>()
  val onContentSizeChange by EventDispatcher<Map<String, Float>>()
  private val surface = CaptionSelectionSurface(
    context,
    { start, end -> onSelectionChange(mapOf("selectionStart" to start, "selectionEnd" to end)) },
    { height -> onContentSizeChange(mapOf("height" to height)) },
  )

  init {
    addView(surface, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
  }

  var captionText: String
    get() = surface.captionText
    set(value) { surface.captionText = value }

  var initialStart: Int
    get() = surface.initialStart
    set(value) { surface.initialStart = value }

  var initialEnd: Int
    get() = surface.initialEnd
    set(value) { surface.initialEnd = value }

  var fontSize: Float
    get() = surface.fontSize
    set(value) { surface.fontSize = value }

  var lineHeight: Float
    get() = surface.lineHeight
    set(value) { surface.lineHeight = value }

  var textColor: String
    get() = surface.textColor
    set(value) { surface.textColor = value }

  var selectionColor: String
    get() = surface.selectionColor
    set(value) { surface.selectionColor = value }

  fun update() = surface.update()
}
