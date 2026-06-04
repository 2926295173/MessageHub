package im.zyx.phonebridge.pairing

import android.util.Log
import im.zyx.phonebridge.core.protocol.DeviceInfo
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.PairChallengePayload
import im.zyx.phonebridge.core.protocol.PairConfirmPayload
import im.zyx.phonebridge.core.protocol.PairRequestPayload
import im.zyx.phonebridge.core.protocol.PairResultPayload
import im.zyx.phonebridge.core.protocol.json
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

private const val TAG = "Pairing"

/**
 * The Android side of the pairing protocol. Mirrors the Rust
 * `pairing::Initiator` in crates/phonebridge-net/src/pairing.rs.
 *
 * Flow:
 * 1. Android generates a 6-digit code and sends `device.pair.request`
 *    (the code travels in the request body).
 * 2. Desktop shows the code; when the user types it on the desktop the
 *    desktop sends back `device.pair.challenge` echoing the code.
 * 3. Android confirms by sending `device.pair.confirm`.
 * 4. Desktop answers with `device.pair.result` (accepted: bool).
 *
 * The Android UI:
 * - shows the 6-digit code for the user to type on the desktop
 * - shows "Confirm on desktop" while waiting for the user to accept
 */
sealed interface PairingState {
    object Idle : PairingState
    data class AwaitingDesktop(val code: String) : PairingState
    data class ChallengeReceived(val code: String) : PairingState
    data class Paired(val deviceId: String) : PairingState
    data class Failed(val reason: String) : PairingState
}

class PairingMachine {
    private val _state = MutableStateFlow<PairingState>(PairingState.Idle)
    val state: StateFlow<PairingState> = _state

    fun begin(
        ourDeviceId: String,
        desktopDeviceId: String,
        code: String,
        desktopInfo: DeviceInfo
    ): Envelope {
        require(code.length == 6 && code.all { it.isDigit() }) { "pairing code must be 6 digits" }
        _state.value = PairingState.AwaitingDesktop(code)
        val payload = json.encodeToJsonElement(
            PairRequestPayload.serializer(),
            PairRequestPayload(desktop = desktopInfo, code = code)
        )
        return Envelope(
            id = newEnvelopeId(),
            type = MessageType.DEVICE_PAIR_REQUEST,
            from = ourDeviceId,
            to = desktopDeviceId,
            ts = nowIso(),
            payload = payload
        )
    }

    /**
     * React to desktop's pair.challenge. Returns the confirm envelope
     * to send back, or null if the state machine ignored the message.
     */
    fun onChallenge(envelope: Envelope): Envelope? {
        val payload = runCatching {
            json.decodeFromJsonElement(PairChallengePayload.serializer(), envelope.payload)
        }.onFailure { Log.w(TAG, "bad challenge payload: $it") }.getOrNull() ?: return null
        val current = _state.value
        if (current !is PairingState.AwaitingDesktop) return null
        return if (payload.code == current.code) {
            _state.value = PairingState.ChallengeReceived(current.code)
            Envelope(
                id = newEnvelopeId(),
                type = MessageType.DEVICE_PAIR_CONFIRM,
                from = envelope.to,
                to = envelope.from,
                ts = nowIso(),
                payload = json.encodeToJsonElement(
                    PairConfirmPayload.serializer(),
                    PairConfirmPayload(code = current.code)
                )
            )
        } else {
            _state.value = PairingState.Failed("code mismatch")
            null
        }
    }

    fun onResult(envelope: Envelope, ourDeviceId: String) {
        val payload = runCatching {
            json.decodeFromJsonElement(PairResultPayload.serializer(), envelope.payload)
        }.getOrNull() ?: return
        _state.value = if (payload.accepted) {
            PairingState.Paired(ourDeviceId)
        } else {
            PairingState.Failed(payload.reason ?: "rejected")
        }
    }

    fun reset() {
        _state.value = PairingState.Idle
    }

    companion object {
        fun generateCode(): String = (0 until 6)
            .map { kotlin.random.Random.nextInt(0, 10) }
            .joinToString("")

        private fun newEnvelopeId(): String = java.util.UUID.randomUUID().toString()
        private fun nowIso(): String = java.time.Instant.now().toString()
    }
}
