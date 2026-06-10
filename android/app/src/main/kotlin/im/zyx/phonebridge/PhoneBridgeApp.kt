package im.zyx.phonebridge

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import androidx.hilt.work.HiltWorkerFactory
import androidx.work.Configuration
import dagger.hilt.android.HiltAndroidApp
import im.zyx.phonebridge.keepalive.KeepAliveWorkScheduler
import javax.inject.Inject

@HiltAndroidApp
class PhoneBridgeApp : Application(), Configuration.Provider {

    @Inject lateinit var workerFactory: HiltWorkerFactory
    @Inject lateinit var keepAliveScheduler: KeepAliveWorkScheduler

    override val workManagerConfiguration: Configuration
        get() = Configuration.Builder()
            .setWorkerFactory(workerFactory)
            .setMinimumLoggingLevel(android.util.Log.INFO)
            .build()

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
        keepAliveScheduler.schedulePeriodicSelfCheck()
    }

    private fun ensureNotificationChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        val low = NotificationChannel(
            CHANNEL_BRIDGE,
            getString(R.string.notif_channel_bridge),
            NotificationManager.IMPORTANCE_LOW
        )
        nm?.createNotificationChannel(low)
        val alerts = NotificationChannel(
            CHANNEL_ALERTS,
            getString(R.string.notif_channel_alerts),
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = getString(R.string.notif_channel_alerts_desc)
            enableVibration(true)
        }
        nm?.createNotificationChannel(alerts)
    }

    companion object {
        const val CHANNEL_BRIDGE = "phonebridge.foreground"
        const val CHANNEL_ALERTS = "phonebridge.alerts"
    }
}
