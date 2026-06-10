package im.zyx.phonebridge.keepalive

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import dagger.hilt.android.qualifiers.ApplicationContext
import java.util.concurrent.TimeUnit
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Thin wrapper over [WorkManager] that encapsulates the unique work
 * names and policies used by the bridge. Centralising the names here
 * means we never have to remember whether the periodic check is named
 * `phonebridge.selfcheck` or `self_check` at the call site.
 */
@Singleton
class KeepAliveWorkScheduler @Inject constructor(
    @ApplicationContext private val context: Context,
) {
    private val wm: WorkManager get() = WorkManager.getInstance(context)

    /**
     * One-shot startup work. Enqueued by [BootReceiver] on
     * BOOT_COMPLETED / MY_PACKAGE_REPLACED. Returns immediately; the
     * actual foreground-service start happens inside the worker.
     *
     * [ExistingWorkPolicy.REPLACE] so a fresh reboot that fires the
     * receiver twice (some OEMs do this) doesn't queue two
     * start-service calls back to back.
     */
    fun enqueueStartup() {
        val req = OneTimeWorkRequestBuilder<StartupWorker>()
            .setConstraints(
                Constraints.Builder()
                    .setRequiredNetworkType(NetworkType.CONNECTED)
                    .build()
            )
            .setBackoffCriteria(
                androidx.work.BackoffPolicy.EXPONENTIAL,
                10_000L,
                TimeUnit.MILLISECONDS,
            )
            .build()
        wm.enqueueUniqueWork(WORK_STARTUP, ExistingWorkPolicy.REPLACE, req)
    }

    /**
     * Periodic self-check. Enqueued once at app start (see
     * [PhoneBridgeApp.onCreate]) and re-enqueued with KEEP on every
     * subsequent launch so the schedule is stable across reboots.
     *
     * Minimum interval enforced by WorkManager is 15 minutes; that's
     * fine for this purpose because the only ability we currently
     * check is the notification listener, which the user toggles
     * manually.
     */
    fun schedulePeriodicSelfCheck() {
        val req = PeriodicWorkRequestBuilder<SelfCheckWorker>(15, TimeUnit.MINUTES)
            .setConstraints(
                Constraints.Builder()
                    .setRequiredNetworkType(NetworkType.NOT_REQUIRED)
                    .build()
            )
            .build()
        wm.enqueueUniquePeriodicWork(
            WORK_SELFCHECK,
            ExistingPeriodicWorkPolicy.KEEP,
            req,
        )
    }

    companion object {
        const val WORK_STARTUP = "phonebridge.startup"
        const val WORK_SELFCHECK = "phonebridge.selfcheck"
    }
}
