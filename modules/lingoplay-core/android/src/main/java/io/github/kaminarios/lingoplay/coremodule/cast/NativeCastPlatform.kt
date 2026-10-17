package io.github.kaminarios.lingoplay.coremodule.cast

import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Network
import android.os.Build
import io.github.kaminarios.lingoplay.core.CastPlatform
import java.net.Inet4Address

/** Platform I/O only. No Cast commands, media addresses, or session state in Kotlin. */
internal class NativeCastPlatform(private val context: Context) : CastPlatform {
  private val sockets = CastSockets()
  private fun wifi(): Pair<Network, String>? = try {
    val manager = context.getSystemService(ConnectivityManager::class.java)
    @Suppress("DEPRECATION")
    val networks = listOfNotNull(manager.activeNetwork) + manager.allNetworks.toList()
    networks.distinct().firstNotNullOfOrNull { network ->
      val caps = manager.getNetworkCapabilities(network)
      if (caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) != true || caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) null
      else manager.getLinkProperties(network)?.linkAddresses?.firstOrNull {
        val address = it.address
        address is Inet4Address && !address.isLoopbackAddress && (address.isSiteLocalAddress || address.isLinkLocalAddress)
      }?.address?.hostAddress?.let { network to it }
    }
  } catch (_: Exception) { null }
  override fun wifiAddress(): String = wifi()?.second ?: ""
  override fun bindWifiSocket(socketFd: UInt, address: String): Boolean { return try {
    require(socketFd <= Int.MAX_VALUE.toUInt())
    val selected = wifi()?.takeIf { it.second == address } ?: return false
    // Duplicate the descriptor; Rust retains ownership of its listener. Socket
    // network marks are shared by the duplicate and inherited by accepted peers.
    android.os.ParcelFileDescriptor.fromFd(socketFd.toInt()).use { selected.first.bindSocket(it.fileDescriptor) }
    true
  } catch (error: Exception) {
    // Report only a fixed stage, exception type and numeric errno. Never log
    // exception messages, socket endpoints, media addresses or capabilities.
    val errno = generateSequence<Throwable>(error) { it.cause }
      .filterIsInstance<android.system.ErrnoException>().firstOrNull()?.errno ?: 0
    android.util.Log.w("LingoPlayCast", "Wi-Fi listener binding failed: ${error.javaClass.simpleName}; errno=$errno")
    false
  } }
  override fun open(host: String, port: UShort): String {
    val selected = wifi() ?: return ""
    return sockets.open(host, port.toInt(), selected.first)
  }
  override fun read(socket: String): ByteArray = sockets.read(socket)
  override fun write(socket: String, frame: ByteArray): Boolean = sockets.write(socket, frame)
  override fun close(socket: String) = sockets.close(socket)
  override fun activity(active: Boolean): Boolean = try {
    val intent = Intent(context, CastActivityService::class.java)
    if (active) {
      // A visible notification is part of the authorized LAN listener's lifetime.
      val manager = context.getSystemService(NotificationManager::class.java)
      if (Build.VERSION.SDK_INT >= 24 && !manager.areNotificationsEnabled() ||
          Build.VERSION.SDK_INT >= 26 && manager.getNotificationChannel("casting")?.importance == NotificationManager.IMPORTANCE_NONE) false
      else {
        if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent) else context.startService(intent)
        true
      }
    } else { context.stopService(intent); true }
  } catch (_: Exception) { false }
}
