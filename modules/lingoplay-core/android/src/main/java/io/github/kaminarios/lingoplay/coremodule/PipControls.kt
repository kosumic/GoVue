package io.github.kaminarios.lingoplay.coremodule

import android.app.Activity
import android.app.PendingIntent
import android.app.PictureInPictureParams
import android.app.RemoteAction
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.core.content.ContextCompat
import org.json.JSONObject
import java.lang.ref.WeakReference

/** Renders Rust's controls and adapts Android window transitions. No queue or
 * playback logic lives here. The same Expo player/activity stays mounted. */
@androidx.annotation.RequiresApi(26)
internal class PipControls(
  private val context: Context,
  private val activity: () -> Activity?,
  private val emit: (Map<String, Any>) -> Unit,
) {
  private val main = Handler(Looper.getMainLooper())
  private val prefix = "${context.packageName}.PIP."
  private var video = ""
  private var actions = emptyList<RemoteAction>()
  private var allowed = emptySet<String>()
  private var closed = false
  private var handoff: WeakReference<Activity>? = null
  private var deadline = 0L
  private val receiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      val kind = intent.action?.removePrefix(prefix) ?: return
      val host = activity() ?: return
      if (closed || !host.isInPictureInPictureMode || intent.getStringExtra("videoId") != video || kind !in allowed) return
      emit(mapOf("action" to kind, "videoId" to video))
    }
  }
  init {
    ContextCompat.registerReceiver(context, receiver, IntentFilter().apply {
      for (id in listOf("audio", "toggle", "previous", "next")) addAction(prefix + id)
    }, ContextCompat.RECEIVER_NOT_EXPORTED)
  }
  fun configure(raw: String) {
    if (closed || raw.length > 4096) return
    val config = JSONObject(raw)
    val id = config.optString("videoId")
    require(id.isEmpty() || id.matches(Regex("[A-Za-z0-9_-]{11}")))
    val values = config.getJSONArray("actions")
    require(values.length() <= 3)
    val enabled = mutableSetOf<String>()
    val rendered = mutableListOf<RemoteAction>()
    for (index in 0 until values.length()) {
      val item = values.getJSONObject(index)
      val kind = item.getString("id")
      val drawable = when (item.getString("icon")) {
        "headphones" -> R.drawable.lingoplay_pip_headphones
        "previous" -> R.drawable.lingoplay_pip_previous
        "next" -> R.drawable.lingoplay_pip_next
        "pause" -> R.drawable.lingoplay_pip_pause
        "play" -> R.drawable.lingoplay_pip_play
        else -> error("Invalid PiP icon")
      }
      require(kind in listOf("audio", "previous", "toggle", "next"))
      val title = item.getString("label")
      require(title.length in 1..80)
      val intent = Intent(prefix + kind).setPackage(context.packageName).putExtra("videoId", id)
      val pending = PendingIntent.getBroadcast(context, 10490, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
      val action = RemoteAction(Icon.createWithResource(context, drawable), title, title, pending)
      action.isEnabled = item.getBoolean("enabled")
      if (action.isEnabled) enabled.add(kind)
      rendered.add(action)
    }
    video = id
    allowed = enabled
    actions = rendered
    activity()?.let { host ->
      if (!host.isFinishing && !host.isDestroyed) {
        runCatching { host.setPictureInPictureParams(PictureInPictureParams.Builder().setActions(actions).build()) }
      }
    }
  }
  fun foreground() {
    if (video.isNotEmpty()) emit(mapOf("action" to "foreground", "videoId" to video, "handoff" to (handoff != null)))
  }
  fun backgroundAudio(expectedVideo: String): Boolean {
    val host = activity() ?: return false
    if (closed || handoff != null || expectedVideo != video || "audio" !in allowed || !host.isInPictureInPictureMode) return false
    handoff = WeakReference(host)
    deadline = SystemClock.elapsedRealtime() + 4000
    suppressAutoEnter(host)
    // Android moves a pinned task behind Home while retaining its activity.
    // Keeping this task alive also retains React's existing Expo player.
    return try {
      if (!host.moveTaskToBack(true)) { failHandoff(); return false }
      main.postDelayed(::finishHandoff, 80)
      true
    } catch (_: Exception) {
      failHandoff()
      false
    }
  }
  private fun suppressAutoEnter(host: Activity) {
    if (Build.VERSION.SDK_INT >= 31) runCatching {
      host.setPictureInPictureParams(PictureInPictureParams.Builder().setAutoEnterEnabled(false).build())
    }
  }
  private fun finishHandoff() {
    val host = handoff?.get() ?: return
    if (closed || host.isFinishing || host.isDestroyed || SystemClock.elapsedRealtime() >= deadline) { failHandoff(); return }
    suppressAutoEnter(host)
    if (!host.isInPictureInPictureMode) {
      handoff = null
      emit(mapOf("action" to "audio_complete", "videoId" to video))
      return
    }
    main.postDelayed(::finishHandoff, 80)
  }
  private fun failHandoff() {
    handoff = null
    if (!closed) emit(mapOf("action" to "audio_failed", "videoId" to video))
  }
  fun close() {
    closed = true
    handoff = null
    main.removeCallbacksAndMessages(null)
    runCatching { context.unregisterReceiver(receiver) }
  }
}
