package io.github.kaminarios.lingoplay.coremodule

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import io.github.kaminarios.lingoplay.core.AiPlatform
import io.github.kaminarios.lingoplay.core.AiSecret
import io.github.kaminarios.lingoplay.core.AiHttpRequest
import io.github.kaminarios.lingoplay.core.AiResponse
import java.security.KeyStore
import java.util.concurrent.TimeUnit
import java.util.concurrent.Executors
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/** App-private ciphertext, with a non-exportable Android Keystore AES key. */
internal class NativeAiPlatform(context: Context) : AiPlatform {
  private val prefs = context.getSharedPreferences("lingoplay_ai_secrets", Context.MODE_PRIVATE)
  private val alias = "io.github.kaminarios.lingoplay.ai"
  private val cancellation = Executors.newSingleThreadScheduledExecutor { runnable -> Thread(runnable, "LingoPlay AI cancellation").apply { isDaemon = true } }
  private val client = OkHttpClient.Builder().connectTimeout(15, TimeUnit.SECONDS)
    .readTimeout(180, TimeUnit.SECONDS).callTimeout(300, TimeUnit.SECONDS)
    .followRedirects(false).followSslRedirects(false).build()

  @Synchronized private fun encryptionKey(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey(alias, null) as? SecretKey)?.let { return it }
    return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
      init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
    }.generateKey()
  }
  private fun valid(key: String) = key.startsWith("ai.") && key.length <= 100 && key.all { it.isLetterOrDigit() || it in "._-" }
  override fun secretRead(key: String): AiSecret = try {
    require(valid(key))
    val value = prefs.getString(key, null)
    if (value == null) AiSecret(true, "") else {
      val bytes = Base64.decode(value, Base64.NO_WRAP)
      require(bytes.size > 28)
      val cipher = Cipher.getInstance("AES/GCM/NoPadding")
      cipher.init(Cipher.DECRYPT_MODE, encryptionKey(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
      cipher.updateAAD(key.toByteArray(Charsets.UTF_8))
      AiSecret(true, String(cipher.doFinal(bytes.copyOfRange(12, bytes.size)), Charsets.UTF_8))
    }
  } catch (_: Exception) { AiSecret(false, "") }
  override fun secretWrite(key: String, value: String): Boolean = try {
    require(valid(key) && value.length <= 100_000)
    if (value.isEmpty()) prefs.edit().remove(key).commit() else {
      val cipher = Cipher.getInstance("AES/GCM/NoPadding")
      cipher.init(Cipher.ENCRYPT_MODE, encryptionKey())
      cipher.updateAAD(key.toByteArray(Charsets.UTF_8))
      val bytes = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
      prefs.edit().putString(key, Base64.encodeToString(bytes, Base64.NO_WRAP)).commit()
    }
  } catch (_: Exception) { false }

  override fun request(request: AiHttpRequest, response: AiResponse) {
    try {
      require(request.url.startsWith("https://"))
      val builder = Request.Builder().url(request.url)
      for (header in request.headers) builder.header(header.name, header.value)
      if (request.method == "POST") builder.post(request.body.toByteArray(Charsets.UTF_8).toRequestBody())
      else require(request.method == "GET")
      val call = client.newCall(builder.build())
      val watch = cancellation.scheduleAtFixedRate({ if (response.isCancelled()) call.cancel() }, 200, 200, TimeUnit.MILLISECONDS)
      try { call.execute().use { reply ->
        response.start(reply.code)
        reply.body?.byteStream()?.use { input ->
          val buffer = ByteArray(16 * 1024)
          while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            if (!response.push(buffer.copyOf(count))) break
          }
        }
      } } finally { watch.cancel(false) }
    } catch (_: Exception) { response.failed() }
  }
}
