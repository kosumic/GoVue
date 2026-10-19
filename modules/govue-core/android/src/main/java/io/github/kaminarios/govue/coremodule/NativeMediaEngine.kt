package io.github.kaminarios.govue.coremodule

import android.content.Context
import android.net.http.HttpEngine
import android.net.http.HttpException
import android.net.http.UrlRequest
import android.net.http.UrlResponseInfo
import android.net.http.UploadDataProvider
import android.net.http.UploadDataSink
import android.os.SystemClock
import androidx.annotation.RequiresApi
import io.github.kaminarios.govue.core.MediaStreamReply
import io.github.kaminarios.govue.core.ExtractionRequest
import io.github.kaminarios.govue.core.ExtractionReply
import io.github.kaminarios.govue.core.ExtractionHeader
import okhttp3.HttpUrl.Companion.toHttpUrl
import java.io.IOException
import java.nio.ByteBuffer
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/** Platform HTTPS bytes only. Rust still owns redirects, retries and range validation. */
@RequiresApi(34)
internal class NativeMediaEngine(private val context: Context, private val capacity: Semaphore) {
  companion object {
    private const val HANDLE_BIT = 0x8000000000000000uL
    fun owns(handle: ULong) = handle and HANDLE_BIT != 0uL
  }
  private class Engine(val engine: HttpEngine) {
    val active = AtomicInteger(0)
    var usedAt = SystemClock.elapsedRealtime()
    @Volatile var retryAt = 0L
  }
  private val engines = LinkedHashMap<String, Engine>()
  private val executor = Executors.newFixedThreadPool(4)
  private val streams = ConcurrentHashMap<ULong, Body>()
  private val sequence = AtomicLong(0)

  private class Upload(private val bytes: ByteArray) : UploadDataProvider() {
    private var position = 0
    override fun getLength(): Long = bytes.size.toLong()
    override fun read(sink: UploadDataSink, buffer: ByteBuffer) {
      val count = minOf(buffer.remaining(), bytes.size - position)
      buffer.put(bytes, position, count); position += count
      sink.onReadSucceeded(false)
    }
    override fun rewind(sink: UploadDataSink) { position = 0; sink.onRewindSucceeded() }
  }

  private fun protocol(info: UrlResponseInfo): String {
    val value = info.negotiatedProtocol
    return if (value.startsWith("h3") || value.startsWith("quic")) "h3" else if (value == "h2") "h2" else "http1"
  }

  // The same bounded HTTPS adapter serves configuration and player requests.
  // Rust supplies every header, body and size limit, including guest cookies.
  fun fetch(request: ExtractionRequest): ExtractionReply? {
    if (!capacity.tryAcquire()) return ExtractionReply(0, byteArrayOf(), emptyList())
    var body: Body? = null
    try {
      val limit = request.maxBytes.toInt()
      require(limit in 1..8_000_000)
      require(request.method == "GET" || request.method == "POST")
      val address = request.url.toHttpUrl()
      val engine = reserve(address.host) ?: return null
      val cap = if (request.prefixOnly) limit else limit + 1
      val opened = Body(engine, cap)
      body = opened
      val builder = engine.engine.newUrlRequestBuilder(address.toString(), executor, opened)
        .setHttpMethod(request.method).setCacheDisabled(true)
      for (header in request.headers) builder.addHeader(header.name, header.value)
      if (request.method == "POST") builder.setUploadDataProvider(Upload(request.body.toByteArray(Charsets.UTF_8)), executor)
      opened.request = builder.build()
      val info = mediaTrace(if (request.method == "POST") "GoVue extraction: POST headers" else "GoVue extraction: GET headers") {
        opened.request!!.start()
        opened.started = true
        opened.headers()
      }
      mediaTrace("GoVue extraction: protocol ${protocol(info)}") {}
      mediaTrace("GoVue extraction: HTTP ${info.httpStatusCode}") {}
      val cookies = info.headers.asList.filter { it.key.equals("Set-Cookie", ignoreCase = true) }
        .map { ExtractionHeader("Set-Cookie", it.value) }
      // A redirect callback is paused; never follow it or call read on it.
      if (info.httpStatusCode in listOf(301, 302, 303, 307, 308))
        return ExtractionReply(info.httpStatusCode, byteArrayOf(), cookies)
      val bytes = java.io.ByteArrayOutputStream()
      while (bytes.size() < cap) {
        val part = opened.read(minOf(16384, cap - bytes.size()))
        if (part.isEmpty()) break
        bytes.write(part)
      }
      return ExtractionReply(info.httpStatusCode, bytes.toByteArray(), cookies)
    } catch (_: Exception) {
      body?.engine?.retryAt = SystemClock.elapsedRealtime() + 60000
      mediaTrace("GoVue extraction: HTTP engine fallback") {}
      return null
    } finally { body?.cancel(); capacity.release() }
  }

  private fun reserve(host: String): Engine? = synchronized(engines) {
    val now = SystemClock.elapsedRealtime()
    engines[host]?.let {
      if (now < it.retryAt) return@synchronized null
      it.active.incrementAndGet(); it.usedAt = now
      return@synchronized it
    }
    if (engines.size >= 8) {
      val idle = engines.entries.filter { it.value.active.get() == 0 }.minByOrNull { it.value.usedAt }
        ?: return@synchronized null
      try { idle.value.engine.shutdown() } catch (_: Exception) { return@synchronized null }
      engines.remove(idle.key)
    }
    val engine = runCatching {
      mediaTrace("GoVue transport: HTTP engine") {
        HttpEngine.Builder(context).setEnableHttpCache(HttpEngine.Builder.HTTP_CACHE_DISABLED, 0)
          .setEnableQuic(true).addQuicHint(host, 443, 443).build()
      }
    }.getOrNull() ?: return@synchronized null
    Engine(engine).also { it.active.incrementAndGet(); engines[host] = it }
  }

  private class Body(val engine: Engine, var remaining: Int) : UrlRequest.Callback {
    val lock = Object()
    val buffer: ByteBuffer = ByteBuffer.allocateDirect(65_536)
    var request: UrlRequest? = null
    var started = false
    var info: UrlResponseInfo? = null
    var location: String? = null
    var finished = false
    var failed = false
    var closed = false
    var reading = false
    var delivered = false
    val openedAt = SystemClock.elapsedRealtime()
    private val released = AtomicBoolean(false)

    fun releaseEngine() { if (released.compareAndSet(false, true)) engine.active.decrementAndGet() }
    private fun finish(error: Boolean) {
      synchronized(lock) { finished = true; failed = error; reading = false; lock.notifyAll() }
      releaseEngine()
    }
    override fun onRedirectReceived(request: UrlRequest, info: UrlResponseInfo, newLocationUrl: String) {
      synchronized(lock) {
        this.info = info
        location = header(info, "Location") ?: newLocationUrl
        lock.notifyAll()
      }
    }
    override fun onResponseStarted(request: UrlRequest, info: UrlResponseInfo) {
      synchronized(lock) { this.info = info; lock.notifyAll() }
    }
    override fun onReadCompleted(request: UrlRequest, info: UrlResponseInfo, byteBuffer: ByteBuffer) {
      synchronized(lock) {
        if (!closed) { byteBuffer.flip(); delivered = true }
        reading = false; lock.notifyAll()
      }
    }
    override fun onSucceeded(request: UrlRequest, info: UrlResponseInfo) = finish(false)
    override fun onFailed(request: UrlRequest, info: UrlResponseInfo?, error: HttpException) = finish(true)
    override fun onCanceled(request: UrlRequest, info: UrlResponseInfo?) = finish(false)

    fun headers(): UrlResponseInfo = synchronized(lock) {
      while (info == null && !finished && !closed) {
        val left = openedAt + 4000 - SystemClock.elapsedRealtime()
        if (left <= 0) throw IOException("Media headers timed out.")
        lock.wait(left)
      }
      info ?: throw IOException("Media headers unavailable.")
    }
    fun read(max: Int): ByteArray = synchronized(lock) {
      if (closed || (finished && !delivered)) {
        if (failed) throw IOException("Media response failed.")
        return@synchronized byteArrayOf()
      }
      if (!delivered) {
        buffer.clear(); buffer.limit(minOf(max, remaining)); reading = true
        request!!.read(buffer)
        val deadline = minOf(openedAt + 18000, SystemClock.elapsedRealtime() + 12000)
        while (reading && !closed) {
          val left = deadline - SystemClock.elapsedRealtime()
          if (left <= 0) throw IOException("Media body timed out.")
          lock.wait(left)
        }
      }
      if (failed) throw IOException("Media response failed.")
      if (!delivered || closed) return@synchronized byteArrayOf()
      val bytes = ByteArray(minOf(max, buffer.remaining()))
      buffer.get(bytes); remaining -= bytes.size
      if (!buffer.hasRemaining()) delivered = false
      bytes
    }
    fun cancel() {
      synchronized(lock) { closed = true; reading = false; lock.notifyAll() }
      request?.let { if (!it.isDone) it.cancel() }
      if (!started) releaseEngine()
    }
    companion object {
      fun header(info: UrlResponseInfo, name: String): String? =
        info.headers.asList.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value
    }
  }

  fun openRange(url: String, userAgent: String, origin: String, start: ULong, end: ULong): MediaStreamReply? {
    if (!capacity.tryAcquire()) return MediaStreamReply(0, 0uL, -1, null)
    var body: Body? = null
    var retained = false
    try {
      require(end >= start && end - start < 1048576uL)
      val address = url.toHttpUrl().newBuilder().addQueryParameter("range", "$start-$end").build()
      val engine = reserve(address.host) ?: return null
      val opened = Body(engine, (end - start + 2uL).toInt())
      body = opened
      opened.request = engine.engine.newUrlRequestBuilder(address.toString(), executor, opened)
        .setHttpMethod("GET").setCacheDisabled(true)
        .addHeader("User-Agent", userAgent).addHeader("Origin", origin)
        .addHeader("Referer", "$origin/").addHeader("Accept-Encoding", "identity").build()
      val info = mediaTrace("GoVue transport: GET headers") {
        opened.request!!.start()
        opened.started = true
        opened.headers()
      }
      val encoding = Body.header(info, "Content-Encoding")
      if (encoding != null && !encoding.equals("identity", ignoreCase = true)) return null
      mediaTrace("GoVue transport: protocol ${protocol(info)}") {}
      mediaTrace("GoVue transport: HTTP ${info.httpStatusCode}") {}
      val length = Body.header(info, "Content-Length")?.toLongOrNull() ?: -1L
      val location = opened.location?.takeIf { it.length <= 30000 }
      if (info.httpStatusCode != 200 && info.httpStatusCode != 206)
        return MediaStreamReply(info.httpStatusCode, 0uL, length, location)
      val handle = sequence.incrementAndGet().toULong() or HANDLE_BIT
      streams[handle] = opened; retained = true
      return MediaStreamReply(info.httpStatusCode, handle, length, location)
    } catch (_: Exception) {
      body?.engine?.retryAt = SystemClock.elapsedRealtime() + 60000
      mediaTrace("GoVue transport: HTTP engine fallback") {}
      return null
    } finally {
      if (!retained) { body?.cancel(); capacity.release() }
    }
  }

  fun read(handle: ULong, maxBytes: UInt): ByteArray {
    val body = streams[handle] ?: return byteArrayOf()
    return try {
      require(maxBytes in 1u..65536u)
      if (body.remaining <= 0) { close(handle); byteArrayOf() }
      else mediaTrace("GoVue transport: stream read") { body.read(minOf(maxBytes.toInt(), body.remaining)) }
    } catch (_: Exception) {
      body.engine.retryAt = SystemClock.elapsedRealtime() + 60000
      close(handle); byteArrayOf()
    }
  }
  fun close(handle: ULong) {
    streams.remove(handle)?.let { try { it.cancel() } finally { capacity.release() } }
  }
}
