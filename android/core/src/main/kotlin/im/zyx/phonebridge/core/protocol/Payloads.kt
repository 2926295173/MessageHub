package im.zyx.phonebridge.core.protocol

import kotlinx.serialization.Serializable

// ============================================================================
// device lifecycle
// ============================================================================

@Serializable
data class DeviceHelloPayload(
    val name: String,
    val device_type: DeviceType,
    val protocol_version: Int,
    val pubkey: String,
    val port: Int? = null,
    val manufacturer: String? = null,
    val model: String? = null
)

@Serializable
data class DeviceHeartbeatPayload(
    val rtt_ms: Int? = null
)

@Serializable
data class DeviceInfoUpdatePayload(
    val battery_level: Int? = null,
    val is_charging: Boolean? = null,
    val network_type: NetworkType? = null,
    val android_version: String? = null,
    val app_version: String? = null
)

// ============================================================================
// pairing
// ============================================================================

@Serializable
data class PairRequestPayload(
    val ephemeral_pubkey: String
)

@Serializable
data class PairChallengePayload(
    val ephemeral_pubkey: String,
    val code: String,
    val expires_at: Long
)

@Serializable
data class PairConfirmPayload(
    val accepted: Boolean
)

@Serializable
class PairAcceptPayload

@Serializable
data class PairRejectPayload(
    val reason: String? = null
)

@Serializable
data class PairCompletePayload(
    val cert_pem: String,
    val cert_fingerprint: String
)

@Serializable
data class UnpairPayload(
    val reason: String? = null
)

// ============================================================================
// notifications
// ============================================================================

@Serializable
data class NotificationReceivedPayload(
    val id: String,
    val package_name: String,
    val app_name: String? = null,
    val title: String,
    val content: String,
    val posted_at: Long,
    val is_sensitive: Boolean = false,
    val category: String? = null
)

@Serializable
data class NotificationDismissedPayload(
    val id: String
)

// ============================================================================
// sms
// ============================================================================

@Serializable
data class SmsReceivedPayload(
    val id: String,
    val address: String,
    val body: String,
    val received_at: Long,
    val sim_slot: Int? = null,
    val subscription_id: Int? = null
)

@Serializable
data class SmsSendRequestPayload(
    val to: String,
    val body: String,
    val subscription_id: Int? = null
)

@Serializable
data class SmsSendResultPayload(
    val request_id: String,
    val ok: Boolean,
    val error_code: String? = null,
    val error_message: String? = null
)

@Serializable
data class SmsListRequestPayload(
    val limit: Int? = null,
    val before: Long? = null
)

@Serializable
data class SmsListResultPayload(
    val messages: List<SmsReceivedPayload>
)

// ============================================================================
// calls
// ============================================================================

@Serializable
data class CallStatePayload(
    val state: CallStateKind,
    val phone_number: String? = null,
    val call_id: String? = null,
    val contact_name: String? = null,
    val sim_slot: Int? = null
)

@Serializable
data class CallIncomingPayload(
    val phone_number: String,
    val contact_name: String? = null,
    val sim_slot: Int? = null
)

@Serializable
class CallAnswerRequestPayload

@Serializable
class CallEndRequestPayload

@Serializable
data class CallDialRequestPayload(
    val number: String
)

@Serializable
data class CallHistoryEntryPayload(
    val phone_number: String,
    val contact_name: String? = null,
    val started_at: Long,
    val duration_seconds: Int? = null,
    val direction: CallDirection,
    val sim_slot: Int? = null
)

@Serializable
data class CallHistoryPayload(
    val entries: List<CallHistoryEntryPayload>
)
