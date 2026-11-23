// Adapted from LiTube's CastV2Discovery, GPL-3.0, and NewTube, MIT. See notices/.
package io.github.kaminarios.govue.coremodule.cast

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.util.ArrayDeque
import java.util.concurrent.ConcurrentHashMap

/** Platform Bonjour/NSD adapter; receiver selection and session state live in Rust. */
internal class CastDiscovery(context: Context) {
  private val manager = context.getSystemService(Context.NSD_SERVICE) as? NsdManager
  private val main = Handler(Looper.getMainLooper())
  private val devices = ConcurrentHashMap<String, JSONObject>()
  private val queue = ArrayDeque<NsdServiceInfo>()
  private var resolving = false
  private var listener: NsdManager.DiscoveryListener? = null
  @Volatile private var generation = 0
  @Volatile private var enabled = false
  @Volatile private var requested = false
  @Volatile private var error: String? = null
  fun snapshot(discover: Boolean): String {
    if (discover != requested) {
      requested=discover
      main.post { setEnabled(requested) }
    }
    return JSONObject().put("available", manager != null).put("enabled", enabled).put("error", error ?: JSONObject.NULL)
      .put("devices", JSONArray(devices.values.toList())).toString()
  }
  private fun setEnabled(value: Boolean) {
    if (value == enabled) return
    generation++; val epoch = generation
    listener?.let { runCatching { manager?.stopServiceDiscovery(it) } }
    listener = null; enabled = value; error = null
    synchronized(queue) { queue.clear(); resolving = false }
    if (!value) return
    devices.clear()
    if (manager == null) { error = "Device discovery is unavailable."; return }
    val callback = object : NsdManager.DiscoveryListener {
      override fun onDiscoveryStarted(type: String) = Unit
      override fun onDiscoveryStopped(type: String) = Unit
      override fun onStartDiscoveryFailed(type: String, code: Int) { if (epoch == generation) error = "Device discovery could not start. Check local-network access." }
      override fun onStopDiscoveryFailed(type: String, code: Int) = Unit
      override fun onServiceLost(info: NsdServiceInfo) { if (epoch == generation) devices.entries.removeAll { it.value.optString("service") == info.serviceName } }
      override fun onServiceFound(info: NsdServiceInfo) {
        if (epoch != generation || devices.size >= 100) return
        synchronized(queue) { if (queue.size < 100) queue.add(info); if (resolving) return; resolving = true }
        next(epoch)
      }
    }
    listener = callback
    try { manager.discoverServices("_googlecast._tcp", NsdManager.PROTOCOL_DNS_SD, callback) }
    catch (_: Exception) { error = "Device discovery could not start. Check local-network access." }
  }
  @Suppress("DEPRECATION")
  private fun next(epoch: Int) {
    val info = synchronized(queue) {
      if (epoch != generation) return
      queue.poll().also { if (it == null) resolving = false }
    } ?: return
    try { manager?.resolveService(info, object : NsdManager.ResolveListener {
      override fun onResolveFailed(service: NsdServiceInfo, code: Int) { next(epoch) }
      override fun onServiceResolved(service: NsdServiceInfo) {
        if (epoch == generation) {
          val host = service.host?.hostAddress
          val capabilities = service.attributes["ca"]?.toString(Charsets.UTF_8)?.toLongOrNull()
          if (host != null && capabilities?.let { it and 1L == 0L } != true && devices.size < 100) {
            val port = service.port.takeIf { it in 1..65535 } ?: 8009
            val id = "$host:$port"
            val name = service.attributes["fn"]?.toString(Charsets.UTF_8)?.take(200) ?: service.serviceName.take(200)
            devices[id] = JSONObject().put("id", id).put("name", name).put("host", host).put("port", port).put("service", service.serviceName)
          }
        }
        next(epoch)
      }
    }) } catch (_: Exception) { next(epoch) }
  }
  fun close() { requested=false; main.post { setEnabled(false) } }
}
