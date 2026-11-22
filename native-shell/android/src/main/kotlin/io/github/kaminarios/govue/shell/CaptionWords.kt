package io.github.kaminarios.govue.shell

import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext
import com.longbridge.nsh.NshView
import io.github.kaminarios.govue.coremodule.CaptionSelectionSurface

class CaptionWords(private val context: NshComponentContext) : NshComponent {
    private val selector = CaptionSelectionSurface(
        context.context,
        { start, end -> context.emit("selection", mapOf("selectionStart" to start, "selectionEnd" to end)) },
        { height -> context.emit("size", height) },
    )

    override fun view() = selector

    override fun setProp(name: String, value: Any?) {
        if (name != "selection" || value !is Map<*, *>) return
        val text = value["text"] as? String ?: return
        val start = (value["selectionStart"] as? Number)?.toInt() ?: return
        val end = (value["selectionEnd"] as? Number)?.toInt() ?: return
        if (text.length > 50_000 || start < 0 || end <= start || end > text.length) return
        selector.captionText = text
        selector.initialStart = start
        selector.initialEnd = end
        selector.textColor = if (value["dark"] == true) "#FFFFFF" else "#0F0F0F"
        selector.update()
    }

    companion object {
        @JvmStatic
        fun register(view: NshView) = view.registerComponent("govue/caption-words", ::CaptionWords)
    }
}
