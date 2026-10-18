package io.github.kaminarios.govue.coremodule

import android.content.Context
import android.database.ContentObserver
import android.hardware.SensorManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.OrientationEventListener

/** Platform sensor adapter. Angle bands and fullscreen policy belong to Rust. */
internal class PhysicalOrientation(
  private val context: Context,
  private val emit: (Map<String, Any>) -> Unit,
) {
  private val main = Handler(Looper.getMainLooper())
  private var wanted = false
  private var foreground = true
  private var observing = false
  private var degrees = -1
  private var lastSent = 0L
  private val sensor = object : OrientationEventListener(context, SensorManager.SENSOR_DELAY_UI) {
    override fun onOrientationChanged(value: Int) {
      degrees = value
      if (SystemClock.elapsedRealtime() - lastSent >= 100) report()
    }
  }
  private val observer = object : ContentObserver(main) {
    override fun onChange(selfChange: Boolean) = report()
  }
  private fun report() {
    if (!observing) return
    lastSent = SystemClock.elapsedRealtime()
    val autoRotate = runCatching {
      Settings.System.getInt(context.contentResolver, Settings.System.ACCELEROMETER_ROTATION, 1) == 1
    }.getOrDefault(false)
    emit(mapOf("degrees" to degrees, "systemAutoRotate" to autoRotate))
  }
  fun observe(value: Boolean) { wanted = value; update() }
  fun foreground(value: Boolean) { foreground = value; update() }
  private fun update() {
    val next = wanted && foreground
    if (next == observing) return
    observing = next
    if (next) {
      if (sensor.canDetectOrientation()) sensor.enable()
      context.contentResolver.registerContentObserver(Settings.System.getUriFor(Settings.System.ACCELEROMETER_ROTATION), false, observer)
      report()
    } else {
      sensor.disable()
      context.contentResolver.unregisterContentObserver(observer)
    }
  }
  fun close() { wanted = false; update() }
}
