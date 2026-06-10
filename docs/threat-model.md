# PhoneBridge Threat Model

> Companion to [`protocol-v1.md`](protocol-v1.md). Defines the trust boundary, attacker model, mitigations, and explicitly out-of-scope risks.

## 1. Trust boundary

```
┌─────────────────────────────────────────────┐
│  Trusted:                                   │
│    • Message-center host (user's machine)   │
│    • User's local network (LAN)             │
│    • User's Android device (post-pairing)   │
│                                             │
│  Untrusted:                                 │
│    • Other devices on the same LAN segment  │
│    • Other devices on the same Wi-Fi AP     │
│    • Any remote network / cloud             │
└─────────────────────────────────────────────┘
```

PhoneBridge has **no cloud component**. All traffic stays on the LAN. The threat model assumes the LAN may be shared with hostile devices (untrusted roommate, public Wi-Fi, conference network, etc.).

## 2. Assets

| Asset                                | Sensitivity       | Where stored                              |
|--------------------------------------|-------------------|-------------------------------------------|
| Notification content (incl. 2FA OTPs)| **High**          | Desktop SQLite, in-memory until consumed |
| SMS bodies                           | **High**          | Desktop SQLite                            |
| SMS metadata (number, timestamp)     | Medium            | Desktop SQLite                            |
| Call log (numbers, durations)        | Medium            | Desktop SQLite                            |
| Device long-term private key         | **Critical**      | Android Keystore, Desktop `*.key.pem` (encrypted at rest in MVP if possible) |
| Desktop TLS private key              | **Critical**      | Desktop `*.key.pem`                       |
| Pairing 4-digit code                 | Transient (30s)   | In-memory only on both sides              |
| Session shared secret (HKDF)         | High              | In-memory only                            |

## 3. Adversary classes

| Class                  | Capability                                                       | In scope? |
|------------------------|------------------------------------------------------------------|-----------|
| **Passive eavesdropper on LAN** | Sniff unicast Wi-Fi frames, ARP spoof.                 | **Yes**   |
| **Active MITM on LAN** | ARP/DHCP spoofing, mDNS poisoning, fake service broadcast.       | **Yes**   |
| **Rogue paired device**| Has a valid (but stolen / user-revoked) cert + private key.       | **Yes**   |
| **Local privileged attacker**  | Local shell on desktop or root on phone.                 | Partial (see §7) |
| **Network remote attacker**    | Reaches the desktop port via WAN.                         | **No** (no inbound port forwarding in MVP) |
| **Compromised phone OS / supply chain** | Backdoored Android system calls.                 | **No** (out of model) |

## 4. Security objectives

1. **Confidentiality:** All LAN traffic between paired devices is encrypted with TLS 1.2+ using ECDHE key exchange and AES-GCM. No cleartext fallback. Snooping a captured frame yields nothing.
2. **Authenticity:** Each side proves possession of the long-term private key during the TLS handshake. A device cannot impersonate another.
3. **Replay protection:** TLS 1.2+ prevents frame replay at the transport layer. Application-level replay is bounded by message `id` de-duplication for the SMS request/result round-trip.
4. **Pairing MITM resistance:** A MITM who can intercept all traffic between Desktop and Android during the 30s pairing window still cannot complete pairing because:
   - The 4-digit code is derived from the ECDH shared secret, and the code is **only shown to the user on the phone**.
   - A 4-digit code has 1/10,000 entropy; the user confirms visually, defeating automated MITM. The 30-second window limits the brute-force space an attacker can probe via relay, but the primary defense is the user's eyes, not the entropy.
5. **Revocation:** Removing a device deletes its pinned cert and key. The message-center will refuse any subsequent handshake presenting the old cert.
6. **Defense in depth:** Even with a compromised link, missing/expired codes, mismatched fingerprints, or repeated pairing attempts trigger explicit failure states and audit log entries.

## 5. Mitigations in detail

### 5.1 mDNS hardening

- The mDNS service type `_phonebridge._tcp` is unencrypted and unauthenticated by definition. TXT records contain only the public key (advertised openly is fine; the private key is never sent) and non-sensitive device name.
- **mDNS spoofing mitigation:** The user is shown the **fingerprint of the responding desktop's cert** during pairing. Spoofed mDNS responses still cannot produce a valid TLS handshake without the desktop's long-term private key. If a fake desktop presents a fake cert, the user sees a fingerprint mismatch when comparing on the phone (v2 enhancement: surface fingerprint in pairing UI).
- **AP-isolated networks:** If mDNS doesn't traverse the AP, the user may input the desktop's IP address manually. Both paths are supported.

### 5.2 Pairing code derivation

See [`protocol-v1.md` §4.2](protocol-v1.md). Constant salt and info strings are deliberate — they make this protocol-specific HKDF step distinguishable from generic ECDH usages, reducing the risk of a confused-deputy attack where an attacker substitutes keys from another protocol run.

The 30-second expiry on `pair.challenge.expires_at` bounds the window for human-error and replay.

### 5.3 TLS pinning

After pairing, each side stores:

- Desktop stores: Android's cert PEM + SHA-256 fingerprint.
- Android stores: Desktop's cert PEM + SHA-256 fingerprint.

On every reconnect, the WebSocket client validates the server cert against the pinned fingerprint. A mismatch closes the connection with code `1008` and the user is shown "Device identity changed. Re-pair required."

### 5.4 Rate limiting

See [`protocol-v1.md` §6](protocol-v1.md). Violations close the WS.

### 5.5 Notification content sensitivity

Notifications flagged `isSensitive=true` (e.g. apps using `FLAG_NO_PEEK`) are stored with the body but the web console displays "(hidden content)" in the preview pane. The body is still searchable in the database but never displayed in the UI without explicit user click.

### 5.6 Log and audit

The message-center writes a structured audit log (`audit_log` table + rolling file) for:

- Pairing start / success / failure / cancellation
- Device connect / disconnect
- Cert rotation
- WS close with non-1000 codes
- Unpair events

Logs do **not** include notification content, SMS bodies, or call audio. They include device ids, message types, and timestamps only.

## 6. Out-of-scope risks (documented, not mitigated in MVP)

| Risk                                              | Why out of scope                                            |
|---------------------------------------------------|-------------------------------------------------------------|
| Local privileged attacker on desktop              | Assumes host integrity. A compromised host can read SQLite directly. |
| Compromised Android system                       | The notification listener sees everything the OS surfaces. Trusting the Android platform is fundamental. |
| Side-channel on the 4-digit code (shoulder surf) | User responsibility; 30s window.                            |
| DoS against the desktop port                     | Mitigated by mDNS discovery + LAN boundary. If a hostile peer is on LAN, the user can disable `discovery_enabled` in config. |
| LAN-wide Wi-Fi sniffing without mTLS              | Mitigated by TLS 1.2+; modern cipher suites provide AEAD.   |
| Replay of `sms.send.request` after desktop reboot | Bounded by `request_id` (envelope id) — Android ignores duplicate `id`s. |

## 7. Local privileged attacker

The MVP does not implement disk encryption for the SQLite database or the TLS key on the desktop. A user with shell access to the desktop host can read everything.

**Future hardening (out of MVP):**

- Encrypt SQLite at rest using `sqlcipher` (binding exists: `libsqlcipher`).
- Encrypt the desktop TLS key with a passphrase derived from the host's TPM/sealed storage.
- Optional screen-lock-style password gate for the web console itself.

## 8. Certificate rotation

In MVP, a cert is rotated by re-pairing. The user removes the device (which deletes the old cert + key) and pairs again. There is no automatic rotation in v1.

A compromised cert (e.g. phone backup restored to a different device) is handled by manual unpair + re-pair.

## 9. Incident response

- **Suspected MITM during pairing:** abort the pairing attempt. Both sides discard ephemeral state. No long-term keys are exposed because they are only generated after `pair.complete`.
- **Suspected key compromise:** unpair the device from the desktop settings page. The message-center deletes the cert + key, broadcasts an mDNS refresh, and refuses reconnects.
- **Stolen phone:** same as compromised key. Use the desktop web console to unpair the device remotely (the next time the phone comes online, the WS is rejected and the user is prompted to re-pair).
