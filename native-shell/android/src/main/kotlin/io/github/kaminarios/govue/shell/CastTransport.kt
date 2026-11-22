package io.github.kaminarios.govue.shell

import android.content.Context
import android.view.View
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext
import com.longbridge.nsh.NshView
import io.github.kaminarios.govue.coremodule.cast.CastActivityService
import io.github.kaminarios.govue.coremodule.cast.CastDiscovery
import io.github.kaminarios.govue.coremodule.cast.CastNetwork
import java.io.File

class CastTransport private constructor(context: Context) {
  private val network = CastNetwork(context)

  fun wifiAddress(): String = network.wifiAddress()
  fun bindWifiSocket(fd: Int, address: String): Boolean = network.bindWifiSocket(fd, address)
  fun connect(host: String, port: Int): String = network.connect(host, port)
  fun read(socket: String): ByteArray = network.read(socket)
  fun write(socket: String, frame: ByteArray): Boolean = network.write(socket, frame)
  fun close(socket: String) = network.close(socket)
  fun activity(active: Boolean): Boolean = network.activity(active)

  companion object {
    @JvmStatic private external fun installNative(adapter: CastTransport)
    @JvmStatic private external fun stopNative(root: String)

    @JvmStatic fun register(surface: NshView, context: Context) {
      val app = context.applicationContext
      CastActivityService.stopCast = { stopNative(File(app.filesDir, "transcripts").path) }
      installNative(CastTransport(app))
      surface.registerComponent("govue/cast-discovery") { events -> Discovery(context, events) }
    }
  }

  private class Discovery(context: Context, private val events: NshComponentContext) : NshComponent {
    private val discovery = CastDiscovery(context.applicationContext)
    private val native = View(context)
    private var enabled = false

    override fun view(): View = native

    override fun setProp(name: String, value: Any?) {
      if (name == "enabled") {
        enabled = value == true
        discovery.snapshot(enabled)
      }
    }

    override fun command(name: String, args: Any?) {
      if (name == "snapshot" && enabled) events.emit("devices", discovery.snapshot(true))
    }

    override fun destroy() = discovery.close()
  }
}
