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
        // The user swiped the notification on the device (or the
        // system dismissed it). Mirror this to the daemon so the
        // web console sees the same state. Reasons the system might
        // call this without a user action:
        //   - APP_CANCEL: the app that posted the notification
        //     called NotificationManager.cancel
        //   - APP_CANCEL_ALL: same but for all of the app's notifs
        //   - USER: the user swiped it
        //   - PACKAGE_REMOVED, PACKAGE_BANNED, etc.
        // All of these warrant telling the daemon. The daemon's
        // `notification.dismissed` handler marks the row read; for
        // APP_CANCEL_* we don't want to send a notification.dismissed
        // (the row was never delivered to begin with), but the
        // `mark_notification_read_by_sbn_key` SQL is idempotent and
        // cheap, so it's fine to send unconditionally.
        val payload = NotificationDismissedPayload(id = sbn.key)
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = java.time.Instant.now().toEpochMilli(),
            type = MessageType.NOTIFY_DISMISSED,
            device_id = pairing.ourDeviceId(),
            payload = json.encodeToJsonElement(NotificationDismissedPayload.serializer(), payload)
        )
        scope.launch { client.send(env) }
    }

    private fun appLabelFor(pkg: String): String = try {
        val pm = packageManager
        val info = pm.getApplicationInfo(pkg, 0)
        pm.getApplicationLabel(info).toString()
    } catch (_: Throwable) {
        pkg
    }
}
