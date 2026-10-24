package io.github.kaminarios.govue.coremodule

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.view.inputmethod.InputMethodManager
import androidx.core.view.ContentInfoCompat
import androidx.core.view.ViewCompat
import com.facebook.react.bridge.ReactContext
import com.facebook.react.uimanager.UIManagerHelper
import com.facebook.react.views.textinput.ReactEditText
import io.github.kaminarios.govue.core.aiAttachmentSourceLimit
import java.io.File
import java.lang.ref.WeakReference
import java.util.UUID
import java.util.concurrent.Executors

/** Platform URI permissions and input integration; Rust owns file policy and decoding. */
internal class ChatAttachments(private val context: ReactContext, private val emit: (Map<String, Any?>) -> Unit) {
  private class Binding(val input: WeakReference<ReactEditText>) {
    val pending = mutableMapOf<String, File>()
  }
  private val bindings = mutableMapOf<String, Binding>()
  private val copies = Executors.newSingleThreadExecutor()
  private val main = android.os.Handler(android.os.Looper.getMainLooper())
  private val clipboard get() = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

  fun available(): Boolean = runCatching {
    val description = clipboard.primaryClipDescription ?: return false
    // Inspect type metadata only; copy contents when the user chooses Paste.
    (0 until description.mimeTypeCount).any {
      val mime = description.getMimeType(it)
      mime.startsWith("image/") || mime == "application/pdf" || mime == "text/uri-list"
    }
  }.getOrDefault(false)

  fun copyClipboard(): Map<String, Any?>? {
    val clip = runCatching { clipboard.primaryClip }.getOrNull() ?: return null
    for (index in 0 until clip.itemCount) {
      val uri = clip.getItemAt(index).uri ?: continue
      copy(uri, imageMime(clip))?.let { return it }
    }
    return null
  }

  fun bind(tag: Int, token: String) {
    val input = UIManagerHelper.getUIManagerForReactTag(context, tag)?.resolveView(tag) as? ReactEditText
      ?: error("Could not find the chat input.")
    bindings.filterValues { it.input.get() === input }.keys.toList().forEach(::unbind)
    unbind(token)
    bindings[token] = Binding(WeakReference(input))
    ViewCompat.setOnReceiveContentListener(input, arrayOf("image/*")) { _, payload -> receive(token, payload) }
    if (input.hasFocus()) (context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).restartInput(input)
  }
  fun unbind(token: String) {
    val binding = bindings.remove(token) ?: return
    binding.input.get()?.let { ViewCompat.setOnReceiveContentListener(it, null, null) }
    binding.pending.values.forEach { it.delete() }
  }
  fun take(token: String, uri: String): Boolean = bindings[token]?.pending?.remove(uri) != null
  fun close() { bindings.keys.toList().forEach(::unbind); copies.shutdown() }

  private fun receive(token: String, payload: ContentInfoCompat): ContentInfoCompat? {
    val fallback = imageMime(payload.clip)
    val parts = payload.partition { item -> item.uri?.let { uri ->
      (runCatching { context.contentResolver.getType(uri) }.getOrNull() ?: fallback)?.startsWith("image/") == true
    } == true }
    val images = parts.first ?: return payload
    copies.execute {
      try {
        for (index in 0 until images.clip.itemCount) {
          // The retained payload keeps the keyboard's temporary URI permission alive.
          val file = copy(images.clip.getItemAt(index).uri!!, fallback) ?: continue
          val uri = file["uri"] as String
          main.post {
            val binding = bindings[token]
            if (binding?.input?.get() == null) File(Uri.parse(uri).path!!).delete()
            else { binding.pending[uri] = File(Uri.parse(uri).path!!); emit(file + ("token" to token)) }
          }
        }
      } catch (error: Exception) {
        main.post { if (bindings.containsKey(token)) emit(mapOf("token" to token, "error" to (error.message ?: "Could not paste this image."))) }
      }
    }
    return parts.second
  }
  private fun imageMime(clip: ClipData): String? = (0 until clip.description.mimeTypeCount)
    .map(clip.description::getMimeType).firstOrNull { it.startsWith("image/") }

  private fun copy(uri: Uri, fallback: String?): Map<String, Any?>? {
    // Clipboard providers must grant access to their content. Do not turn a
    // supplied private-app pathname or our own FileProvider into a file read.
    if (uri.scheme != "content" || uri.authority?.startsWith("${context.packageName}.") == true) return null
    val resolver = context.contentResolver
    val name = runCatching {
      resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) cursor.getString(0) else null
      }
    }.getOrNull() ?: "clipboard"
    val directory = File(context.cacheDir, "chat-inputs").apply { check(isDirectory || mkdirs()) }
    val target = File(directory, UUID.randomUUID().toString())
    try {
      val input = resolver.openInputStream(uri) ?: return null
      input.use { source -> target.outputStream().use { output ->
        val buffer = ByteArray(65536)
        var total = 0L
        val limit = aiAttachmentSourceLimit().toLong()
        while (true) {
          val count = source.read(buffer)
          if (count < 0) break
          total += count
          check(total <= limit) { "This attachment is too large." }
          output.write(buffer, 0, count)
        }
        check(total > 0) { "This clipboard file is empty." }
      } }
      return mapOf("uri" to Uri.fromFile(target).toString(), "name" to name,
        "mimeType" to (runCatching { resolver.getType(uri) }.getOrNull() ?: fallback))
    } catch (error: Exception) { target.delete(); throw error }
  }
}
