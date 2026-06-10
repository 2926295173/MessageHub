package im.zyx.phonebridge.pairing

import android.util.Log
import im.zyx.phonebridge.core.crypto.CertGen
import im.zyx.phonebridge.core.crypto.Ecdh
import im.zyx.phonebridge.core.crypto.PairingCode
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.PairChallengePayload
import im.zyx.phonebridge.core.protocol.PairCompletePayload
import im.zyx.phonebridge.core.protocol.PairConfirmPayload
import im.zyx.phonebridge.core.protocol.PairRequestPayload
import im.zyx.phonebridge.core.protocol.PairRejectPayload
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.data.IdentityStore
import im.zyx.phonebridge.data.IdentityWithKey
import java.security.KeyPair
import java.security.interfaces.ECPrivateKey
import java.security.interfaces.ECPublicKey
import java.time.Instant
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

private const val TAG = "Pairing"

/**
 * The Android side of the pairing protocol. Mirrors the Rust
 * `pairing::Responder` in `crates/phonebridge-net/src/pairing.rs`.
 *
 * The long-term identity (keypair + self-signed X.509 cert) and
 * the stable device id are persisted in [PrefsRepository] so they
 * survive process restarts — the message-center sees the same `device_id`
 * and the same `pubkey` in `device.hello` on every reconnect.
 *
 * Flow:
 *  1. Desktop sends `device.pair.request` with its ephemeral pubkey.
 *  2. We generate an ephemeral keypair, compute ECDH, derive the
 *     4-digit code via HKDF-SHA256, and reply with
 *     `device.pair.challenge` carrying our ephemeral pubkey + the
 *     code + an expiry.
 *  3. The user visually verifies the code on Android, types it on
 *     the desktop, and the desktop sends `device.pair.accept`.
 *  4. We send `device.pair.confirm(accepted=true)`.
 *  5. The desktop sends `device.pair.complete` with its long-term
 *     cert. We verify the PEM/fingerprint, then send our own
 *     `device.pair.complete` with our long-term cert.
 */
sealed interface PairingState {
    object Idle : PairingState
    data class ShowingCode(
        val code: String,
        val expiresAtMs: Long,
        val peerDeviceId: String,
        val peerEphemeralPubB64: String
    ) : PairingState
    data class Confirming(
        val code: String,
        val peerDeviceId: String
    ) : PairingState
    data class Paired(
        val ourDeviceId: String,
        val peerDeviceId: String,
        val peerFingerprint: String
    ) : PairingState
    data class Failed(val reason: String) : PairingState
    data class Initiating(val startedAtMs: Long) : PairingState
}

@Singleton
class PairingMachine @Inject constructor(
    private val identityStore: IdentityStore
) {
    private val _state = MutableStateFlow<PairingState>(PairingState.Idle)
    val state: StateFlow<PairingState> = _state

    // Per-session: ephemeral keypair (fresh per pairing).
    private var ephemeral: KeyPair? = null

    // Long-term identity, loaded from IdentityStore on first use
    // (and persisted after the first generation so future restarts
    // see the same identity).
    private var longTermKp: KeyPair? = null
    private var longTermPem: String? = null
    private var longTermFingerprintStr: String? = null

    /** Lazily load (or generate) the persistent long-term identity. */
    @Synchronized
    fun ensureIdentity(commonName: String = "phonebridge-android") {
        if (longTermKp != null) return
        val id: IdentityWithKey = identityStore.getOrCreateIdentityBlocking(commonName)
        longTermKp = id.keyPair
        longTermPem = id.pem
        longTermFingerprintStr = id.fingerprint
        Log.i(TAG, "identity ready; fp=${id.fingerprint}")
    }

    val longTermFingerprint: String?
        get() {
            ensureIdentity()
            return longTermFingerprintStr
        }

    /**
     * Base64 (URL-safe, no padding) of our long-term ECDH P-256
     * public key in uncompressed form. Sent in `device.hello`.
     */
    val longTermPublicBase64: String
        get() {
            ensureIdentity()
            val pub = (longTermKp?.public as? ECPublicKey) ?: return "stub"
            return Ecdh.toBase64(pub)
        }

    /**
     * Stable device id (UUIDv4). Persisted in [IdentityStore] so
     * it survives process restarts.
     */
    fun ourDeviceId(): String = identityStore.getOrCreateDeviceIdBlocking()

    fun onRequest(envelope: Envelope): Envelope? {
        ensureIdentity()
        val req = runCatching {
            json.decodeFromJsonElement(PairRequestPayload.serializer(), envelope.payload)
        }.onFailure { Log.w(TAG, "bad pair.request payload: $it") }
            .getOrNull() ?: run {
            _state.value = PairingState.Failed("bad pair.request payload")
            return null
        }
        val peerPub = try {
            Ecdh.publicKeyFromBase64(req.ephemeral_pubkey)
        } catch (t: Throwable) {
            _state.value = PairingState.Failed("bad peer pubkey: ${t.message}")
            return null
        }
        val myKp = Ecdh.generateKeyPair()
        ephemeral = myKp
        val shared = try {
            Ecdh.agree(myKp.private as ECPrivateKey, peerPub)
        } catch (t: Throwable) {
            _state.value = PairingState.Failed("ECDH failed: ${t.message}")
            return null
        }
        val code = PairingCode.derive(shared)
        val expiresAt = Instant.now().toEpochMilli() + 30_000L
        val myPubB64 = Ecdh.toBase64(myKp.public as ECPublicKey)
        _state.value = PairingState.ShowingCode(
            code = code,
            expiresAtMs = expiresAt,
            peerDeviceId = envelope.device_id,
            peerEphemeralPubB64 = req.ephemeral_pubkey
        )
        Log.i(TAG, "pair.request received; code=$code; expires_at=$expiresAt")
        val payload = PairChallengePayload(
            ephemeral_pubkey = myPubB64,
            code = code,
            expires_at = expiresAt
        )
        return Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_PAIR_CHALLENGE,
            device_id = ourDeviceId(),
            payload = json.encodeToJsonElement(PairChallengePayload.serializer(), payload)
        )
    }

    /**
     * User explicitly accepted the pairing on the phone. Build a
     * `device.pair.confirm(accepted=true)` envelope to send — does NOT
     * wait for an incoming `device.pair.accept` from the desktop. The
     * phone is the trusted UI surface (per the project's threat
     * model: the desktop may be compromised, the phone is always
     * safe), so the user's click here is the canonical confirmation.
     *
     * Returns null if the state machine is not in [PairingState.ShowingCode]
     * (e.g. the code already expired or the user accepted too late).
     */
    fun userAccepts(): Envelope? {
        val current = _state.value
        if (current !is PairingState.ShowingCode) {
            Log.w(TAG, "userAccepts in wrong state: $current")
            return null
        }
        if (Instant.now().toEpochMilli() > current.expiresAtMs) {
            _state.value = PairingState.Failed("code expired")
            Log.w(TAG, "userAccepts after expiry; transitioned to Failed")
            return null
        }
        _state.value = PairingState.Confirming(
            code = current.code,
            peerDeviceId = current.peerDeviceId,
        )
        Log.i(TAG, "user accepts pairing; sending confirm(true)")
        return makeConfirm(current.peerDeviceId, ourDeviceId(), accepted = true)
    }

    /**
     * User explicitly rejected the pairing on the phone. Sends a
     * `device.pair.confirm(accepted=false)` and transitions to
     * [PairingState.Failed]. Symmetric with [userAccepts].
     */
    fun userRejects(reason: String = "rejected by user"): Envelope? {
        val current = _state.value
        if (current !is PairingState.ShowingCode) {
            Log.w(TAG, "userRejects in wrong state: $current")
            return null
        }
        val env = makeConfirm(current.peerDeviceId, ourDeviceId(), accepted = false)
        _state.value = PairingState.Failed(reason)
        Log.i(TAG, "user rejects pairing: $reason")
        return env
    }

    fun onAccept(envelope: Envelope, ourDeviceId: String): Envelope {
        val current = _state.value
        if (current !is PairingState.ShowingCode) {
            Log.w(TAG, "pair.accept in wrong state: $current")
            return makeConfirm(envelope.device_id, ourDeviceId, accepted = false)
        }
        if (Instant.now().toEpochMilli() > current.expiresAtMs) {
            _state.value = PairingState.Failed("code expired")
            return makeConfirm(envelope.device_id, ourDeviceId, accepted = false)
        }
        _state.value = PairingState.Confirming(
            code = current.code,
            peerDeviceId = envelope.device_id
        )
        Log.i(TAG, "pair.accept received; sending confirm(true)")
        return makeConfirm(envelope.device_id, ourDeviceId, accepted = true)
    }

    fun onReject(envelope: Envelope) {
        val payload = runCatching {
            json.decodeFromJsonElement(PairRejectPayload.serializer(), envelope.payload)
        }.getOrNull()
        val reason = payload?.reason ?: "rejected by peer"
        _state.value = PairingState.Failed(reason)
        Log.w(TAG, "pair.reject received: $reason")
    }

    fun onComplete(envelope: Envelope, ourDeviceId: String): Envelope? {
        val payload = runCatching {
            json.decodeFromJsonElement(PairCompletePayload.serializer(), envelope.payload)
        }.onFailure {
            _state.value = PairingState.Failed("bad pair.complete: ${it.message}")
        }.getOrNull() ?: return null

        if (!payload.cert_pem.contains("BEGIN CERTIFICATE")) {
            _state.value = PairingState.Failed("peer cert missing BEGIN CERTIFICATE")
            return null
        }
        if (!isValidFingerprint(payload.cert_fingerprint)) {
            _state.value = PairingState.Failed("peer fingerprint malformed")
            return null
        }

        val prevState = _state.value
        val peerId = when (prevState) {
            is PairingState.Confirming -> prevState.peerDeviceId
            is PairingState.ShowingCode -> prevState.peerDeviceId
            else -> envelope.device_id
        }
        _state.value = PairingState.Paired(
            ourDeviceId = ourDeviceId,
            peerDeviceId = peerId,
            peerFingerprint = payload.cert_fingerprint
        )
        Log.i(TAG, "paired with $peerId; peer fp=${payload.cert_fingerprint}")

        val pem = longTermPem ?: run {
            Log.e(TAG, "no long-term cert; ensureIdentity() was not called")
            return null
        }
        val fp = longTermFingerprintStr ?: return null
        val replyPayload = PairCompletePayload(
            cert_pem = pem,
            cert_fingerprint = fp
        )
        return Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_PAIR_COMPLETE,
            device_id = ourDeviceId,
            payload = json.encodeToJsonElement(PairCompletePayload.serializer(), replyPayload)
        )
    }

    fun reset() {
        _state.value = PairingState.Idle
        ephemeral = null
        // Long-term identity is persistent; do not reset.
    }

    // ========================================================================
    // Initiator (phone-initiated) side
    // ========================================================================

    /**
     * Phone is initiating pairing with the desktop. Generates an
     * ephemeral keypair, transitions to [PairingState.Initiating], and
     * returns the `device.pair.request` envelope to send.
     *
     * Returns null if we're not in [PairingState.Idle] (i.e. a pairing
     * is already in flight in the other direction).
     */
    fun initiate(): Envelope? {
        if (_state.value !is PairingState.Idle) {
            Log.w(TAG, "initiate while not idle: ${_state.value}")
            return null
        }
        val kp = Ecdh.generateKeyPair()
        val ourPubB64 = Ecdh.toBase64(kp.public as ECPublicKey)
        ephemeral = kp
        // The desktop will use the peer id from the envelope header,
        // so we set device_id to our own id; the WS layer will route
        // it to the right connected device.
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_PAIR_REQUEST,
            device_id = ourDeviceId(),
            payload = json.encodeToJsonElement(
                PairRequestPayload.serializer(),
                PairRequestPayload(ephemeral_pubkey = ourPubB64)
            )
        )
        _state.value = PairingState.Initiating(
            startedAtMs = Instant.now().toEpochMilli(),
        )
        Log.i(TAG, "initiate: sending pair.request")
        return env
    }

    /**
     * Handle `device.pair.confirm` from the desktop. If accepted,
     * build a `device.pair.complete` carrying our long-term cert and
     * transition to [PairingState.Paired]. If rejected, transition to
     * [PairingState.Failed]. Symmetric to the responder's [onAccept]
     * but skips the code-typed-on-the-desktop step.
     */
    fun onInitiatorConfirm(envelope: Envelope, ourDeviceId: String): Envelope? {
        if (_state.value !is PairingState.Initiating) {
            Log.w(TAG, "onInitiatorConfirm in wrong state: ${_state.value}")
            return null
        }
        val confirm: PairConfirmPayload = runCatching {
            json.decodeFromJsonElement(PairConfirmPayload.serializer(), envelope.payload)
        }.onFailure { Log.w(TAG, "bad pair.confirm payload: $it") }
            .getOrNull() ?: return null

        if (!confirm.accepted) {
            _state.value = PairingState.Failed("desktop rejected pairing")
            Log.w(TAG, "desktop rejected pairing")
            return null
        }

        // Accepted: build our pair.complete.
        val pem = longTermPem ?: run {
            Log.e(TAG, "no long-term cert; ensureIdentity() was not called")
            return null
        }
        val fp = longTermFingerprintStr ?: return null
        _state.value = PairingState.Confirming(
            code = "(phone-initiated, no code)",
            peerDeviceId = envelope.device_id,
        )
        Log.i(TAG, "desktop accepted; sending pair.complete (fp=$fp)")
        return Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_PAIR_COMPLETE,
            device_id = ourDeviceId,
            payload = json.encodeToJsonElement(
                PairCompletePayload.serializer(),
                PairCompletePayload(cert_pem = pem, cert_fingerprint = fp)
            )
        )
    }

    private fun makeConfirm(peerDeviceId: String, ourDeviceId: String, accepted: Boolean): Envelope {
        val payload = PairConfirmPayload(accepted = accepted)
        return Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_PAIR_CONFIRM,
            device_id = ourDeviceId,
            payload = json.encodeToJsonElement(PairConfirmPayload.serializer(), payload)
        )
    }

    private fun isValidFingerprint(s: String): Boolean {
        if (s.length != 32 * 3 - 1) return false
        val parts = s.split(':')
        if (parts.size != 32) return false
        return parts.all { it.length == 2 && it.all { c -> c in '0'..'9' || c in 'A'..'F' } }
    }
}
