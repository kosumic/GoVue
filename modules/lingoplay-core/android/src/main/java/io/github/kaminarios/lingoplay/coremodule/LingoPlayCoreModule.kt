package io.github.kaminarios.lingoplay.coremodule

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.functions.Queues
import expo.modules.kotlin.Promise
import io.github.kaminarios.lingoplay.core.execute
import io.github.kaminarios.lingoplay.core.aiExecute
import io.github.kaminarios.lingoplay.core.dictationExecute
import io.github.kaminarios.lingoplay.core.installAiPlatform
import java.io.File
import android.media.MediaCodec
import android.media.MediaCodecList
import android.media.AudioManager
import android.content.Context
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.net.Uri
import android.os.Build
import android.os.Trace
import java.nio.ByteBuffer
import io.github.kaminarios.lingoplay.core.MediaHttp
import io.github.kaminarios.lingoplay.core.MediaRangeReply
import io.github.kaminarios.lingoplay.core.installMediaHttp
import io.github.kaminarios.lingoplay.core.installExtractionHttp
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import io.github.kaminarios.lingoplay.coremodule.cast.CastDiscovery
import io.github.kaminarios.lingoplay.coremodule.cast.NativeCastPlatform
import io.github.kaminarios.lingoplay.core.installCastPlatform

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
  private val main = android.os.Handler(android.os.Looper.getMainLooper())
  private var physicalOrientation: PhysicalOrientation? = null
  private var castDiscovery: CastDiscovery? = null
  private var pipControls: PipControls? = null
  @androidx.annotation.RequiresApi(26)
  private fun ensurePipControls(): PipControls? {
    if (Build.VERSION.SDK_INT < 26) return null
    if (pipControls == null) pipControls = PipControls(requireNotNull(appContext.reactContext),
      { appContext.currentActivity }, { sample -> sendEvent("pipAction", sample) })
    return pipControls
  }
  private val publicationPool = java.util.concurrent.ThreadPoolExecutor(2, 2, 30, TimeUnit.SECONDS,
    java.util.concurrent.ArrayBlockingQueue<Runnable>(8))
  private val browserLaunches = java.util.ArrayDeque<String>()
  private var initialBrowserLaunchTaken = false
  private fun captureInitialBrowserLaunch(): Boolean {
    synchronized(browserLaunches) {
      if (initialBrowserLaunchTaken) return false
      // Expo can start the module before its host activity is attached. A null
      // activity must not consume the one opportunity to read the launch intent.
      val activity = appContext.currentActivity ?: return false
      initialBrowserLaunchTaken = true
      val before = browserLaunches.size
      captureBrowserLaunch(activity.intent)
      return browserLaunches.size > before
    }
  }
  private fun captureBrowserLaunch(intent: android.content.Intent?) {
    val text = when(intent?.action) {
      android.content.Intent.ACTION_SEND -> intent.getStringExtra(android.content.Intent.EXTRA_TEXT)
      android.content.Intent.ACTION_VIEW -> intent.dataString
      else -> null
    } ?: return
    // Rust parses and validates the link. Never retain shared text on disk.
    if (text.length > 100000) return
    synchronized(browserLaunches) {
      if (browserLaunches.size >= 8) browserLaunches.removeFirst()
      browserLaunches.addLast(text)
    }
  }
  private var pendingSave: Pair<File, Promise>? = null
  private val galleryCalls=java.util.concurrent.atomic.AtomicInteger(0)
  private val imageClient=OkHttpClient.Builder().connectTimeout(15,TimeUnit.SECONDS).readTimeout(15,TimeUnit.SECONDS).followRedirects(false).followSslRedirects(false).build()
  override fun definition() = ModuleDefinition {
    Name("LingoPlayCore")
    AsyncFunction("getDownloadActivityCount") { DownloadActivityService.activeCount }
    Events("browserLaunch", "deviceOrientation", "pipAction")
    AsyncFunction("configurePipControls") { raw: String ->
      if (Build.VERSION.SDK_INT >= 26) ensurePipControls()?.configure(raw)
      Unit
    }.runOnQueue(Queues.MAIN)
    AsyncFunction("pipBackgroundAudio") { videoId: String ->
      if (Build.VERSION.SDK_INT >= 26) ensurePipControls()?.backgroundAudio(videoId) ?: false else false
    }.runOnQueue(Queues.MAIN)
    AsyncFunction("observeDeviceOrientation") { enabled: Boolean ->
      main.post {
        if (physicalOrientation == null && enabled) {
          physicalOrientation = PhysicalOrientation(requireNotNull(appContext.reactContext)) { sample -> sendEvent("deviceOrientation", sample) }
        }
        physicalOrientation?.observe(enabled)
      }
      Unit
    }
    OnActivityEntersForeground { main.post {
      physicalOrientation?.foreground(true)
      if (Build.VERSION.SDK_INT >= 26) pipControls?.foreground()
      if (captureInitialBrowserLaunch()) sendEvent("browserLaunch", emptyMap<String, Any>())
    } }
    OnActivityEntersBackground { main.post { physicalOrientation?.foreground(false); castDiscovery?.snapshot(false) } }
    OnDestroy {
      publicationPool.shutdown()
      main.post { physicalOrientation?.close(); physicalOrientation = null; castDiscovery?.close(); castDiscovery = null; if (Build.VERSION.SDK_INT >= 26) pipControls?.close(); pipControls = null }
    }
    AsyncFunction("publishDownload") { id: String, promise: Promise ->
      val context = requireNotNull(appContext.reactContext)
      try {
        publicationPool.execute {
          try { promise.resolve(DownloadPublisher(context).publish(id)) }
          catch (_: Exception) {
            promise.reject("ERR_PUBLISH_DOWNLOAD", "The file could not be saved in Android Downloads. Check storage space and retry.", null)
          }
        }
      } catch (_: java.util.concurrent.RejectedExecutionException) {
        promise.reject("ERR_PUBLISH_DOWNLOAD", "Other files are being saved. Try again in a moment.", null)
      }
    }
    AsyncFunction("deletePublishedDownload") { uri: String ->
      DownloadPublisher(requireNotNull(appContext.reactContext)).delete(uri)
    }
    AsyncFunction("castDiscoverySnapshot") { enabled: Boolean ->
      synchronized(this@LingoPlayCoreModule) {
        if(castDiscovery==null) castDiscovery=CastDiscovery(requireNotNull(appContext.reactContext))
        requireNotNull(castDiscovery).snapshot(enabled)
      }
    }
    AsyncFunction("castNeedsLocalPermission") {
      Build.VERSION.SDK_INT >= 37 && requireNotNull(appContext.reactContext).checkSelfPermission("android.permission.ACCESS_LOCAL_NETWORK") != android.content.pm.PackageManager.PERMISSION_GRANTED
    }
    OnNewIntent { intent ->
      synchronized(browserLaunches) {
        initialBrowserLaunchTaken = true
        captureBrowserLaunch(intent)
      }
      sendEvent("browserLaunch", emptyMap<String, Any>())
    }
    AsyncFunction("takeBrowserLaunches") {
      captureInitialBrowserLaunch()
      synchronized(browserLaunches) {
        browserLaunches.toList().also { browserLaunches.clear() }
      }
    }
    AsyncFunction("galleryBytes") { url: String, key: String, promise: Promise ->
      require(url.length<=8192)
      val address=url.toHttpUrl()
      require(address.isHttps && address.username.isEmpty() && address.password.isEmpty() && address.port==443 && listOf("ggpht.com","ytimg.com","googleusercontent.com").any { address.host==it||address.host.endsWith(".$it") }) { "Unsupported image address." }
      require(key.matches(Regex("[a-f0-9]{64}"))) { "Invalid image cache identity." }
      val context=requireNotNull(appContext.reactContext)
      check(galleryCalls.incrementAndGet()<=3) { galleryCalls.decrementAndGet(); "Too many image requests." }
      Thread {
        val directory=File(context.filesDir,"transcripts/media/gallery")
        val temporary=File(directory,"$key.part")
        try {
          imageClient.newCall(Request.Builder().url(address).build()).execute().use { response ->
            val result=org.json.JSONObject().put("status",response.code)
            if(response.code in 300..399) result.put("location",response.header("Location")?.take(8192) ?: "")
            else if(response.isSuccessful){
              directory.mkdirs();check(directory.isDirectory)
              val body=requireNotNull(response.body);check(body.contentLength()<=20L*1024*1024)
              body.byteStream().use { input -> temporary.outputStream().use { output ->
                val buffer=ByteArray(16384);var total=0L
                while(true){val count=input.read(buffer);if(count<0)break;total+=count;check(total<=20L*1024*1024);output.write(buffer,0,count)}
              } }
            }
            promise.resolve(result.toString())
          }
        } catch (_:Exception){temporary.delete();promise.resolve("{\"status\":0}")}
        finally{galleryCalls.decrementAndGet()}
      }.apply {isDaemon=true;start()}
    }
    AsyncFunction("getBrowserUserAgent") { android.webkit.WebSettings.getDefaultUserAgent(requireNotNull(appContext.reactContext)) }
    AsyncFunction("saveFile") { uri: String, name: String, mime: String, promise: Promise ->
      check(pendingSave == null) { "A file picker is already open." }
      val context = requireNotNull(appContext.reactContext)
      val parsed = Uri.parse(uri)
      require(parsed.scheme == "file") { "Only saved files can be exported." }
      val file = File(requireNotNull(parsed.path)).canonicalFile
      require(file.isFile && listOf(context.filesDir, context.cacheDir).any { file.path.startsWith(it.canonicalPath + File.separator) }) { "File must be in app storage." }
      require(name.isNotBlank() && name.length <= 255 && !name.contains(Regex("[\\\\/:*?\"<>|\\r\\n]"))) { "Invalid export filename." }
      require(mime.matches(Regex("[a-zA-Z0-9.+-]+/[a-zA-Z0-9.+-]+"))) { "Invalid export format." }
      pendingSave = file to promise
      try {
        appContext.throwingActivity.startActivityForResult(android.content.Intent(android.content.Intent.ACTION_CREATE_DOCUMENT).apply {
          addCategory(android.content.Intent.CATEGORY_OPENABLE); type = mime
          putExtra(android.content.Intent.EXTRA_TITLE, name)
        }, 7147)
      } catch (error: Exception) { pendingSave = null; throw error }
    }
    OnActivityResult { _, (requestCode, resultCode, intent) ->
      if (requestCode == 7147) {
        val pending = pendingSave
        pendingSave = null
        if (pending != null) {
          val destination = intent?.data
          if (resultCode != android.app.Activity.RESULT_OK || destination?.scheme != "content") pending.second.resolve(false)
          else {
            val context = requireNotNull(appContext.reactContext)
            Thread {
              try {
                context.contentResolver.openOutputStream(destination, "w").use { output ->
                  requireNotNull(output) { "Could not open the chosen file." }
                  pending.first.inputStream().use { input -> input.copyTo(output, 128 * 1024) }
                }
                pending.second.resolve(true)
              } catch (_: Exception) { pending.second.reject("FILE_EXPORT_FAILED", "Could not save this file. Check the selected location.", null) }
            }.apply { isDaemon = true; start() }
          }
        }
      }
    }
    AsyncFunction("setDownloadActivity") { count: Int, title: String, received: Double, total: Double ->
      val context = requireNotNull(appContext.reactContext)
      val intent = android.content.Intent(context, DownloadActivityService::class.java)
        .putExtra("count", count.coerceIn(0, 16)).putExtra("title", title.take(200))
        .putExtra("received", received.coerceIn(0.0, 200_000_000_000.0).toLong())
        .putExtra("total", total.coerceIn(0.0, 200_000_000_000.0).toLong())
      if (count > 0) {
        if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent) else context.startService(intent)
      } else context.stopService(intent)
      Unit
    }
    OnCreate {
      installMediaHttp(NativeMediaHttp())
      installExtractionHttp(NativeExtractionHttp())
      installCastPlatform(NativeCastPlatform(requireNotNull(appContext.reactContext).applicationContext))
      installAiPlatform(NativeAiPlatform(requireNotNull(appContext.reactContext)))
    }
    Function("dictation") { request: String -> dictationExecute(request) }
    AsyncFunction("ai") { request: String ->
      val context = requireNotNull(appContext.reactContext)
      aiExecute(request, File(context.filesDir, "transcripts").path)
    }
    AsyncFunction("execute") { request: String ->
      val context = requireNotNull(appContext.reactContext)
      Trace.beginSection("LingoPlay core: native.execute")
      try { execute(request, File(context.filesDir, "transcripts").path) }
      finally { Trace.endSection() }
    }
    Function("transcriptTrace") { requestId: String, stage: String, cookie: Int, begin: Boolean ->
      // Trace names contain only a generated request ID and an allowlisted
      // stage from the app, never page text, request bodies, or signed URLs.
      if (Build.VERSION.SDK_INT < 29 || !Trace.isEnabled() ||
          !requestId.matches(Regex("[0-9:]{1,80}")) || !stage.matches(Regex("[a-z_.]{1,64}"))) false
      else {
        val name = "LingoPlay transcript: $stage [$requestId]"
        if (begin) Trace.beginAsyncSection(name, cookie) else Trace.endAsyncSection(name, cookie)
        true
      }
    }
    AsyncFunction("supportedMediaFormats") { raw: String ->
      require(raw.length <= 200000) { "Too many media formats." }
      val formats = JSONArray(raw)
      require(formats.length() <= 500) { "Too many media formats." }
      val decoders = MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.filter { !it.isEncoder }
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
        val supported = decoders.any { decoder ->
          runCatching {
            val hardware = if (Build.VERSION.SDK_INT >= 29) decoder.isHardwareAccelerated else !decoder.name.startsWith("OMX.google.") && !decoder.name.startsWith("c2.android.")
            if (!decoder.supportedTypes.any { it.equals(mime, ignoreCase = true) }) false
            else if (mime.startsWith("audio/")) true
            else if (!hardware && (mime == "video/av01" || height > 1080)) false
            else {
              val video = decoder.getCapabilitiesForType(mime).videoCapabilities
              if (width <= 0 || height <= 0) mime != "video/av01" && height <= 1080
              else if (fps > 0) video.areSizeAndRateSupported(width, height, fps.toDouble())
              else video.isSizeSupported(width, height)
            }
          }.getOrDefault(false)
        }
        if (supported) result.put(format.optString("key"))
      }
      result.toString()
    }
    AsyncFunction("getSystemVolume") {
      val audio = requireNotNull(appContext.reactContext).getSystemService(Context.AUDIO_SERVICE) as AudioManager
      audio.getStreamVolume(AudioManager.STREAM_MUSIC).toDouble() / audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC).coerceAtLeast(1)
    }
    AsyncFunction("setSystemVolume") { value: Double ->
      require(value.isFinite()) { "Invalid volume." }
      val audio = requireNotNull(appContext.reactContext).getSystemService(Context.AUDIO_SERVICE) as AudioManager
      audio.setStreamVolume(AudioManager.STREAM_MUSIC, (value.coerceIn(0.0, 1.0) * audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)).toInt(), 0)
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
