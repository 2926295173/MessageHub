package im.zyx.phonebridge.telephony

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.telecom.TelecomManager
import android.telephony.PhoneStateListener
import android.telephony.SmsManager
import android.telephony.TelephonyCallback
import android.telephony.TelephonyManager
import android.util.Log
import androidx.core.content.ContextCompat
import dagger.hilt.android.qualifiers.ApplicationContext
import im.zyx.phonebridge.core.protocol.CallStateKind
import im.zyx.phonebridge.core.protocol.CallStatePayload
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.pairing.PairingMachine
import java.time.Instant
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val TAG = "CallController"

/**
 * Bridges the desktop's call + SMS commands to the Android telephony
 * subsystem and reports call state changes to the message-center.
 *
 * Permissions: READ_PHONE_STATE, CALL_PHONE, ANSWER_PHONE_CALLS.
 */
@Singleton
class CallController @Inject constructor(
    @ApplicationContext private val context: Context,
    private val client: BridgeClient,
    private val pairing: PairingMachine
) {
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private val telephony: TelephonyManager by lazy {
        context.getSystemService(Context.TELEPHONY_SERVICE) as TelephonyManager
    }
    private val telecom: TelecomManager by lazy {
        context.getSystemService(Context.TELECOM_SERVICE) as TelecomManager
    }

    private val legacyListener = object : PhoneStateListener() {
        @Deprecated("Deprecated in Java")
        override fun onCallStateChanged(state: Int, phoneNumber: String?) {
            sendState(state, phoneNumber)
        }
    }

    fun start() {
        if (!hasPermission(Manifest.permission.READ_PHONE_STATE)) {
            Log.w(TAG, "READ_PHONE_STATE not granted; call monitoring disabled")
            return
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                telephony.registerTelephonyCallback(
                    context.mainExecutor,
                    object : TelephonyCallback(), TelephonyCallback.CallStateListener {
                        override fun onCallStateChanged(state: Int) {
                            sendState(state, null)
                        }
                    }
                )
            } else {
                @Suppress("DEPRECATION")
                telephony.listen(legacyListener, PhoneStateListener.LISTEN_CALL_STATE)
            }
        } catch (t: Throwable) {
            Log.w(TAG, "register telephony callback failed: $t")
        }
    }

    fun stop() {
        try {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
                @Suppress("DEPRECATION")
                telephony.listen(legacyListener, PhoneStateListener.LISTEN_NONE)
            }
        } catch (_: Throwable) {}
    }

    @SuppressLint("MissingPermission")
    fun answer() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            telecom.acceptRingingCall()
        } else {
            Log.w(TAG, "answer not supported on API ${Build.VERSION.SDK_INT}")
        }
    }

    @SuppressLint("MissingPermission")
    fun end() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            Log.w(TAG, "endCall requires API 28+; current is ${Build.VERSION.SDK_INT}")
            return
        }
        if (telecom.endCall()) {
            Log.d(TAG, "endCall returned true")
        } else {
            Log.w(TAG, "endCall returned false")
        }
    }

    fun dial(number: String) {
        if (!hasPermission(Manifest.permission.CALL_PHONE)) {
            Log.w(TAG, "CALL_PHONE not granted; cannot dial")
            return
        }
        val i = android.content.Intent(android.content.Intent.ACTION_CALL, Uri.parse("tel:$number"))
        i.flags = android.content.Intent.FLAG_ACTIVITY_NEW_TASK
        try {
            context.startActivity(i)
        } catch (t: Throwable) {
            Log.w(TAG, "dial failed: $t")
        }
    }

    fun sendSms(address: String, body: String) {
        try {
            val sm = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                context.getSystemService(SmsManager::class.java)
            } else {
                @Suppress("DEPRECATION")
                SmsManager.getDefault()
            }
            sm.sendTextMessage(address, null, body, null, null)
            Log.d(TAG, "sms sent to $address (${body.length} chars)")
        } catch (t: Throwable) {
            Log.w(TAG, "sms send failed: $t")
        }
    }

    private fun sendState(state: Int, phoneNumber: String?) {
        val s = when (state) {
            TelephonyManager.CALL_STATE_RINGING -> CallStateKind.Ringing
            TelephonyManager.CALL_STATE_OFFHOOK -> CallStateKind.Offhook
            TelephonyManager.CALL_STATE_IDLE -> CallStateKind.Idle
            else -> CallStateKind.Idle
        }
        val payload = CallStatePayload(
            state = s,
            phone_number = phoneNumber,
            call_id = null,
            contact_name = null,
            sim_slot = null
        )
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.CALL_STATE,
            device_id = pairing.ourDeviceId(),
            payload = json.encodeToJsonElement(CallStatePayload.serializer(), payload)
        )
        scope.launch { client.send(env) }
    }

    private fun hasPermission(p: String): Boolean =
        ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED
}
