package io.github.kaminarios.lingoplay.coremodule

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import com.facebook.react.HeadlessJsTaskService
import com.facebook.react.bridge.Arguments
import com.facebook.react.jstasks.HeadlessJsTaskConfig

/** Keeps React Native's platform orchestration scheduled in the background.
 * Transfer state, addresses and work scheduling remain exclusively in Rust. */
class DownloadActivityService : HeadlessJsTaskService() {
  private var taskStarted=false
  companion object { @Volatile var activeCount=0; private set }
  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val count = intent?.getIntExtra("count", 0) ?: 0
    activeCount=count.coerceIn(0,2000)
    if (count <= 0) { stopForeground(STOP_FOREGROUND_REMOVE); stopSelf(); return START_NOT_STICKY }
    val manager = getSystemService(NotificationManager::class.java)
    if (android.os.Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel("downloads", "Downloads", NotificationManager.IMPORTANCE_LOW))
    val open = packageManager.getLaunchIntentForPackage(packageName)
    val builder = (if (android.os.Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "downloads") else Notification.Builder(this))
      .setSmallIcon(android.R.drawable.stat_sys_download)
      .setContentTitle(if (count == 1) "Downloading" else "Downloading $count videos")
      .setContentText(intent?.getStringExtra("title")?.take(200) ?: "LingoPlay")
      .setOnlyAlertOnce(true).setOngoing(true).setCategory(Notification.CATEGORY_PROGRESS)
    if (open != null) builder.setContentIntent(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
    val received = intent?.getLongExtra("received", 0) ?: 0
    val total = intent?.getLongExtra("total", 0) ?: 0
    builder.setProgress(100, if (total > 0) (received.coerceIn(0, total) * 100 / total).toInt() else 0, total <= 0)
    startForeground(1047, builder.build())
    if(!taskStarted) {
      taskStarted=true
      startTask(HeadlessJsTaskConfig("LingoPlayDownloads", Arguments.createMap(), 0L, true))
    }
    return START_NOT_STICKY
  }
  override fun onDestroy() { activeCount=0; super.onDestroy() }
  override fun onTimeout(startId: Int, fgsType: Int) {
    activeCount=0
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelf()
  }
}
