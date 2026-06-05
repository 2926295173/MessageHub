package im.zyx.phonebridge.pairing

import android.util.Log
import im.zyx.phonebridge.core.crypto.CertGen
import im.zyx.phonebridge.core.crypto.Ecdh
import im.zyx.phonebridge.core.crypto.PairingCode
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.PairAcceptPayload
import im.zyx.phonebridge.core.protocol.PairChallengePayload
import im.zyx.phonebridge.core.protocol.PairCompletePayload
import im.zyx.phonebridge.core.protocol.PairConfirmPayload
import im.zyx.phonebridge.core.protocol.PairRejectPayload
import im.zyx.phonebridge.core.protocol.PairRequestPayload
import im.zyx.phonebridge.core.protocol.json
import java.security.KeyPair
import java.security.interfaces.ECPrivateKey
import java.security.interfaces.ECPublicKey
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.json.Json

private const val TAG = "Pairing"

/**
 * The Android side of the pairing protocol. Mirrors the Rust
 * `pairing::Responder` in `crates/phonebridge-net/src/pairing.rs`.
 *
 * Flow:
 *  1. Desktop sends `device.pair.request` with its ephemeral pubkey.
 *  2. We generate an ephemeral keypair, compute ECDH, derive the
 *     6-digit code via HKDF-SHA256, and reply with
 *     `device.pair.challenge` carrying our ephemeral pubkey + the code
 *     + an expiry.
 *  3. The user visually verifies the code on Android, types it on
 *     the desktop, and the desktop sends `device.pair.accept`.
 *  4. We send `device.pair.confirm(accepted=true)`.
 *  5. The desktop sends `device.pair.complete` with its long-term
 *     cert. We verify the PEM/fingerprint, persist it, and send our
 *     own `device.pair.complete` with our long-term cert.
 *
 * After step 5 the state is [PairingState.Paired].
 */
sealed interface PairingState {
    /** No pairing in flight. */
    object Idle : PairingState
    /** Desktop's `pair.request` received; we are computing the code. */
    data class ShowingCode(
        val code: String,
        val expiresAtMs: Long,
        val peerDeviceId: String,
        val peerEphemeralPubB64: String
    ) : PairingState
    /** Desktop's `pair.accept` received; we are confirming. */
    data class Confirming(
        val code: String,
        val peerDeviceId: String
    ) : PairingState
    /** Both sides exchanged `pair.complete`. */
    data class Paired(
        val ourDeviceId: String,
        val peerDeviceId: String,
        val peerFingerprint: String
    ) : PairingState
    /** Pairing failed at any stage. */
    data class Failed(val reason: String) : PairingState
}

class PairingMachine {

    private val _state = MutableStateFlow<PairingState>(PairingState.Idle)
    val state: StateFlow<PairingState> = _state

    // Per-session: ephemeral keypair, long-term identity, peer pubkey.
    private var ephemeral: KeyPair? = null
    private var longTerm: KeyPair? = null
    private var selfSigned: CertGen.SelfSignedCert? = null

    /** Construct the long-term identity once at app start. */
    fun ensureIdentity(commonName: String) {
        if (longTerm == null) {
            val kp = Ecdh.generateKeyPair()
            longTerm = kp
            selfSigned = CertGen.generateSelfSigned(commonName, kp)
            Log.i(TAG, "identity ready; fp=${selfSigned!!.fingerprint}")
        }
    }

    val longTermFingerprint: String? get() = selfSigned?.fingerprint

    /**
     * Base64 (URL-safe, no padding) of our long-term ECDH P-256 public
     * key in uncompressed form. Sent in `device.hello` so the daemon
     * has a stable identity to pin at pairing time.
     *
     * Returns "stub" if the identity has not been generated yet.
     */
    val longTermPublicBase64: String
        get() {
            ensureIdentity("phonebridge-android")
            val pub = (longTerm?.public as? java.security.interfaces.ECPublicKey)
                ?: return "stub"
            return Ecdh.toBase64(pub)
        }

    /**
     * Process the desktop's `device.pair.request`. Returns the
     * `device.pair.challenge` envelope to send back, or null on
     * protocol error (state is set to Failed).
     */
    fun onRequest(envelope: Envelope): Envelope? {
        ensureIdentity("phonebridge-android")
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

        // Generate our ephemeral and compute the shared secret.
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
     * Process the desktop's `device.pair.accept`. Returns the
     * `device.pair.confirm` envelope to send back.
     */
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

    /**
     * Process the desktop's `device.pair.reject`. No reply.
     */
    fun onReject(envelope: Envelope) {
        val payload = runCatching {
            json.decodeFromJsonElement(PairRejectPayload.serializer(), envelope.payload)
        }.getOrNull()
        val reason = payload?.reason ?: "rejected by peer"
        _state.value = PairingState.Failed(reason)
        Log.w(TAG, "pair.reject received: $reason")
    }

    /**
     * Process the desktop's `device.pair.complete`. Validates the cert
     * PEM contains "BEGIN CERTIFICATE" and that the fingerprint is
     * 32 colon-separated UPPERCASE hex pairs. If accepted, returns
     * our own `device.pair.complete` envelope carrying our long-term
     * cert.
     */
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

        // We are paired.
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

        // Reply with our own cert.
        val ourCert = selfSigned ?: run {
            Log.e(TAG, "no long-term cert; ensureIdentity() was not called")
            return null
        }
        val replyPayload = PairCompletePayload(
            cert_pem = ourCert.pem,
            cert_fingerprint = ourCert.fingerprint
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
        // We deliberately KEEP the long-term identity so the same
        // device reconnects to the same daemon. Pairing state can be
        // reset; identity persists.
    }

    fun ourDeviceId(): String =
        SELF_DEVICE_ID

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

    /**
     * 32 colon-separated UPPERCASE hex pairs, e.g. `AB:CD:...:EF` (95 chars).
     */
    private fun isValidFingerprint(s: String): Boolean {
        if (s.length != 32 * 3 - 1) return false
        val parts = s.split(':')
        if (parts.size != 32) return false
        return parts.all { it.length == 2 && it.all { c -> c in '0'..'9' || c in 'A'..'F' } }
    }

    companion object {
        /**
         * Stable per-process device id. We don't persist it across
         * process death in MVP; the daemon's registry keys on whatever
         * id the Android sends in `device.hello`, so a restart is
         * fine (the daemon sees a new id, a new Responder, a new
         * pairing round).
         */
        val SELF_DEVICE_ID: String = UUID.randomUUID().toString()
    }
}
