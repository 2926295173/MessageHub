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
import im.zyx.phonebridge.pairing.PairingMachine
import java.time.Instant
import java.util.UUID
import javax.inject.Inject
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val TAG = "NotifRelay"

/**
 * Receives every posted system notification and forwards a
 * `notification.received` envelope to the daemon over the
 * [BridgeClient] connection. Wire shape matches the Rust
 * `NotificationReceived` struct in `phonebridge-proto/src/payload.rs`.
 *
 * Permissions:
 *   - BIND_NOTIFICATION_LISTENER_SERVICE (system, granted in Settings)
 */
@AndroidEntryPoint
class NotificationRelayService : NotificationListenerService() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var pairing: PairingMachine

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    override fun onListenerConnected() {
        super.onListenerConnected()
        Log.d(TAG, "listener connected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        val n = sbn.notification ?: return
        val ex = n.extras
        val title = ex?.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty()
        val text = ex?.getCharSequence(Notification.EXTRA_TEXT)?.toString().orEmpty()
        val big = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            ex?.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString()
        } else null

        val payload = NotificationReceivedPayload(
            id = sbn.key,
            package_name = sbn.packageName,
            app_name = appLabelFor(sbn.packageName),
            title = title,
            content = big ?: text,
            posted_at = sbn.postTime,
            is_sensitive = false,
            category = n.category
        )
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.ofEpochMilli(sbn.postTime).toEpochMilli(),
            type = MessageType.NOTIFY_RECEIVED,
            device_id = pairing.ourDeviceId(),
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
        super.onDestroy()
    }
}
