package im.zyx.phonebridge.keepalive

import android.content.Context
import android.content.Intent

/**
 * A self-checkable "ability" the Android app needs the system to grant.
 *
 * The bridge has a small, fixed set of these: notification-listener
 * access, optional AccessibilityService, floating-window permission,
 * battery-optimization exemption, etc. Each implementation knows how
 * to (a) answer "am I currently usable?" in O(constant) time and
 * (b) build the Intent the user should land on to re-enable it.
 *
 * Implementations MUST be safe to call from any thread and MUST NOT
 * perform blocking IO; the [SelfCheckWorker] will invoke them on its
 * worker thread but they may also be poked from the UI thread for
 * "is this granted right now?" prompts.
 */
interface BridgeAbility {
    /** Stable identifier (used in logs and DataStore keys). */
    val id: String

    /** Human-readable name; surfaced in the heads-up alert. */
    val displayName: String

    /** True if the system currently grants this ability to our package. */
    fun isAvailable(context: Context): Boolean

    /** Intent that deep-links to the relevant system settings page. */
    fun settingsIntent(context: Context): Intent
}
