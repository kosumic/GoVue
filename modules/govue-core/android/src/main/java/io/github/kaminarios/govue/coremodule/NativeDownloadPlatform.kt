package io.github.kaminarios.govue.coremodule

import android.content.Context
import io.github.kaminarios.govue.core.DownloadPlatform
import io.github.kaminarios.govue.core.execute

internal class NativeDownloadPlatform(context: Context) : DownloadPlatform {
  private val files = AndroidDownloadFiles(context, ::execute)
  override fun directory() = files.directory()
  override fun exportDirectory() = files.exportDirectory()
  override fun supportedFormats(formats: String) = files.formats(formats)
  fun formats(raw: String) = files.formats(raw)
  override fun mux(video: String, audio: String, output: String) = files.mux(video, audio, output)
  override fun publish(id: String) = files.publish(id)
  override fun deletePublished(uri: String) = files.deletePublished(uri)
  override fun activity(count: UInt, title: String, received: ULong, total: ULong) =
    files.activity(
      count.coerceAtMost(2000u).toInt(), title,
      received.coerceAtMost(200_000_000_000uL).toLong(),
      total.coerceAtMost(200_000_000_000uL).toLong(),
    )
}
