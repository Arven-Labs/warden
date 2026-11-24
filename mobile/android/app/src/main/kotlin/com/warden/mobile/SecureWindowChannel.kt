package com.warden.mobile

import android.app.Activity
import android.view.WindowManager
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/**
 * `FLAG_SECURE` for the screen that shows the 24 words of a new key (P-49).
 *
 * While the flag is set Android blocks screenshots and screen recordings of the window and draws a blank card in the recent-apps list.
 * It lives on the window, so it stays set when the app goes to the background: that is what protects the thumbnail. Flutter asks to set it
 * before it shows the words and to clear it when they are gone. Both calls are idempotent, and always on the main thread (the channel's).
 */
object SecureWindowChannel {
    const val NAME = "com.warden.mobile/secure_window"

    fun register(messenger: BinaryMessenger, activity: Activity) {
        MethodChannel(messenger, NAME).setMethodCallHandler { call, result ->
            when (call.method) {
                "enable" -> {
                    activity.window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
                    result.success(null)
                }
                "disable" -> {
                    activity.window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
    }
}
