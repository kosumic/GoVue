package io.github.kaminarios.govue.coremodule

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.functions.Queues
import expo.modules.kotlin.functions.Coroutine
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import expo.modules.kotlin.Promise
import io.github.kaminarios.govue.core.execute
import io.github.kaminarios.govue.core.browserUrlPolicy
import io.github.kaminarios.govue.core.aiExecute
import io.github.kaminarios.govue.core.dictationExecute
import io.github.kaminarios.govue.core.installAiPlatform
import java.io.File
import android.media.MediaCodecList
import android.media.AudioManager
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Trace
import io.github.kaminarios.govue.core.MediaHttp
import io.github.kaminarios.govue.core.MediaRangeReply
import io.github.kaminarios.govue.core.installMediaHttp
import io.github.kaminarios.govue.core.installDownloadHttp
import io.github.kaminarios.govue.core.MediaStreamHttp
import io.github.kaminarios.govue.core.MediaStreamReply
import io.github.kaminarios.govue.core.installMediaStreamHttp
import io.github.kaminarios.govue.core.installExtractionHttp
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import io.github.kaminarios.govue.coremodule.cast.CastDiscovery
import io.github.kaminarios.govue.coremodule.cast.NativeCastPlatform
import io.github.kaminarios.govue.core.installCastPlatform

private class NativeMediaHttp(context: Context) : MediaHttp, MediaStreamHttp {
  private val client = OkHttpClient.Builder().connectTimeout(8, TimeUnit.SECONDS)
    .readTimeout(12, TimeUnit.SECONDS).callTimeout(18, TimeUnit.SECONDS)
    .followRedirects(false).followSslRedirects(false).build()
  private class Stream(val response: okhttp3.Response, val input: java.io.InputStream, var remaining: Int)
  private val streams = java.util.concurrent.ConcurrentHashMap<ULong, Stream>()
  private val sequence = java.util.concurrent.atomic.AtomicLong(0)
  private val capacity = java.util.concurrent.Semaphore(24)
  private val http3 = if (Build.VERSION.SDK_INT >= 34) NativeMediaEngine(context, capacity) else null

  override fun fetchRange(url: String, userAgent: String, origin: String, method: String, start: ULong, end: ULong): MediaRangeReply {
    val head = openRange(url, userAgent, origin, method, start, end)
    if (head.handle == 0uL) return MediaRangeReply(head.status, byteArrayOf(), head.location)
    return try {
      val bytes = java.io.ByteArrayOutputStream()
      while (true) {
        val part = read(head.handle, 65536u)
        if (part.isEmpty()) break
        bytes.write(part)
      }
      MediaRangeReply(head.status, bytes.toByteArray(), head.location)
    } finally { close(head.handle) }
  }

  override fun openRange(url: String, userAgent: String, origin: String, method: String, start: ULong, end: ULong): MediaStreamReply {
    if (Build.VERSION.SDK_INT >= 34 && method == "GET")
      http3?.openRange(url, userAgent, origin, start, end)?.let { return it }
    if (!capacity.tryAcquire()) return MediaStreamReply(0, 0uL, -1, null)
    var response: okhttp3.Response? = null
    var retained = false
    try {
      require(end >= start && end - start < 1048576uL)
      require(method == "GET" || method == "POST")
      val parsed = url.toHttpUrl().newBuilder().addQueryParameter("range", "$start-$end").build()
      val builder = Request.Builder().url(parsed).header("User-Agent",userAgent).header("Origin",origin).header("Referer","$origin/").header("Accept-Encoding","identity")
      if(method == "POST") builder.post(byteArrayOf(0x78,0).toRequestBody())
      val request = builder.build()
      val opened = mediaTrace(if (method == "POST") "GoVue transport: POST headers" else "GoVue transport: GET headers") {
        client.newCall(request).execute()
      }
      response = opened
      mediaTrace(if (opened.protocol == okhttp3.Protocol.HTTP_2) "GoVue transport: protocol h2" else "GoVue transport: protocol http1") {}
      mediaTrace("GoVue transport: HTTP ${opened.code}") {}
      val location = opened.header("Location")?.takeIf { it.length <= 30000 }
      val length = opened.body?.contentLength() ?: -1L
      if (opened.code != 200 && opened.code != 206)
        return MediaStreamReply(opened.code, 0uL, length, location)
      val input = opened.body?.byteStream() ?: return MediaStreamReply(0, 0uL, -1, null)
      val handle = sequence.incrementAndGet().toULong()
      streams[handle] = Stream(opened, input, (end - start + 2uL).toInt())
      retained = true
      return MediaStreamReply(opened.code, handle, length, location)
    } catch (_: Exception) { return MediaStreamReply(0, 0uL, -1, null) }
    finally {
      if (!retained) {
        response?.close()
        capacity.release()
      }
    }
  }

  override fun read(handle: ULong, maxBytes: UInt): ByteArray {
    if (Build.VERSION.SDK_INT >= 34 && NativeMediaEngine.owns(handle)) return http3?.read(handle, maxBytes) ?: byteArrayOf()
    val stream = streams[handle] ?: return byteArrayOf()
    return try {
      require(maxBytes in 1u..65536u)
      synchronized(stream) {
        if (stream.remaining <= 0) { close(handle); return byteArrayOf() }
        val buffer = ByteArray(minOf(maxBytes.toInt(), stream.remaining))
        val count = mediaTrace("GoVue transport: stream read") { stream.input.read(buffer) }
        if (count < 0) { close(handle); return byteArrayOf() }
        stream.remaining -= count
        buffer.copyOf(count)
      }
    } catch (_: Exception) { close(handle); byteArrayOf() }
  }

  override fun close(handle: ULong) {
    if (Build.VERSION.SDK_INT >= 34 && NativeMediaEngine.owns(handle)) { http3?.close(handle); return }
    streams.remove(handle)?.let { stream ->
      try { stream.response.close() } finally { capacity.release() }
    }
  }
}

// Phase labels and HTTP status numbers only; addresses and headers stay private.
internal inline fun <T> mediaTrace(name: String, block: () -> T): T {
  Trace.beginSection(name)
  try { return block() } finally { Trace.endSection() }
}

internal class NativeMediaCapabilities {
  private data class Decoder(val hardware: Boolean, val video: android.media.MediaCodecInfo.VideoCapabilities?)
  private val codecs by lazy { MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.filter { !it.isEncoder } }
  private val capabilities = java.util.concurrent.ConcurrentHashMap<String, List<Decoder>>()

  fun supports(mime: String, width: Int, height: Int, fps: Int): Boolean {
    val candidates = capabilities.computeIfAbsent(mime) {
      codecs.filter { codec -> codec.supportedTypes.any { it.equals(mime, ignoreCase = true) } }.mapNotNull { codec ->
        runCatching {
          val hardware = if (Build.VERSION.SDK_INT >= 29) codec.isHardwareAccelerated
            else !codec.name.startsWith("OMX.google.") && !codec.name.startsWith("c2.android.")
          Decoder(hardware, if (mime.startsWith("audio/")) null else codec.getCapabilitiesForType(mime).videoCapabilities)
        }.getOrNull()
      }
    }
    return candidates.any { decoder -> runCatching {
      if (mime.startsWith("audio/")) true
      else if (!decoder.hardware && (mime == "video/av01" || height > 1080)) false
      else if (width <= 0 || height <= 0) mime != "video/av01" && height <= 1080
      else decoder.video?.let { video ->
        if (fps > 0) video.areSizeAndRateSupported(width, height, fps.toDouble()) else video.isSizeSupported(width, height)
      } ?: false
    }.getOrDefault(false) }
  }
}

class GoVueCoreModule : Module() {
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
    // Rust validates the OS-delivered text and settings, then starts a bounded
    // worker. React claims the same job after accepting the navigation target.
    appContext.reactContext?.let { context ->
      mediaTrace("GoVue extraction: launch preparation") {
        execute(org.json.JSONObject().put("op", "extract_launch")
          .put("requestId", "prepare:media").put("text", text).toString(),
          File(context.filesDir, "transcripts").path)
      }
    }
  }
  private var pendingSave: Pair<File, Promise>? = null
  private val galleryCalls=java.util.concurrent.atomic.AtomicInteger(0)
  private val imageClient=OkHttpClient.Builder().connectTimeout(15,TimeUnit.SECONDS).readTimeout(15,TimeUnit.SECONDS).followRedirects(false).followSslRedirects(false).build()
  override fun definition() = ModuleDefinition {
    Name("GoVueCore")
    Events("browserLaunch", "deviceOrientation", "pipAction", "coreUpdate")
    View(CaptionSelectionView::class) {
      Events("onSelectionChange", "onContentSizeChange")
      Prop("text") { view, value: String -> view.captionText = value }
      Prop("selectionStart") { view, value: Int -> view.initialStart = value }
      Prop("selectionEnd") { view, value: Int -> view.initialEnd = value }
      Prop("fontSize") { view, value: Float -> view.fontSize = value }
      Prop("lineHeight") { view, value: Float -> view.lineHeight = value }
      Prop("textColor") { view, value: String -> view.textColor = value }
      Prop("selectionColor") { view, value: String -> view.selectionColor = value }
      OnViewDidUpdateProps { view -> view.update() }
    }
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
      main.post { physicalOrientation?.close(); physicalOrientation = null; castDiscovery?.close(); castDiscovery = null; if (Build.VERSION.SDK_INT >= 26) pipControls?.close(); pipControls = null }
    }
    AsyncFunction("castDiscoverySnapshot") { enabled: Boolean ->
      synchronized(this@GoVueCoreModule) {
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
    OnCreate {
      io.github.kaminarios.govue.core.installDownloadPlatform(NativeDownloadPlatform(requireNotNull(appContext.reactContext).applicationContext))
      io.github.kaminarios.govue.core.installCoreEvents(object : io.github.kaminarios.govue.core.CoreEvents {
        override fun changed(channel: String) { main.post { sendEvent("coreUpdate", mapOf("channel" to channel)) } }
      })
      val mediaHttp = NativeMediaHttp(requireNotNull(appContext.reactContext).applicationContext)
      installMediaHttp(mediaHttp)
      installDownloadHttp(NativeDownloadHttp.shared(requireNotNull(appContext.reactContext).applicationContext))
      installMediaStreamHttp(mediaHttp)
      installExtractionHttp(NativeExtractionHttp(requireNotNull(appContext.reactContext).applicationContext))
      installCastPlatform(NativeCastPlatform(requireNotNull(appContext.reactContext).applicationContext))
      installAiPlatform(NativeAiPlatform(requireNotNull(appContext.reactContext)))
      captureInitialBrowserLaunch()
    }
    Function("browserUrlPolicy") { url: String -> browserUrlPolicy(url) }
    Function("state") { request: String -> io.github.kaminarios.govue.core.stateExecute(request) }
    Function("dictation") { request: String -> dictationExecute(request) }
    AsyncFunction("ai") Coroutine { request: String ->
      withContext(Dispatchers.IO) {
        val context = requireNotNull(appContext.reactContext)
        aiExecute(request, File(context.filesDir, "transcripts").path)
      }
    }
    AsyncFunction("execute") Coroutine { request: String ->
      withContext(Dispatchers.IO) {
        val context = requireNotNull(appContext.reactContext)
        Trace.beginSection("GoVue core: native.execute")
        try { execute(request, File(context.filesDir, "transcripts").path) }
        finally { Trace.endSection() }
      }
    }
    Function("transcriptTrace") { requestId: String, stage: String, cookie: Int, begin: Boolean ->
      // Trace names contain only a generated request ID and an allowlisted
      // stage from the app, never page text, request bodies, or signed URLs.
      if (Build.VERSION.SDK_INT < 29 || !Trace.isEnabled() ||
          !requestId.matches(Regex("[0-9:]{1,80}")) || !stage.matches(Regex("[a-z_.]{1,64}"))) false
      else {
        val name = "GoVue transcript: $stage [$requestId]"
        if (begin) Trace.beginAsyncSection(name, cookie) else Trace.endAsyncSection(name, cookie)
        true
      }
    }
    AsyncFunction("supportedMediaFormats") { raw: String ->
      NativeDownloadPlatform(requireNotNull(appContext.reactContext).applicationContext).formats(raw)
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

  }
}
