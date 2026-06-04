package im.zyx.phonebridge

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.Build
import dagger.hilt.android.HiltAndroidApp

@HiltAndroidApp
class PhoneBridgeApp : Application() {

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(NotificationManager::class.java)
            val ch = NotificationChannel(
                CHANNEL_BRIDGE,
                getString(R.string.notif_channel_bridge),
                NotificationManager.IMPORTANCE_LOW
            )
            nm?.createNotificationChannel(ch)
        }
    }

    companion object {
        const val CHANNEL_BRIDGE = "phonebridge.foreground"
    }
}
