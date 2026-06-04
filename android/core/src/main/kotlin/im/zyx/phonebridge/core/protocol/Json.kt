package im.zyx.phonebridge.core.protocol

import kotlinx.serialization.json.Json

/**
 * Shared JSON config used by the Android side for envelope and payload
 * serialization. Mirrors what the Rust side accepts:
 *  - ignoreUnknownKeys (Rust may add fields; we tolerate extras)
 *  - encodeDefaults (we send the v=1 field even when the caller didn't
 *    set it explicitly)
 *
 * Putting it in core keeps every module depending on the same instance.
 */
val json: Json = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
}
