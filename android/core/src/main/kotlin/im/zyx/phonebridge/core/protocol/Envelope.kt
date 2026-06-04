package im.zyx.phonebridge.core.protocol

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * Wire-level envelope. Mirrors phonebridge-proto::Envelope in Rust.
 *
 * Protocol v1 contract (see schema/protocol.schema.json):
 * - id is a UUIDv4 string.
 * - type is one of the 24 message types in PayloadType.
 * - from / to are device-id strings; "*" means broadcast.
 * - ts is RFC3339 in UTC.
 * - payload is opaque JsonElement; the *Payload types below describe
 *   the structure each `type` expects.
 */
@Serializable
data class Envelope(
    val v: Int = 1,
    val id: String,
    val type: String,
    val from: String,
    val to: String,
    val ts: String,
    val payload: JsonElement
)
