package io.github.kaminarios.govue.coremodule

import android.content.Context
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.net.Uri
import android.os.Build
import io.github.kaminarios.govue.core.DownloadPlatform
import java.io.File
import java.nio.ByteBuffer
import org.json.JSONArray

internal class NativeDownloadPlatform(private val context: Context) : DownloadPlatform {
  companion object { private val mediaCapabilities = NativeMediaCapabilities() }
  override fun directory() = File(context.filesDir, "downloads").path
  override fun exportDirectory() = File(context.cacheDir, "download-exports").path
  override fun supportedFormats(formats: String): String = formats(formats)
  fun formats(raw: String): String {
      require(raw.length <= 200000) { "Too many media formats." }
      val formats = JSONArray(raw)
      require(formats.length() <= 500) { "Too many media formats." }
      val result = JSONArray()
      for (i in 0 until formats.length()) {
        val format = formats.getJSONObject(i)
        val codec = format.optString("mimeType")
        val mime = when {
          codec.startsWith("audio/mp4") && codec.contains("mp4a") -> "audio/mp4a-latm"
          codec.startsWith("audio/webm") && codec.contains("opus") -> "audio/opus"
          codec.contains("avc1") -> "video/avc"
          codec.contains("vp09") || codec.contains("vp9") -> "video/x-vnd.on2.vp9"
          codec.contains("av01") -> "video/av01"
          codec.contains("hvc1") || codec.contains("hev1") -> "video/hevc"
          else -> continue
        }
        val width = format.optInt("width")
        val height = format.optInt("height")
        val fps = format.optInt("fps")
        val supported = mediaCapabilities.supports(mime, width, height, fps)
        if (supported) result.put(format.optString("key"))
      }
      return result.toString()

  }
  override fun mux(video: String, audio: String, output: String): Boolean = try { muxFiles(video, audio, output); true } catch (_: Exception) { false }
  fun muxFiles(video: String, audio: String, output: String): String {
      fun local(uri: String): File {
        val parsed = Uri.parse(uri)
        require(parsed.scheme == "file") { "Only local media files can be combined." }
        val file = File(requireNotNull(parsed.path)).canonicalFile
        require(listOf(context.filesDir, context.cacheDir).any { file.path.startsWith(it.canonicalPath + File.separator) }) { "Media must be in app storage." }
        return file
      }
      val target = local(output)
      require(!target.exists()) { "Output already exists." }
      val extractors = mutableListOf<MediaExtractor>()
      var muxer: MediaMuxer? = null
      try {
        val inputs = listOf(local(video) to "video/", local(audio) to "audio/").map { (file, type) ->
          val extractor = MediaExtractor()
          extractors.add(extractor)
          extractor.setDataSource(file.path)
          val index = (0 until extractor.trackCount).firstOrNull { extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith(type) == true }
            ?: error("Missing ${type.dropLast(1)} track.")
          extractor.selectTrack(index)
          extractor to extractor.getTrackFormat(index)
        }
        val writer = MediaMuxer(target.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        muxer = writer
        val tracks = inputs.map { writer.addTrack(it.second) }
        writer.start()
        val buffer = ByteBuffer.allocate(16 * 1024 * 1024)
        inputs.forEachIndexed { index, (extractor, _) ->
          val info = MediaCodec.BufferInfo()
          while (true) {
            buffer.clear()
            val bytes = extractor.readSampleData(buffer, 0)
            if (bytes < 0) break
            val sampleFlags = extractor.sampleFlags
            require(sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC.inv() == 0) {
              "Only complete, unencrypted media samples can be combined."
            }
            // MediaMuxer accepts codec buffer flags, not extractor sample flags.
            val bufferFlags = if (sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
            info.set(0, bytes, extractor.sampleTime.coerceAtLeast(0), bufferFlags)
            writer.writeSampleData(tracks[index], buffer, info)
            extractor.advance()
          }
        }
        writer.stop()
        return target.toURI().toString()
      } catch (failure: Exception) {
        target.delete()
        throw failure
      } finally {
        extractors.forEach { runCatching { it.release() } }
        runCatching { muxer?.release() }
      }
  }
  override fun publish(id: String): Int = try { if (DownloadPublisher(context).publish(id)) 1 else 0 } catch (_: Exception) { -1 }
  override fun deletePublished(uri: String): Boolean = try { DownloadPublisher(context).delete(uri); true } catch (_: Exception) { false }
  override fun activity(count: UInt, title: String, received: ULong, total: ULong) {
    val intent = android.content.Intent(context, DownloadActivityService::class.java)
      .putExtra("count", count.toInt().coerceIn(0,2000)).putExtra("title", title.take(200))
      .putExtra("received", received.coerceAtMost(200_000_000_000uL).toLong())
      .putExtra("total", total.coerceAtMost(200_000_000_000uL).toLong())
    try {
      if (count > 0u) {
        if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent) else context.startService(intent)
      } else context.stopService(intent)
    } catch (_: Exception) { /* Android may defer service starts until foreground. */ }
  }
}
