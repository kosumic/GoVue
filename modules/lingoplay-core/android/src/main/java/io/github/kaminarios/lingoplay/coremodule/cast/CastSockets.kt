package io.github.kaminarios.lingoplay.coremodule.cast

import java.net.InetAddress
import java.net.InetSocketAddress
import java.security.SecureRandom
import java.security.cert.X509Certificate
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/** Platform TLS adapter. No app traffic can cross a provisional socket until
 * Cast DeviceAuth binds its certificate to a trusted hardware certificate. */
internal class CastSockets {
  private val sockets = ConcurrentHashMap<String, SSLSocket>()
  private val opening = AtomicInteger(0)
  private val deadlines = ScheduledThreadPoolExecutor(1) { task -> Thread(task, "LingoPlayCastDeadline").apply { isDaemon = true } }.apply { removeOnCancelPolicy = true }
  fun open(host: String, port: Int, network: android.net.Network): String {
    if (host.length > 160 || port !in 1..65535) return ""
    if (sockets.size + opening.incrementAndGet() > 2) {
      opening.decrementAndGet(); return ""
    }
    var socket: SSLSocket? = null
    var deadline: ScheduledFuture<*>? = null
    try {
      require(host.matches(Regex("[0-9a-fA-F:.%a-zA-Z_-]+")))
      val address = InetAddress.getByName(host)
      val bytes = address.address
      require(!address.isLoopbackAddress && !address.isAnyLocalAddress &&
        (address.isSiteLocalAddress || address.isLinkLocalAddress || bytes.size == 16 && bytes[0].toInt() and 0xfe == 0xfc))
      val context = SSLContext.getInstance("TLS")
      context.init(null, arrayOf<TrustManager>(object : X509TrustManager {
        override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) = Unit
        override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) = Unit
        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
      }), SecureRandom())
      socket = context.socketFactory.createSocket() as SSLSocket
      network.bindSocket(socket)
      val provisional = socket
      deadline = deadlines.schedule({ runCatching { provisional.close() } }, 20, TimeUnit.SECONDS)
      socket.soTimeout = 7000
      socket.connect(InetSocketAddress(address, port), 7000)
      socket.startHandshake()
      CastDeviceAuth.authenticate(socket, socket.inputStream, socket.outputStream)
      socket.soTimeout = 20000
      val id = UUID.randomUUID().toString()
      sockets[id] = socket
      return id
    } catch (_: Exception) {
      runCatching { socket?.close() }
      return ""
    } finally { deadline?.cancel(false); opening.decrementAndGet() }
  }
  fun read(id: String): ByteArray { return try {
    val socket = sockets[id] ?: return byteArrayOf()
    val input = socket.inputStream
    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20)
    fun exact(size: Int): ByteArray {
      val bytes = ByteArray(size)
      var at = 0
      while (at < size) {
        val remaining = deadline - System.nanoTime()
        require(remaining > 0)
        socket.soTimeout = TimeUnit.NANOSECONDS.toMillis(remaining).coerceIn(1, 20000).toInt()
        val count = input.read(bytes, at, size-at)
        require(count > 0)
        at += count
      }
      return bytes
    }
    val size = java.nio.ByteBuffer.wrap(exact(4)).int
    require(size in 1..65536)
    exact(size)
  } catch (_: Exception) { close(id); byteArrayOf() } }
  fun write(id: String, frame: ByteArray): Boolean { return try {
    require(frame.size in 1..65536)
    val socket = sockets[id] ?: return false
    synchronized(socket) {
      val output = java.io.DataOutputStream(socket.outputStream)
      output.writeInt(frame.size); output.write(frame); output.flush()
    }
    true
  } catch (_: Exception) { close(id); false } }
  fun close(id: String) { runCatching { sockets.remove(id)?.close() } }
  fun closeAll() { sockets.keys.toList().forEach(::close) }
}
