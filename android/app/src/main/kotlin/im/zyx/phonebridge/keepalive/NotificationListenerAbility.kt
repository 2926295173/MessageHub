package im.zyx.phonebridge.keepalive

import android.content.Context
import android.content.Intent
import android.provider.Settings
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Self-check for the [im.zyx.phonebridge.notification.NotificationRelayService]
 * notification-listener binding.
 *
 * The system stores the list of granted notification listeners in
 * `Settings.Secure.enabled_notification_listeners` as a colon-separated
 * list of `pkg/ComponentName` strings. There is no broadcast fired when
 * the user toggles this off — the only reliable way to detect the
 * change is to poll, which is what [SelfCheckWorker] does.
 */
@Singleton
class NotificationListenerAbility @Inject constructor() : BridgeAbility {
    override val id: String = "notification_listener"
    override val displayName: String = "Notification access"

    override fun isAvailable(context: Context): Boolean {
        val flat = Settings.Secure.getString(
            context.contentResolver,
            SETTING_KEY,
        ) ?: return false
        return isGranted(flat, context.packageName)
    }

    override fun settingsIntent(context: Context): Intent {
        return Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    companion object {
        /** `Settings.Secure` key holding the colon-separated grant list. */
        const val SETTING_KEY: String = "enabled_notification_listeners"

        /**
         * Pure parser: given the raw value of [SETTING_KEY] and the
         * current package name, decide whether the system has
         * granted notification access to us. Extracted so the unit
         * test can exercise it without needing a real Context.
         */
        fun isGranted(flatValue: String?, packageName: String): Boolean {
            if (flatValue.isNullOrEmpty()) return false
            val prefix = "$packageName/"
            return flatValue.split(":").any { it.startsWith(prefix) }
        }
    }
}
