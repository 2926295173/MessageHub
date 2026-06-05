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
 *
 * The actual SMS parsing is done by the Android Telephony stack
 * (`Telephony.Sms.Intents.getMessagesFromIntent`); this receiver
 * just groups multipart SMS, builds the envelope, and dispatches
 * via the [BridgeClient]. The pure envelope-building logic lives
 * in [buildSmsReceivedEnvelope] for unit testing.
 */
@AndroidEntryPoint
class SmsReceiver : BroadcastReceiver() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var pairing: PairingMachine

    /**
     * Test-only injection helper. Lets instrumented tests construct
     * a [SmsReceiver] and inject a [BridgeClient] / [PairingMachine]
     * directly, without going through the Hilt test graph (which
     * can't easily replace `@Inject constructor()` bindings from
     * the main source set).
     */
    fun injectForTest(client: BridgeClient, pairing: PairingMachine) {
        this.client = client
        this.pairing = pairing
    }

    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val messages = Telephony.Sms.Intents.getMessagesFromIntent(intent) ?: return
        val env = buildSmsReceivedEnvelope(
            parts = messages.toList(),
            ourDeviceId = pairing.ourDeviceId(),
        ) ?: return
        scope.launch { client.send(env) }
        Log.d(TAG, "forwarded SMS from ${env.payload}")
    }

    companion object {
        /**
         * Pure function: take the parsed [Telephony.SmsMessage]
         * parts (a multipart SMS arrives as multiple parts; we
         * concatenate bodies + use the first part's address) and
         * build the [Envelope] to send to the daemon.
         *
         * Returns null if [parts] is empty (after dropping any
         * nulls that the framework may have inserted).
         */
        fun buildSmsReceivedEnvelope(
            parts: List<android.telephony.SmsMessage?>,
            ourDeviceId: String,
        ): Envelope? {
            val real = parts.filterNotNull()
            if (real.isEmpty()) return null
            val first = real.first()
            val sender = first.displayOriginatingAddress ?: return null
            val body = real.joinToString(separator = "") { it.displayMessageBody.orEmpty() }
            val receivedAt = first.timestampMillis
            val payload = SmsReceivedPayload(
                id = UUID.randomUUID().toString(),
                address = sender,
                body = body,
                received_at = receivedAt,
                sim_slot = null,
                subscription_id = null
            )
            return Envelope(
                v = 1,
                id = UUID.randomUUID().toString(),
                ts = Instant.ofEpochMilli(receivedAt).toEpochMilli(),
                type = MessageType.SMS_RECEIVED,
                device_id = ourDeviceId,
                payload = json.encodeToJsonElement(SmsReceivedPayload.serializer(), payload)
            )
        }
    }
}
