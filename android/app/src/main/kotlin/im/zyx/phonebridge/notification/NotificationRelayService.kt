package im.zyx.phonebridge.notification

import android.app.Notification
import android.os.Build
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.util.Log
import dagger.hilt.android.AndroidEntryPoint
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.NotificationDismissedPayload
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
 * Two-way bridge for system notifications.
 *
 * **Android → daemon** (forward):
 *   - [onNotificationPosted] packs the active notification's
 *     metadata and sends a `notification.received` envelope.
 *
 * **Daemon → Android** (reverse):
 *   - The service also subscribes to incoming envelopes from the
 *     daemon. A `notification.dismissed` envelope (triggered by
 *     the user clicking "Dismiss" in the web console) calls
 *     [cancelNotification] with the `sbn.key` so the system
 *     removes the notification from the shade.
 *
 * Permissions:
 *   - BIND_NOTIFICATION_LISTENER_SERVICE (system, granted in Settings)
 */
@AndroidEntryPoint
class NotificationRelayService : NotificationListenerService() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var pairing: PairingMachine

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private var collectJob: kotlinx.coroutines.Job? = null

    override fun onListenerConnected() {
        super.onListenerConnected()
        Log.d(TAG, "listener connected; subscribing to incoming envelopes")
        collectJob = scope.launch {
            client.incoming.collect { env -> handleIncoming(env) }
        }
    }

    override fun onListenerDisconnected() {
        super.onListenerDisconnected()
        Log.d(TAG, "listener disconnected; cancelling subscribe")
        collectJob?.cancel()
        collectJob = null
    }

    private fun handleIncoming(env: Envelope) {
        if (env.type != MessageType.NOTIFY_DISMISSED) return
        val payload = runCatching {
            json.decodeFromJsonElement(NotificationDismissedPayload.serializer(), env.payload)
        }.onFailure { Log.w(TAG, "bad notification.dismissed payload: $it") }
            .getOrNull() ?: return
        try {
            Log.i(TAG, "dismissing notification id=${payload.id}")
            cancelNotification(payload.id)
        } catch (t: Throwable) {
            Log.w(TAG, "cancelNotification(${payload.id}) failed: $t")
        }
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
        // We don't act on local removals (e.g. user swipes the
        // notification on the device); dismissal is driven by the
        // desktop's `notification.dismissed` envelope (handled in
        // handleIncoming above).
    }

    private fun appLabelFor(pkg: String): String = try {
        val pm = packageManager
        val info = pm.getApplicationInfo(pkg, 0)
        pm.getApplicationLabel(info).toString()
    } catch (_: Throwable) {
        pkg
    }
}
