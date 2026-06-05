package im.zyx.phonebridge.core.protocol

/**
 * The 24 message types in protocol v1, as the exact wire strings used
 * by the Rust daemon. Mirror
 * `crates/phonebridge-proto/src/envelope.rs::MessageType::as_str`.
 */
object MessageType {
    const val DEVICE_HELLO = "device.hello"
    const val DEVICE_HEARTBEAT = "device.heartbeat"
    const val DEVICE_INFO_UPDATE = "device.info.update"

    const val DEVICE_PAIR_REQUEST = "device.pair.request"
    const val DEVICE_PAIR_CHALLENGE = "device.pair.challenge"
    const val DEVICE_PAIR_CONFIRM = "device.pair.confirm"
    const val DEVICE_PAIR_ACCEPT = "device.pair.accept"
    const val DEVICE_PAIR_REJECT = "device.pair.reject"
    const val DEVICE_PAIR_COMPLETE = "device.pair.complete"
    const val DEVICE_UNPAIR = "device.unpair"

    const val NOTIFY_RECEIVED = "notification.received"
    const val NOTIFY_DISMISSED = "notification.dismissed"

    const val SMS_RECEIVED = "sms.received"
    const val SMS_SEND_REQUEST = "sms.send.request"
    const val SMS_SEND_RESULT = "sms.send.result"
    const val SMS_LIST_REQUEST = "sms.list.request"
    const val SMS_LIST_RESULT = "sms.list.result"

    const val CALL_STATE = "call.state"
    const val CALL_INCOMING = "call.incoming"
    const val CALL_ANSWER_REQUEST = "call.answer.request"
    const val CALL_END_REQUEST = "call.end.request"
    const val CALL_DIAL_REQUEST = "call.dial.request"
    const val CALL_HISTORY = "call.history"
}
