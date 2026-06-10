package im.zyx.phonebridge.keepalive

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import androidx.core.app.NotificationCompat
import dagger.hilt.android.qualifiers.ApplicationContext
import im.zyx.phonebridge.PhoneBridgeApp
import im.zyx.phonebridge.R
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Posts a heads-up notification for each [BridgeAbility] the system
 * no longer grants us. Idempotent per ability — repeated calls while
 * the ability is still missing update the existing notification; once
 * the ability is re-granted, the next call simply removes the
 * notification.
 *
 * The notification is intentionally loud (IMPORTANCE_HIGH via
 * [PhoneBridgeApp.CHANNEL_ALERTS]) because losing notification
 * mirroring silently is the most common "I don't know why my phone
 * stopped forwarding" failure mode.
 */
@Singleton
class MissingAbilityNotifier @Inject constructor(
    @ApplicationContext private val context: Context,
) {
    private val nm: NotificationManager =
        context.getSystemService(NotificationManager::class.java)

    /**
     * Post a fresh alert for each [missing] ability and dismiss any
     * previously-posted alert for an ability that has since been
     * re-granted (i.e. present in [available] but not in [missing]).
     */
    fun reconcile(missing: List<BridgeAbility>, available: List<BridgeAbility>) {
        for (ability in missing) post(ability)
        for (ability in available) dismiss(ability)
    }

    private fun post(ability: BridgeAbility) {
        val pi = PendingIntent.getActivity(
            context,
            ability.id.hashCode(),
            ability.settingsIntent(context),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val title = context.getString(R.string.ability_alert_title)
        val body = context.getString(R.string.ability_alert_body, ability.displayName)
        val n = NotificationCompat.Builder(context, PhoneBridgeApp.CHANNEL_ALERTS)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_ERROR)
            .setAutoCancel(true)
            .setContentIntent(pi)
            .build()
        try {
            nm.notify(notifIdFor(ability), n)
        } catch (t: Throwable) {
            // POST_NOTIFICATIONS revoked at runtime → nothing to do.
        }
    }

    private fun dismiss(ability: BridgeAbility) {
        nm.cancel(notifIdFor(ability))
    }

    private fun notifIdFor(ability: BridgeAbility): Int =
        // Stable, unique-per-ability id derived from the ability's
        // identifier string. Hash collisions across abilities are
        // fine because each ability has a unique id.
        (0xA8_0000 or ability.id.hashCode()) and 0x7FFF_FFFF
}
