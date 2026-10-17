package io.github.kaminarios.lingoplay.coremodule

import io.github.kaminarios.lingoplay.core.ExtractionHttp
import io.github.kaminarios.lingoplay.core.ExtractionRequest
import io.github.kaminarios.lingoplay.core.ExtractionReply
import io.github.kaminarios.lingoplay.core.ExtractionHeader
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/** Byte transport only. Rust supplies the validated URL, headers and protocol. */
internal class NativeExtractionHttp : ExtractionHttp {
  private val client = OkHttpClient.Builder().connectTimeout(8, TimeUnit.SECONDS)
    .readTimeout(15, TimeUnit.SECONDS).callTimeout(18, TimeUnit.SECONDS)
    .followRedirects(false).followSslRedirects(false).build()
  override fun request(request: ExtractionRequest): ExtractionReply = try {
    val limit = request.maxBytes.toInt()
    require(limit in 1..8_000_000)
    val builder = Request.Builder().url(request.url)
    for (header in request.headers) builder.header(header.name, header.value)
    if (request.method == "POST") builder.post(request.body.toRequestBody())
    else require(request.method == "GET")
    client.newCall(builder.build()).execute().use { response ->
      val body = java.io.ByteArrayOutputStream()
      response.body?.byteStream()?.use { input ->
        val buffer = ByteArray(16384)
        while (true) {
          val count = input.read(buffer, 0, minOf(buffer.size, limit + 1 - body.size()))
          if (count <= 0) break
          body.write(buffer, 0, count)
          if (body.size() > limit) break
        }
      }
      ExtractionReply(response.code, body.toByteArray(), response.headers.values("Set-Cookie").map { ExtractionHeader("Set-Cookie", it) })
    }
  } catch (_: Exception) { ExtractionReply(0, byteArrayOf(), emptyList()) }
}
