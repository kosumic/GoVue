package io.github.kaminarios.govue.coremodule.cast

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.PowerManager
import io.github.kaminarios.govue.core.execute
import java.io.File

/** Keeps the explicitly active LAN transport alive, with a visible revocation action. */
class CastActivityService : Service() {
  private var wake: PowerManager.WakeLock? = null
  private var wifi: WifiManager.WifiLock? = null
  override fun onBind(intent: Intent?) = null
  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == "io.github.kaminarios.govue.STOP_CAST") {
      Thread {
        runCatching { execute("{\"op\":\"cast_stop_all\"}", File(filesDir, "transcripts").path) }
        stopSelf(startId)
      }.apply { isDaemon = true; start() }
      return START_NOT_STICKY
    }
    val manager = getSystemService(NotificationManager::class.java)
    if (android.os.Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel("casting", "Casting", NotificationManager.IMPORTANCE_LOW))
    val stop = PendingIntent.getService(this, 1048, Intent(this, CastActivityService::class.java).setAction("io.github.kaminarios.govue.STOP_CAST"), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    val builder = (if (android.os.Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "casting") else Notification.Builder(this))
      .setSmallIcon(android.R.drawable.ic_media_play).setContentTitle("GoVue casting")
      .setContentText("Cast or a shared player is active on Wi-Fi")
      .setOnlyAlertOnce(true).setOngoing(true).setCategory(Notification.CATEGORY_TRANSPORT)
      .addAction(Notification.Action.Builder(null, "Stop", stop).build())
    packageManager.getLaunchIntentForPackage(packageName)?.let {
      builder.setContentIntent(PendingIntent.getActivity(this, 1048, it, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
    }
    startForeground(1048, builder.build())
    if (wake == null) {
      wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "GoVue:Cast").apply { acquire(4 * 60 * 60 * 1000L) }
      @Suppress("DEPRECATION")
      wifi = (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager).createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "GoVue:Cast").apply { acquire() }
    }
    return START_NOT_STICKY
  }
  override fun onDestroy() {
    wake?.let { if (it.isHeld) it.release() }; wake = null
    wifi?.let { if (it.isHeld) it.release() }; wifi = null
    super.onDestroy()
  }
}
