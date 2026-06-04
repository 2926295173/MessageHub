package im.zyx.phonebridge.notification

import android.app.Notification
import android.os.Build
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.util.Log
import dagger.hilt.android.AndroidEntryPoint
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.NotificationReceivedPayload
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.network.BridgeClient
import java.time.Instant
import java.util.UUID
import javax.inject.Inject
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val TAG = "NotifRelay"

/**
 * Receives every posted system notification and forwards a summary
 * envelope to the daemon over the [BridgeClient] connection.
 *
 * Permissions:
 *   - BIND_NOTIFICATION_LISTENER_SERVICE (system, granted in Settings)
 *
 * We do *not* call [cancelNotification] on dismiss; the daemon is the
 * source of truth and can ask us to dismiss via the reverse channel
 * (out of scope for MVP).
 */
@AndroidEntryPoint
class NotificationRelayService : NotificationListenerService() {

    @Inject lateinit var client: BridgeClient

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    override fun onListenerConnected() {
        super.onListenerConnected()
        Log.d(TAG, "listener connected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        val n = sbn.notification ?: return
        val ex = n.extras
        val title = ex?.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
        val text = ex?.getCharSequence(Notification.EXTRA_TEXT)?.toString() ?: ""
        val big = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            ex?.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString()
        } else null

        val payload = NotificationReceivedPayload(
            notifId = sbn.key,
            packageName = sbn.packageName,
            appLabel = appLabelFor(sbn.packageName),
            title = title,
            text = (big ?: text),
            postedAt = sbn.postTime
        )
        val env = Envelope(
            id = UUID.randomUUID().toString(),
            type = MessageType.NOTIFY_RECEIVED,
            from = "android",
            to = "daemon",
            ts = Instant.ofEpochMilli(sbn.postTime).toString(),
            payload = json.encodeToJsonElement(NotificationReceivedPayload.serializer(), payload)
        )
        scope.launch { client.send(env) }
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification) {
        // We don't act on this; daemon drives dismissal in the future.
    }

    private fun appLabelFor(pkg: String): String = try {
        val pm = packageManager
        val info = pm.getApplicationInfo(pkg, 0)
        pm.getApplicationLabel(info).toString()
    } catch (_: Throwable) {
        pkg
    }

    override fun onDestroy() {
        scope.launch { /* nothing to flush */ }
        super.onDestroy()
    }
}
