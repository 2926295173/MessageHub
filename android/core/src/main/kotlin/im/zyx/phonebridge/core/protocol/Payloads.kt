package im.zyx.phonebridge.core.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// --- device discovery / pairing ---------------------------------------

@Serializable
data class DeviceInfo(
    val deviceId: String,
    val name: String,
    val model: String,
    val osVersion: String,
    val appVersion: String
)

@Serializable
data class DeviceHelloPayload(
    val device: DeviceInfo,
    val certificate: String
)

@Serializable
data class PairRequestPayload(
    val desktop: DeviceInfo,
    val code: String
)

@Serializable
data class PairChallengePayload(
    val code: String
)

@Serializable
data class PairConfirmPayload(
    val code: String
)

@Serializable
data class PairResultPayload(
    val accepted: Boolean,
    val reason: String? = null
)

@Serializable
data class PairedPayload(
    val device: DeviceInfo
)

@Serializable
data class UnpairPayload(
    val deviceId: String
)

// --- notifications -----------------------------------------------------

@Serializable
data class NotificationReceivedPayload(
    val notifId: String,
    val packageName: String,
    val appLabel: String,
    val title: String,
    val text: String,
    val postedAt: Long
)

@Serializable
data class NotificationDismissedPayload(
    val notifId: String
)

// --- sms ---------------------------------------------------------------

@Serializable
data class SmsReceivedPayload(
    val smsId: String,
    val address: String,
    val body: String,
    val receivedAt: Long
)

@Serializable
data class SmsSendPayload(
    val address: String,
    val body: String
)

@Serializable
data class SmsSendResultPayload(
    val success: Boolean,
    val error: String? = null
)

// --- calls -------------------------------------------------------------

@Serializable
data class CallStatePayload(
    val callId: String,
    val state: String,
    val number: String
)

@Serializable
data class CallIncomingPayload(
    val callId: String,
    val number: String
)

@Serializable
data class CallAnswerPayload(
    val callId: String
)

@Serializable
data class CallEndPayload(
    val callId: String
)

@Serializable
data class CallDialPayload(
    val number: String
)

// --- misc --------------------------------------------------------------

@Serializable
data class BatteryPayload(
    val level: Int,
    val charging: Boolean
)

@Serializable
data class ClipboardSetPayload(
    val text: String
)

@Serializable
data class PingPayload(
    val nonce: String
)

@Serializable
data class PongPayload(
    val nonce: String
)

@Serializable
data class ErrorPayload(
    val code: String,
    val message: String
)
