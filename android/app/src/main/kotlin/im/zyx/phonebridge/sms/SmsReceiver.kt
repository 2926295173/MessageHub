package im.zyx.phonebridge.sms

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.provider.Telephony
import android.util.Log
import dagger.hilt.android.AndroidEntryPoint
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.SmsReceivedPayload
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

private const val TAG = "SmsReceiver"

/**
 * Receives incoming SMS via the system broadcast and forwards a
 * `sms.received` envelope to the daemon. Wire shape matches the
 * Rust `SmsReceived` struct.
 *
 * Permissions: RECEIVE_SMS (granted at runtime).
 */
@AndroidEntryPoint
class SmsReceiver : BroadcastReceiver() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var pairing: PairingMachine

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val messages = Telephony.Sms.Intents.getMessagesFromIntent(intent) ?: return
        val sender = messages.firstOrNull()?.displayOriginatingAddress ?: return
        val body = messages.joinToString(separator = "") { it.displayMessageBody.orEmpty() }
        val receivedAt = messages.firstOrNull()?.timestampMillis ?: System.currentTimeMillis()

        val payload = SmsReceivedPayload(
            id = UUID.randomUUID().toString(),
            address = sender,
            body = body,
            received_at = receivedAt,
            sim_slot = null,
            subscription_id = null
        )
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.ofEpochMilli(receivedAt).toEpochMilli(),
            type = MessageType.SMS_RECEIVED,
            device_id = pairing.ourDeviceId(),
            payload = json.encodeToJsonElement(SmsReceivedPayload.serializer(), payload)
        )
        scope.launch { client.send(env) }
        Log.d(TAG, "forwarded SMS from $sender (${body.length} chars)")
    }
}
