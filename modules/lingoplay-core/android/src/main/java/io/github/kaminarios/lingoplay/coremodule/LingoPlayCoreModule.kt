package io.github.kaminarios.lingoplay.coremodule

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import io.github.kaminarios.lingoplay.core.execute
import io.github.kaminarios.lingoplay.core.aiExecute
import io.github.kaminarios.lingoplay.core.installAiPlatform
import java.io.File
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.net.Uri
import java.nio.ByteBuffer
import io.github.kaminarios.lingoplay.core.MediaHttp
import io.github.kaminarios.lingoplay.core.MediaRangeReply
import io.github.kaminarios.lingoplay.core.installMediaHttp
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

private class NativeMediaHttp : MediaHttp {
  private val client = OkHttpClient.Builder().connectTimeout(8, TimeUnit.SECONDS)
    .readTimeout(12, TimeUnit.SECONDS).followRedirects(false).build()
  override fun fetchRange(url: String, userAgent: String, origin: String, method: String, start: ULong, end: ULong): MediaRangeReply {
    return try {
      val parsed = url.toHttpUrl().newBuilder().addQueryParameter("range", "$start-$end").build()
      val builder = Request.Builder().url(parsed).header("User-Agent",userAgent).header("Origin",origin).header("Referer","$origin/").header("Accept-Encoding","identity")
      if(method == "POST") builder.post(byteArrayOf(0x78,0).toRequestBody())
      val request = builder.build()
      client.newCall(request).execute().use { response ->
        val buffer = ByteArray((end-start+2u).toInt())
        var count = 0
        response.body?.byteStream()?.use { input ->
          while(count < buffer.size) { val n = input.read(buffer,count,buffer.size-count); if(n<0) break; count+=n }
        }
        MediaRangeReply(response.code,buffer.copyOf(count))
      }
    } catch (_: Exception) { MediaRangeReply(0,byteArrayOf()) }
  }
}

class LingoPlayCoreModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("LingoPlayCore")
    OnCreate {
      installMediaHttp(NativeMediaHttp())
      installAiPlatform(NativeAiPlatform(requireNotNull(appContext.reactContext)))
    }
    AsyncFunction("ai") { request: String ->
      val context = requireNotNull(appContext.reactContext)
      aiExecute(request, File(context.filesDir, "transcripts").path)
    }
    AsyncFunction("execute") { request: String ->
      val context = requireNotNull(appContext.reactContext)
      execute(request, File(context.filesDir, "transcripts").path)
    }
    AsyncFunction("mux") { video: String, audio: String, output: String ->
      val context = requireNotNull(appContext.reactContext)
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
            info.set(0, bytes, extractor.sampleTime.coerceAtLeast(0), extractor.sampleFlags)
            writer.writeSampleData(tracks[index], buffer, info)
            extractor.advance()
          }
        }
        writer.stop()
        target.toURI().toString()
      } catch (failure: Exception) {
        target.delete()
        throw failure
      } finally {
        extractors.forEach { runCatching { it.release() } }
        runCatching { muxer?.release() }
      }
    }
  }
}
