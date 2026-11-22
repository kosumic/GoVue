package io.github.kaminarios.govue.shell

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import com.longbridge.nsh.GoVueActivity
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext
import com.longbridge.nsh.NshView

class ReplyNotifications private constructor(private val app: Context) {
  private val main = Handler(Looper.getMainLooper())
  private val manager = app.getSystemService(NotificationManager::class.java)
  private val revisions = mutableMapOf<String, ULong>()

  private fun channel() {
    manager.createNotificationChannel(
      NotificationChannel(CHANNEL, "Chat replies", NotificationManager.IMPORTANCE_HIGH).apply {
        description = "When an AI reply is ready while you are away from the chat"
        enableVibration(true)
      }
    )
  }

  @SuppressLint("MissingPermission")
  fun update(chatId: String, show: Boolean, revision: Long) {
    if (!chatId.matches(chatIdPattern)) return
    val version = revision.toULong()
    main.post {
      if (version <= (revisions[chatId] ?: 0uL)) return@post
      revisions[chatId] = version
      if (!show) {
        manager.cancel(chatId, 3)
        return@post
      }
      if (!manager.areNotificationsEnabled() ||
        Build.VERSION.SDK_INT >= 33 && app.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
      ) return@post
      channel()
      val intent = Intent(app, ReplyNotificationActivity::class.java).setAction(ACTION)
      intent.data = Uri.Builder().scheme("govue-chat").authority("reply").appendPath(chatId).build()
      intent.putExtra(TARGET, chatId)
      val tap = PendingIntent.getActivity(app, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
      val notification = Notification.Builder(app, CHANNEL)
        .setSmallIcon(R.drawable.govue_chat_notification)
        .setContentTitle("Chat reply ready")
        .setContentText("Tap to open your conversation.")
        .setCategory(Notification.CATEGORY_MESSAGE)
        .setVisibility(Notification.VISIBILITY_PRIVATE)
        .setAutoCancel(true)
        .setContentIntent(tap)
        .setOnlyAlertOnce(true)
        .build()
      runCatching { manager.notify(chatId, 3, notification) }
    }
  }

  class Binding internal constructor(
    private val activity: Activity,
    private val notifications: ReplyNotifications,
  ) {
    private var surface: NshComponentContext? = null
    private var asked = false

    internal fun register(view: NshView) {
      view.registerComponent("govue/chat-notifications") { events ->
        object : NshComponent {
          private val native = View(activity)

          override fun view() = native

          override fun setProp(name: String, value: Any?) {
            when (name) {
              "observe" -> if (value == true) {
                surface = events
                drain()
              }
              "prepare" -> if (value == true) prepare()
            }
          }

          override fun destroy() {
            if (surface === events) surface = null
          }
        }
      }
    }

    fun drain() {
      val events = surface ?: return
      takeLaunch()?.let { events.emit("open", it) }
    }

    private fun prepare() {
      notifications.channel()
      if (Build.VERSION.SDK_INT >= 33 && !asked &&
        activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
      ) {
        asked = true
        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 9734)
      }
    }
  }

  companion object {
    private const val ACTION = "io.github.kaminarios.govue.CHAT_REPLY"
    private const val TARGET = "govue.chatId"
    private const val CHANNEL = "chat_replies"
    private val chatIdPattern = Regex("[A-Za-z0-9_-]{1,64}")
    private var pendingLaunch: String? = null
    private var installed: ReplyNotifications? = null

    @JvmStatic private external fun installNative(adapter: ReplyNotifications)

    @JvmStatic fun register(view: NshView, activity: Activity): Binding {
      val platform = installed ?: ReplyNotifications(activity.applicationContext).also {
        installNative(it)
        installed = it
      }
      return Binding(activity, platform).also { it.register(view) }
    }

    internal fun capture(intent: Intent): Boolean {
      if (intent.action != ACTION) return false
      val target = intent.getStringExtra(TARGET)?.takeIf { it.matches(chatIdPattern) } ?: return false
      pendingLaunch = target
      return true
    }

    private fun takeLaunch(): String? = pendingLaunch.also { pendingLaunch = null }
  }
}

class ReplyNotificationActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    if (ReplyNotifications.capture(intent)) {
      startActivity(Intent(this, GoVueActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP))
    }
    finish()
  }
}
