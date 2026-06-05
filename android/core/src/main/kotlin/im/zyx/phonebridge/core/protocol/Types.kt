package im.zyx.phonebridge.core.protocol

/**
 * Stable device-type discriminator on the wire.
 * Rust uses `#[serde(rename_all = "lowercase")]` so the JSON value
 * is `"desktop"` or `"android"`.
 */
@kotlinx.serialization.Serializable
enum class DeviceType {
    @kotlinx.serialization.SerialName("desktop")
    Desktop,
    @kotlinx.serialization.SerialName("android")
    Android
}

@kotlinx.serialization.Serializable
enum class NetworkType {
    @kotlinx.serialization.SerialName("wifi")
    Wifi,
    @kotlinx.serialization.SerialName("cellular")
    Cellular,
    @kotlinx.serialization.SerialName("ethernet")
    Ethernet,
    @kotlinx.serialization.SerialName("none")
    None
}

@kotlinx.serialization.Serializable
enum class CallStateKind {
    @kotlinx.serialization.SerialName("idle")
    Idle,
    @kotlinx.serialization.SerialName("ringing")
    Ringing,
    @kotlinx.serialization.SerialName("offhook")
    Offhook
}

@kotlinx.serialization.Serializable
enum class CallDirection {
    @kotlinx.serialization.SerialName("incoming")
    Incoming,
    @kotlinx.serialization.SerialName("outgoing")
    Outgoing,
    @kotlinx.serialization.SerialName("missed")
    Missed
}
