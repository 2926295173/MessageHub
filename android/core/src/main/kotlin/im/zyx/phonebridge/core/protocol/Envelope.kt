package im.zyx.phonebridge.core.protocol

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * Wire envelope. Mirror of `phonebridge_proto::Envelope` in Rust.
 *
 * Wire shape (JSON):
 * ```
 * {
 *   "v": 1,
 *   "id": "uuid-v4-string",
 *   "ts": 1717000000000,        // unix epoch ms (Long)
 *   "type": "device.pair.request",
 *   "device_id": "uuid-v4-string",
 *   "payload": { ... }
 * }
 * ```
 *
 * There is no `to` field — every envelope is either from a device to
 * the message-center or vice versa; the address is implicit in the role.
 */
@Serializable
data class Envelope(
    val v: Int = 1,
    val id: String,
    val ts: Long,
    val type: String,
    val device_id: String,
    val payload: JsonElement
)
