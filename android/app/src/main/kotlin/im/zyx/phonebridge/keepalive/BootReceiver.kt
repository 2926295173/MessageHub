package im.zyx.phonebridge.keepalive

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.work.ListenableWorker
import androidx.work.Worker
import androidx.work.WorkerParameters
import dagger.hilt.android.EntryPointAccessors
import dagger.hilt.EntryPoint
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import im.zyx.phonebridge.network.BridgeService
import androidx.core.content.ContextCompat

private const val TAG = "BootReceiver"

/**
 * Receiver for system boot, lock-screen boot (direct-boot aware), and
 * self-update broadcasts. Hands off to [KeepAliveWorkScheduler] which
 * in turn schedules a [StartupWorker]. The worker is what actually
 * starts [BridgeService] because receivers must return within ~10s
 * and starting a foreground service from a stale process can fail
 * with `ForegroundServiceStartNotAllowedException` on Android 12+.
 *
 * The `directBootAware="true"` attribute in the manifest allows the
 * receiver to run before the user unlocks the device (so a reboot
 * during the night still gets the bridge reconnected by morning).
 */
class BootReceiver : BroadcastReceiver() {

    @EntryPoint
    @InstallIn(SingletonComponent::class)
    interface BootEntryPoint {
        fun scheduler(): KeepAliveWorkScheduler
    }

    override fun onReceive(context: Context, intent: Intent) {
        val action = intent.action ?: return
        Log.i(TAG, "received $action; enqueuing startup work")
        val ep = EntryPointAccessors.fromApplication(
            context.applicationContext,
            BootEntryPoint::class.java,
        )
        ep.scheduler().enqueueStartup()
    }
}

/**
 * Worker that actually starts the foreground service. Runs as soon as
 * the constraints (network available) are met, with exponential
 * backoff up to 5 minutes between retries.
 */
class StartupWorker(
    appContext: Context,
    params: WorkerParameters,
) : Worker(appContext, params) {
    override fun doWork(): ListenableWorker.Result {
        return try {
            val i = Intent(applicationContext, BridgeService::class.java)
            ContextCompat.startForegroundService(applicationContext, i)
            ListenableWorker.Result.success()
        } catch (t: Throwable) {
            Log.w(TAG, "startForegroundService failed: ${t.message}")
            // Retry with backoff; do NOT mark as failure (which
            // cancels the work) — we want the next system event
            // (boot, app open) to re-enqueue.
            ListenableWorker.Result.retry()
        }
    }
}
