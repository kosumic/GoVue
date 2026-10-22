package io.github.kaminarios.govue.coremodule

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/** Preserve notification targets before Android restores an old launcher task. */
class ChatNotificationActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    NativeChatNotifications.capture(intent)
    packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
      launch.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
      startActivity(launch)
    }
    finish()
  }
}
