package io.github.kaminarios.lingoplay.coremodule

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.provider.MediaStore
import androidx.annotation.RequiresApi
import io.github.kaminarios.lingoplay.core.execute
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest

/** OS byte I/O only. Rust owns filenames, artifact identity and publication state. */
internal class DownloadPublisher(private val context: Context) {
  companion object {
    private class Slot(val lock: Any = Any(), var users: Int = 0)
    private val slots = mutableMapOf<String, Slot>()
  }
  private val resolver get() = context.contentResolver
  private val relativePath = Environment.DIRECTORY_DOWNLOADS + "/LingoPlay/"
  private val root = File(context.filesDir, "transcripts").path
  private fun core(command: JSONObject): JSONObject {
    val reply = JSONObject(execute(command.toString(), root))
    check(reply.optBoolean("ok")) { "The download changed before it could be saved." }
    return reply.getJSONObject("value")
  }
  private fun command(op: String, id: String) = JSONObject().put("op", op).put("id", id)
  private data class Saved(val bytes: Long, val pending: Boolean)

  private fun ownedUri(value: String): Uri {
    require(value.matches(Regex("content://media/(external|external_primary)/downloads/[0-9]{1,20}"))) { "Invalid download identity." }
    return Uri.parse(value)
  }
  /** Check UID ownership as well as the scoped directory before any write/delete. */
  private fun inspect(uri: Uri): Saved? {
    resolver.query(uri, arrayOf(MediaStore.MediaColumns.OWNER_PACKAGE_NAME, MediaStore.MediaColumns.RELATIVE_PATH,
      MediaStore.MediaColumns.SIZE, MediaStore.MediaColumns.IS_PENDING), null, null, null)?.use { cursor ->
      if (!cursor.moveToFirst()) return null
      check(cursor.getString(0) == context.packageName && cursor.getString(1) == relativePath) { "The file is not owned by this download library." }
      return Saved(cursor.getLong(2), cursor.getInt(3) != 0)
    }
    return null
  }
  private fun local(file: String): File {
    require(file.matches(Regex("[A-Za-z0-9][A-Za-z0-9_.-]{0,254}")) && !file.contains(".."))
    val directory = File(context.filesDir, "downloads").canonicalFile
    val source = File(directory, file).canonicalFile
    check(source.parentFile == directory && source.isFile && source.length() in 1..200_000_000_000L) { "The saved source file is missing." }
    return source
  }
  @RequiresApi(29)
  private fun create(name: String, mime: String): Uri {
    require(name.isNotBlank() && name.length <= 255 && !name.contains(Regex("[\\\\/:*?\"<>|\\r\\n]")))
    require(mime in listOf("video/mp4", "audio/mp4", "image/jpeg", "application/x-subrip"))
    val values = ContentValues().apply {
      put(MediaStore.MediaColumns.DISPLAY_NAME, name)
      put(MediaStore.MediaColumns.MIME_TYPE, mime)
      put(MediaStore.MediaColumns.RELATIVE_PATH, relativePath)
      put(MediaStore.MediaColumns.IS_PENDING, 1)
      put(MediaStore.MediaColumns.DATE_EXPIRES, System.currentTimeMillis() / 1000 + 86400)
    }
    return requireNotNull(resolver.insert(MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values))
  }
  private fun complete(uri: Uri) {
    requireNotNull(inspect(uri)) { "The published file is missing." }
    val values = ContentValues().apply {
      put(MediaStore.MediaColumns.IS_PENDING, 0)
      putNull(MediaStore.MediaColumns.DATE_EXPIRES)
    }
    check(resolver.update(uri, values, null, null) == 1) { "The saved file could not be finalized." }
  }
  fun publish(id: String): Boolean {
    if (Build.VERSION.SDK_INT < 29) return false
    val slot = synchronized(slots) {
      slots.getOrPut(id) { check(slots.size < 16); Slot() }.also { it.users++ }
    }
    try { return synchronized(slot.lock) { publishLocked(id) } }
    finally { synchronized(slots) { slot.users--; if (slot.users == 0) slots.remove(id) } }
  }
  @RequiresApi(29)
  private fun publishLocked(id: String): Boolean {
    val plan = core(command("downloads_publication_plan", id)).getJSONObject("publication")
    val generation = plan.getLong("generation")
    var checkedAt = 0L
    fun current(force: Boolean = false) {
      val now = android.os.SystemClock.elapsedRealtime()
      if (force || now - checkedAt >= 1000) {
        core(command("downloads_publication_check", id).put("generation", generation))
        checkedAt = now
      }
    }
    val artifacts = plan.getJSONArray("artifacts")
    for (index in 0 until artifacts.length()) {
      val artifact = artifacts.getJSONObject(index)
      val file = artifact.getString("file")
      val previous = artifact.optString("uri", "").takeUnless { it.isEmpty() || it == "null" }
      var target = previous?.let(::ownedUri)
      val saved = target?.let(::inspect)
      if (artifact.optBoolean("published") && saved != null) {
        check(saved.bytes > 0 && saved.bytes == artifact.optLong("bytes", -1)) { "The published file changed." }
        current(true)
        if (saved.pending) complete(requireNotNull(target))
        continue
      }
      check(plan.optString("state") != "complete" || previous == null || saved != null) { "The published file was removed." }
      val source = local(file)
      val sourceLength = source.length()
      if (saved == null) {
        target = create(artifact.getString("name"), artifact.getString("mimeType"))
        try {
          core(command("downloads_publication_target", id).put("generation", generation).put("file", file).put("uri", target.toString()).put("previousUri", previous ?: JSONObject.NULL))
        } catch (error: Exception) {
          // The target never became part of Rust's history. It is still pending.
          delete(target.toString())
          throw error
        }
      }
      val uri = requireNotNull(target)
      requireNotNull(inspect(uri))
      current(true)
      check(resolver.update(uri, ContentValues().apply {
        put(MediaStore.MediaColumns.IS_PENDING, 1)
        put(MediaStore.MediaColumns.DATE_EXPIRES, System.currentTimeMillis() / 1000 + 86400)
      }, null, null) == 1)
      val sourceHash = MessageDigest.getInstance("SHA-256")
      var sourceBytes = 0L
      val descriptor = requireNotNull(resolver.openFileDescriptor(uri, "rwt"))
      ParcelFileDescriptor.AutoCloseOutputStream(descriptor).use { output ->
        source.inputStream().use { input ->
          val buffer = ByteArray(64 * 1024)
          while (true) {
            current()
            val count = input.read(buffer)
            if (count < 0) break
            sourceBytes += count
            check(sourceBytes <= 200_000_000_000L)
            sourceHash.update(buffer, 0, count)
            output.write(buffer, 0, count)
          }
        }
        output.flush()
        output.fd.sync()
      }
      val copiedHash = MessageDigest.getInstance("SHA-256")
      var copiedBytes = 0L
      requireNotNull(resolver.openInputStream(uri)).use { input ->
        val buffer = ByteArray(64 * 1024)
        while (true) {
          current()
          val count = input.read(buffer)
          if (count < 0) break
          copiedBytes += count
          check(copiedBytes <= sourceBytes)
          copiedHash.update(buffer, 0, count)
        }
      }
      fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
      core(command("downloads_publication_verified", id).put("generation", generation).put("file", file).put("uri", uri.toString())
        .put("sourceBytes", sourceLength).put("copiedBytes", copiedBytes)
        .put("sourceSha256", hex(sourceHash.digest())).put("copiedSha256", hex(copiedHash.digest())))
      complete(uri)
    }
    return true
  }
  fun delete(value: String) {
    if (Build.VERSION.SDK_INT < 29) return
    val uri = ownedUri(value)
    if (inspect(uri) != null) resolver.delete(uri, null, null)
  }
}
