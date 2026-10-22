package io.github.kaminarios.govue.coremodule

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import io.github.kaminarios.govue.core.ChatNotificationPlatform

internal class NativeChatNotifications(private val context: Context) : ChatNotificationPlatform {
  private val main = Handler(Looper.getMainLooper())
  private val revisions = mutableMapOf<String, ULong>()
  private val manager = context.getSystemService(NotificationManager::class.java)

  fun prepare() {
    if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(
      NotificationChannel("chat_replies", "Chat replies", NotificationManager.IMPORTANCE_HIGH).apply {
        description = "When an AI reply is ready while you are away from the chat"
        enableVibration(true)
      }
    )
  }

  override fun update(chatId: String, show: Boolean, revision: ULong) {
    main.post {
      if (revision <= (revisions[chatId] ?: 0uL)) return@post
      revisions[chatId] = revision
      if (!show) { manager.cancel(chatId, 3); return@post }
      if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) return@post
      if (Build.VERSION.SDK_INT >= 24 && !manager.areNotificationsEnabled()) return@post
      prepare()
      // Capture the target before resuming a surviving React activity task:
      // Android may recreate that task with its original launcher intent.
      val intent = Intent(context, ChatNotificationActivity::class.java).setAction(ACTION)
      // Unique PendingIntent identity per conversation, without reply text or URLs.
      intent.data = Uri.Builder().scheme("govue-chat").authority("reply").appendPath(chatId).build()
      intent.putExtra(TARGET, chatId)
      val tap = PendingIntent.getActivity(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
      val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(context, "chat_replies") else Notification.Builder(context)
      val notification = builder.setSmallIcon(R.drawable.govue_chat_notification)
        .setContentTitle("Chat reply ready").setContentText("Tap to open your conversation.")
        .setCategory(Notification.CATEGORY_MESSAGE).setVisibility(Notification.VISIBILITY_PRIVATE)
        .setAutoCancel(true).setContentIntent(tap).setOnlyAlertOnce(true)
        .apply { if (Build.VERSION.SDK_INT < 26) setDefaults(Notification.DEFAULT_ALL).setPriority(Notification.PRIORITY_HIGH) }
        .build()
      runCatching { manager.notify(chatId, 3, notification) }
    }
  }

  companion object {
    const val ACTION = "io.github.kaminarios.govue.CHAT_REPLY"
    const val TARGET = "govue.chatId"
    private var pendingLaunch: String? = null
    @Synchronized fun capture(intent: Intent?) { target(intent)?.let { pendingLaunch = it } }
    @Synchronized fun hasLaunch(): Boolean = pendingLaunch != null
    @Synchronized fun takeLaunch(): String? = pendingLaunch.also { pendingLaunch = null }
    fun target(intent: Intent?): String? = if (intent?.action == ACTION)
      intent.getStringExtra(TARGET)?.takeIf { it.matches(Regex("[A-Za-z0-9_-]{1,64}")) } else null
  }
}
