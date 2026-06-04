package im.zyx.phonebridge.core.protocol

/**
 * The 24 message types the v1 daemon understands. Android mirrors them
 * one-to-one; the wire representation is a lowercase snake_case string.
 *
 * Keep this list in sync with crates/phonebridge-proto/src/payload.rs
 * and schema/protocol.schema.json.
 */
object MessageType {
    const val DEVICE_HELLO = "device.hello"
    const val DEVICE_PAIR_REQUEST = "device.pair.request"
    const val DEVICE_PAIR_CHALLENGE = "device.pair.challenge"
    const val DEVICE_PAIR_CONFIRM = "device.pair.confirm"
    const val DEVICE_PAIR_RESULT = "device.pair.result"
    const val DEVICE_PAIRED = "device.paired"
    const val DEVICE_UNPAIR = "device.unpair"

    const val NOTIFY_RECEIVED = "notification.received"
    const val NOTIFY_DISMISSED = "notification.dismissed"

    const val SMS_RECEIVED = "sms.received"
    const val SMS_SEND = "sms.send"
    const val SMS_SEND_RESULT = "sms.send.result"

    const val CALL_STATE = "call.state"
    const val CALL_INCOMING = "call.incoming"
    const val CALL_ANSWER = "call.answer"
    const val CALL_END = "call.end"
    const val CALL_DIAL = "call.dial"

    const val BATTERY = "battery"
    const val CLIPBOARD_SET = "clipboard.set"
    const val PING = "ping"
    const val PONG = "pong"
    const val ERROR = "error"

    const val CONSOLE_HELLO = "console.hello"
    const val CONSOLE_EVENT = "console.event"
}
