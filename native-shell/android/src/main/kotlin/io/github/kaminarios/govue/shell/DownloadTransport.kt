package io.github.kaminarios.govue.shell

import android.content.Context
import io.github.kaminarios.govue.coremodule.AndroidDownloadFiles
import io.github.kaminarios.govue.coremodule.AndroidDownloadHttp

class DownloadTransport private constructor(context: Context) {
  companion object {
    @JvmStatic
    fun install(context: Context) {
      System.loadLibrary("nsh")
      installNative(DownloadTransport(context.applicationContext))
    }

    @JvmStatic private external fun installNative(adapter: DownloadTransport)
    @JvmStatic private external fun execute(command: String, root: String): String
  }

  private val files = AndroidDownloadFiles(context, ::execute)
  private val http = AndroidDownloadHttp.shared(context)

  private fun directory() = files.directory()
  private fun exportDirectory() = files.exportDirectory()
  private fun formats(formats: String) = files.formats(formats)
  private fun mux(video: String, audio: String, output: String) = files.mux(video, audio, output)
  private fun publish(id: String) = files.publish(id)
  private fun deletePublished(uri: String) = files.deletePublished(uri)
  private fun activity(count: Int, title: String, received: Long, total: Long) =
    files.activity(count, title, received, total)

  private fun wifiConnected() = http.wifiConnected()
  private fun fetch(
    address: String, userAgent: String, origin: String, method: String,
    first: Long, last: Long, wifiOnly: Boolean,
  ) = http.fetch(address, userAgent, origin, method, first, last, wifiOnly)
}
