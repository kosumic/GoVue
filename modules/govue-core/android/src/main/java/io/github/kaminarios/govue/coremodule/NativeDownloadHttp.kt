package io.github.kaminarios.govue.coremodule

import android.content.Context
import io.github.kaminarios.govue.core.DownloadHttp
import io.github.kaminarios.govue.core.DownloadRequest
import io.github.kaminarios.govue.core.MediaRangeReply

internal class NativeDownloadHttp private constructor(private val http: AndroidDownloadHttp) : DownloadHttp {
  companion object {
    fun shared(context: Context): NativeDownloadHttp = NativeDownloadHttp(AndroidDownloadHttp.shared(context))
  }

  override fun wifiConnected() = http.wifiConnected()

  override fun fetch(request: DownloadRequest): MediaRangeReply {
    val result = http.fetch(
      request.url, request.userAgent, request.origin, request.method,
      request.range?.start?.toLong() ?: -1, request.range?.end?.toLong() ?: -1,
      request.wifiOnly,
    )
    return MediaRangeReply(result.status, result.body, result.location)
  }
}
