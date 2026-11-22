package io.github.kaminarios.govue.coremodule

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import okhttp3.ConnectionPool
import okhttp3.Call
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.Semaphore
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/** OS network facts and bounded HTTPS only; Rust owns policy waits and retries. */
internal class AndroidDownloadHttp private constructor(context: Context) {
  companion object {
    @Volatile private var instance: AndroidDownloadHttp? = null
    fun shared(context: Context): AndroidDownloadHttp = instance ?: synchronized(this) {
      instance ?: AndroidDownloadHttp(context.applicationContext).also { instance = it }
    }
  }
  private val connectivity = context.getSystemService(ConnectivityManager::class.java)
  private val client = OkHttpClient.Builder().connectTimeout(8, TimeUnit.SECONDS)
    .readTimeout(12, TimeUnit.SECONDS).callTimeout(18, TimeUnit.SECONDS)
    .followRedirects(false).followSslRedirects(false).retryOnConnectionFailure(false).build()
  private val capacity = Semaphore(24)
  private var bound: Pair<Network, OkHttpClient>? = null
  private val restricted = ConcurrentHashMap<Call, Network>()

  init {
    // Native callbacks cancel in-flight requests without waiting for JS. This
    // also covers VPNs whose underlying transport changes to cellular.
    connectivity.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
      override fun onLost(network: Network) {
        restricted.forEach { (call, selected) -> if (selected == network) call.cancel() }
      }
      override fun onAvailable(network: Network) {
        restricted.forEach { (call, selected) -> if (selected != network) call.cancel() }
      }
      override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
        if (!permitsWifi(caps))
          restricted.forEach { (call, selected) -> if (selected == network) call.cancel() }
      }
    })
  }

  private fun permitsWifi(caps: NetworkCapabilities): Boolean =
    caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
      !caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) &&
      caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)

  private fun wifi(): Network? = runCatching {
    val network = connectivity.activeNetwork ?: return@runCatching null
    val caps = connectivity.getNetworkCapabilities(network) ?: return@runCatching null
    network.takeIf { permitsWifi(caps) }
  }.getOrNull()

  fun wifiConnected(): Boolean = wifi() != null

  private fun clientFor(network: Network): OkHttpClient = synchronized(this) {
    bound?.takeIf { it.first == network }?.second ?: client.newBuilder()
      // A separate pool prevents reuse of sockets opened on cellular or an old
      // Wi-Fi network. Native callbacks also cancel VPN underlay changes.
      .connectionPool(ConnectionPool()).socketFactory(network.socketFactory)
      .dns(object : okhttp3.Dns {
        override fun lookup(hostname: String) = network.getAllByName(hostname).toList()
      }).build().also {
        bound?.second?.connectionPool?.evictAll()
        bound = network to it
      }
  }

  fun fetch(
    address: String, userAgent: String, origin: String, method: String,
    first: Long, last: Long, wifiOnly: Boolean,
  ): AndroidRangeReply {
    val network = if (wifiOnly) wifi() ?: return AndroidRangeReply(-1, byteArrayOf(), null) else null
    if (!capacity.tryAcquire()) return AndroidRangeReply(0, byteArrayOf(), null)
    var call: Call? = null
    return try {
      var url = address.toHttpUrl()
      require(url.isHttps && (method == "GET" || method == "POST"))
      if (first >= 0) {
        require(last >= first && last - first < 1048576)
        url = url.newBuilder().addQueryParameter("range", "$first-$last").build()
      }
      val builder = Request.Builder().url(url).header("User-Agent", userAgent)
        .header("Origin", origin).header("Referer", "${origin}/")
        .header("Accept-Encoding", "identity")
      if (method == "POST") builder.post(byteArrayOf(0x78, 0).toRequestBody())
      val selected = network?.let(::clientFor) ?: client
      val pending = selected.newCall(builder.build())
      call = pending
      if (network != null) {
        restricted[pending] = network
        if (wifi() != network) return AndroidRangeReply(-1, byteArrayOf(), null)
      }
      pending.execute().use { response ->
        val location = response.header("Location")?.takeIf { it.length <= 30000 }
        if (response.code != 200 && response.code != 206)
          return@use AndroidRangeReply(response.code, byteArrayOf(), location)
        val body = response.body ?: return@use AndroidRangeReply(0, byteArrayOf(), null)
        val bytes = body.byteStream().use { input ->
          val output = java.io.ByteArrayOutputStream()
          val buffer = ByteArray(65536)
          while (output.size() <= 1048576) {
            val count = input.read(buffer, 0, minOf(buffer.size, 1048577 - output.size()))
            if (count < 0) break
            output.write(buffer, 0, count)
          }
          output.toByteArray()
        }
        if (wifiOnly && wifi() != network &&
          (first < 0 || bytes.size.toLong() != last - first + 1))
          AndroidRangeReply(-1, byteArrayOf(), null)
        else if (bytes.size > 1048576) AndroidRangeReply(0, byteArrayOf(), null)
        else AndroidRangeReply(response.code, bytes, location)
      }
    } catch (_: Exception) {
      AndroidRangeReply(if (wifiOnly && (call?.isCanceled() == true || wifi() != network)) -1 else 0, byteArrayOf(), null)
    } finally { call?.let { restricted.remove(it) }; capacity.release() }
  }
}

internal data class AndroidRangeReply(val status: Int, val body: ByteArray, val location: String?)
