package im.zyx.phonebridge.keepalive

import android.content.Context
import android.content.Intent
import android.provider.Settings
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Placeholder [BridgeAbility] for a future AccessibilityService.
 *
 * The current MVP does not need an AccessibilityService — the
 * notification listener + [android.telephony.TelephonyCallback] cover
 * all required functionality. This class exists so [SelfCheckWorker]
 * can iterate over a stable, well-defined set of abilities and so the
 * future implementer does not need to touch the scheduler or the
 * alerting logic: just swap the body of [isAvailable] for a real
 * `Settings.Secure.getString("enabled_accessibility_services")` query
 * (and the matching `accessibility_service_config.xml` on the
 * service declaration).
 */
@Singleton
class AccessibilityServiceAbility @Inject constructor() : BridgeAbility {
    override val id: String = "accessibility"
    override val displayName: String = "Accessibility service"

    override fun isAvailable(context: Context): Boolean {
        // No AccessibilityService is declared in the manifest yet.
        // When one is added, replace this with:
        //   val flat = Settings.Secure.getString(
        //       context.contentResolver, "enabled_accessibility_services"
        //   ) ?: return false
        //   return flat.split(":").any { it.startsWith(context.packageName + "/") }
        return false
    }

    override fun settingsIntent(context: Context): Intent {
        return Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
}
