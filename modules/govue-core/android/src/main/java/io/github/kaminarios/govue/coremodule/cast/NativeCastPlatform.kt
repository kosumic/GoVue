package io.github.kaminarios.govue.coremodule.cast

import android.content.Context
import io.github.kaminarios.govue.core.CastPlatform
import io.github.kaminarios.govue.core.execute
import java.io.File

internal class NativeCastPlatform(context: Context) : CastPlatform {
  private val network = CastNetwork(context)
  init {
    CastActivityService.stopCast = { execute("{\"op\":\"cast_stop_all\"}", File(context.filesDir, "transcripts").path) }
  }
  override fun wifiAddress() = network.wifiAddress()
  override fun bindWifiSocket(socketFd: UInt, address: String) =
    socketFd <= Int.MAX_VALUE.toUInt() && network.bindWifiSocket(socketFd.toInt(), address)
  override fun connect(host: String, port: UShort) = network.connect(host, port.toInt())
  override fun read(socket: String) = network.read(socket)
  override fun write(socket: String, frame: ByteArray) = network.write(socket, frame)
  override fun close(socket: String) = network.close(socket)
  override fun activity(active: Boolean) = network.activity(active)
}
