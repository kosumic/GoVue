package io.github.kaminarios.govue.coremodule

import android.media.MediaCodecList
import android.os.Build

internal class NativeMediaCapabilities {
  private data class Decoder(val hardware: Boolean, val video: android.media.MediaCodecInfo.VideoCapabilities?)
  private val codecs by lazy { MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.filter { !it.isEncoder } }
  private val capabilities = java.util.concurrent.ConcurrentHashMap<String, List<Decoder>>()

  fun supports(mime: String, width: Int, height: Int, fps: Int): Boolean {
    val candidates = capabilities.computeIfAbsent(mime) {
      codecs.filter { codec -> codec.supportedTypes.any { it.equals(mime, ignoreCase = true) } }.mapNotNull { codec ->
        runCatching {
          val hardware = if (Build.VERSION.SDK_INT >= 29) codec.isHardwareAccelerated
            else !codec.name.startsWith("OMX.google.") && !codec.name.startsWith("c2.android.")
          Decoder(hardware, if (mime.startsWith("audio/")) null else codec.getCapabilitiesForType(mime).videoCapabilities)
        }.getOrNull()
      }
    }
    return candidates.any { decoder -> runCatching {
      if (mime.startsWith("audio/")) true
      else if (!decoder.hardware && (mime == "video/av01" || height > 1080)) false
      else if (width <= 0 || height <= 0) mime != "video/av01" && height <= 1080
      else decoder.video?.let { video ->
        if (fps > 0) video.areSizeAndRateSupported(width, height, fps.toDouble()) else video.isSizeSupported(width, height)
      } ?: false
    }.getOrDefault(false) }
  }
}
