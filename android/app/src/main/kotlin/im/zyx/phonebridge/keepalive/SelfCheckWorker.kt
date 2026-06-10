package im.zyx.phonebridge.keepalive

import android.content.Context
import androidx.hilt.work.HiltWorker
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import im.zyx.phonebridge.data.PrefsRepository
import kotlinx.coroutines.flow.first
import javax.inject.Inject

/**
 * Periodic worker that asks every registered [BridgeAbility] whether
 * it's still granted, and forwards the result to
 * [MissingAbilityNotifier] so the user gets a heads-up alert when a
 * capability was revoked.
 *
 * Why polling rather than a broadcast: Android does not broadcast a
 * notification when the user disables the notification-listener
 * binding (or AccessibilityService, or any similar system gate).
 * The only reliable cross-OEM signal is to re-check the relevant
 * `Settings.Secure` string on a schedule. WorkManager is the
 * official, doze-bucket-respecting mechanism for doing that.
 */
@HiltWorker
class SelfCheckWorker @AssistedInject constructor(
    @Assisted appContext: Context,
    @Assisted params: WorkerParameters,
    private val abilities: Set<@JvmSuppressWildcards BridgeAbility>,
    private val prefs: PrefsRepository,
) : CoroutineWorker(appContext, params) {

    override suspend fun doWork(): Result {
        val missing = mutableListOf<BridgeAbility>()
        val available = mutableListOf<BridgeAbility>()
        for (a in abilities) {
            if (a.isAvailable(applicationContext)) available.add(a) else missing.add(a)
        }
        // Cheap debounce: only post alerts if the alert was not
        // dismissed in the last 10 minutes by the user (i.e. the
        // notification's auto-cancel click went through). We don't
        // track "dismissed" per-ability yet, so we just post every
        // time the worker fires when something is missing — this is
        // bounded to once per 15 minutes.
        if (missing.isNotEmpty() || available.isNotEmpty()) {
            MissingAbilityNotifierResolver.resolve(applicationContext)
                .reconcile(missing, available)
        }
        return Result.success()
    }

    companion object {
        // Marks the abilities that the user has permanently
        // suppressed. If a missing ability is in this set, do not
        // alert. We expose a way for the UI to add ids to this set
        // (e.g. a "don't show again" action on the alert). Currently
        // we honour nothing — reserved for v2.
        @Suppress("unused")
        suspend fun isSuppressed(prefs: PrefsRepository, id: String): Boolean {
            val csv = prefs.suppressedAlertsCsv.first().orEmpty()
            return csv.split(',').any { it.trim() == id }
        }
    }
}

/**
 * Resolves a singleton [MissingAbilityNotifier] from the
 * application context. We can't @Inject a system service into a
 * Worker that doesn't have a Hilt entry point of its own, so we go
 * through a tiny holder class. This is intentionally not a
 * Service-locator anti-pattern: the notifier is a pure
 * [android.app.NotificationManager] wrapper and the resolver is
 * scoped to the worker process.
 */
internal object MissingAbilityNotifierResolver {
    fun resolve(ctx: Context): MissingAbilityNotifier {
        val entryPoint = dagger.hilt.android.EntryPointAccessors.fromApplication(
            ctx.applicationContext,
            EntryPoint::class.java,
        )
        return entryPoint.notifier()
    }

    @dagger.hilt.EntryPoint
    @dagger.hilt.InstallIn(dagger.hilt.components.SingletonComponent::class)
    interface EntryPoint {
        fun notifier(): MissingAbilityNotifier
    }
}
