# PhoneBridge Protocol v1

> **Source of truth:** [`schema/protocol.schema.json`](../schema/protocol.schema.json).
> This document is human-readable companion. If they disagree, the schema wins.

## 1. Transport

- **WebSocket** over **TLS 1.2+**, URI scheme `wss://`.
- Desktop listens on `0.0.0.0:8443`, path `/ws`. Configurable.
- Binary frames are **not** used. All frames are **text frames** containing a single UTF-8 JSON envelope.
- mDNS service: `_phonebridge._tcp` (port and TXT records carry identity; see §3).

## 2. Envelope

Every frame is a single JSON object with these required fields:

| Field       | Type   | Description                                                  |
|-------------|--------|--------------------------------------------------------------|
| `v`         | int    | Protocol version. Constantly `1`.                            |
| `id`        | uuid   | Per-message unique id (UUIDv4). Used for de-duplication.     |
| `ts`        | int64  | Unix epoch milliseconds when the message was created.        |
| `type`      | string | Dotted message type, see §4.                                 |
| `device_id` | uuid   | Stable id of the **sending** device (phone or desktop).      |
| `payload`   | object | Type-specific body. See §4. May be `{}` for empty payloads.   |

Implementations **MUST** reject messages where `v != 1`. Forward compatibility (v2+) is out of MVP scope.

## 3. Device identity

Every device (Android phone, desktop daemon) has:

- A **stable UUIDv4** generated on first install (`device_id`).
- A **long-term ECDH P-256 keypair** (NIST P-256, 32-byte private, 65-byte uncompressed public).
- A **self-signed X.509 certificate** generated after pairing, binding the public key to the `device_id`. PEM-encoded.

Public key is advertised in mDNS TXT as `pubkey` (base64, no padding, of the 65-byte SubjectPublicKeyInfo point).

The certificate is **not** used for X.509 CA chains — it is **pinned** at pairing time. After pairing, the peer stores the certificate fingerprint (`SHA-256`, colon-separated upper-case hex) and refuses any TLS handshake presenting a different cert.

## 4. Message types

All types are namespaced `domain.action`. The schema enforces one payload schema per type.

### 4.1 Device lifecycle

| Type                | Direction         | Purpose                                                              |
|---------------------|-------------------|----------------------------------------------------------------------|
| `device.hello`      | both → both       | First message after WebSocket open. Carries name, type, pubkey, port.|
| `device.heartbeat`  | both → both       | Liveness ping. Daemon times out devices after 3 missed (90s).        |
| `device.info.update`| android → desktop | Battery, network, OS version, app version.                           |
| `device.unpair`     | either → either   | Request to remove the pairing. Closes the WS.                        |

### 4.2 Pairing (6-step)

| Type                   | Direction          | Purpose                                                  |
|------------------------|--------------------|----------------------------------------------------------|
| `device.pair.request`  | initiator → peer   | Send ephemeral ECDH pubkey.                              |
| `device.pair.challenge`| responder → both   | Ephemeral pubkey + 6-digit code + 30s expiry timestamp.  |
| `device.pair.confirm`  | responder → initiator | `{accepted: true\|false}` from the user confirmation. |
| `device.pair.accept`   | initiator → responder | Acknowledgement before sending the cert.              |
| `device.pair.reject`   | responder → initiator | Optional reason string.                                |
| `device.pair.complete` | initiator → responder | `cert_pem` + `cert_fingerprint`.                      |

**Code derivation** (must be identical on both sides):

```
shared_secret = ECDH(ephemeral_priv_initiator, ephemeral_pub_responder)
              == ECDH(ephemeral_priv_responder, ephemeral_pub_initiator)

hkdf_salt     = "phonebridge/v1/pair"       (UTF-8 bytes, 21)
hkdf_info     = "phonebridge/v1/code"       (UTF-8 bytes, 21)
okm            = HKDF-SHA256(shared_secret, salt=hkdf_salt, info=hkdf_info, L=4)
code_int       = okm as u32 (big-endian) mod 1_000_000
code           = format!("{:06}", code_int)
```

The 6-digit code is shown **only on the Android side** (user confirmation). The desktop displays a generic "waiting for confirmation on phone" state with a 30-second countdown synced to `pair.challenge.expires_at`.

### 4.3 Notifications

| Type                       | Direction         | Purpose                                            |
|----------------------------|-------------------|----------------------------------------------------|
| `notification.received`    | android → desktop | New notification posted on the phone.              |
| `notification.dismissed`   | android → desktop | User dismissed a notification.                     |

**Filter rule (Android side):** The app's own package must be filtered out. Apps using `FLAG_NO_PEEK` / `LockscreenVisibility.SECRET` are flagged with `is_sensitive=true` so the desktop console can hide content previews. A user-managed blocklist applies additionally.

### 4.4 SMS

| Type                | Direction         | Purpose                                                |
|---------------------|-------------------|--------------------------------------------------------|
| `sms.received`      | android → desktop | Incoming SMS broadcast received.                       |
| `sms.send.request`  | desktop → android | Send an SMS.                                           |
| `sms.send.result`   | android → desktop | Result for a `sms.send.request` (matched by `request_id = envelope.id`). |
| `sms.list.request`  | desktop → android | Fetch recent SMS history (limit, before-ts).           |
| `sms.list.result`   | android → desktop | Response, list of `smsReceived`-shaped messages.       |

`request_id` is the **envelope `id`** of the corresponding `sms.send.request`. The desktop correlates results by id.

### 4.5 Calls

| Type                   | Direction         | Purpose                                              |
|------------------------|-------------------|------------------------------------------------------|
| `call.state`           | android → desktop | Phone state transition (`idle` / `ringing` / `offhook`). |
| `call.incoming`        | android → desktop | Dedicated event when a new incoming call starts.    |
| `call.answer.request`  | desktop → android | User clicked "Answer" in web console.                |
| `call.end.request`     | desktop → android | User clicked "Hang up".                              |
| `call.dial.request`    | desktop → android | User initiated an outgoing call from web console.    |
| `call.history`         | android → desktop | Recent call log entries.                             |

For `call.answer.request` / `call.end.request` / `call.dial.request`, the Android side answers with a `sms.send.result`-style `*_result` envelope if needed in v2; in v1 the desktop just trusts the WS round-trip and reflects the state via the next `call.state` event.

## 5. Error handling

In v1 there is **no** explicit error message type. Receivers:

- Drop the message silently and log if the schema is violated.
- Close the WebSocket with code `1008` (policy violation) and reason text if the device sends a `device.hello` with a `device_id` that doesn't match the pinned cert fingerprint.

## 6. Rate limits

- `device.heartbeat`: 1 / 30s per device.
- `notification.received`: at most 1 / 50ms per device (per app coalesce window).
- `sms.send.request`: 10 / minute per device.
- `call.dial.request`: 5 / minute per device.

Violations close the WS with code `1008`.

## 7. Versioning

The protocol field is `v`. v1 is the only version in MVP. A v2 would be negotiated out-of-band (not in MVP).

## 8. Wire examples

### 8.1 Android sends notification

```json
{
  "v": 1,
  "id": "5e7c0e9a-1d2b-4f9c-8b2e-7a1c2d3e4f5a",
  "ts": 1717000000123,
  "type": "notification.received",
  "device_id": "0a1b2c3d-4e5f-6789-0abc-def012345678",
  "payload": {
    "id": "android-notif-abc123",
    "package": "com.whatsapp",
    "app_name": "WhatsApp",
    "title": "Alice",
    "content": "Hey, are you free?",
    "posted_at": 1717000000000,
    "is_sensitive": false,
    "category": "msg"
  }
}
```

### 8.2 Desktop requests SMS send

```json
{
  "v": 1,
  "id": "8d3b1a5e-9f0c-4d2a-b5e3-1c7d8e9f0a1b",
  "ts": 1717000000456,
  "type": "sms.send.request",
  "device_id": "0d0e0f0a-0b0c-0d0e-0f0a-0b0c0d0e0f0a",
  "payload": {
    "to": "+8613800000000",
    "body": "Hello from desktop",
    "subscription_id": 1
  }
}
```
