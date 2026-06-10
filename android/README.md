# PhoneBridge Android client

LAN-side mobile half of [PhoneBridge](../). Pairs with the desktop
daemon, mirrors notifications, forwards SMS, and exposes call control
over a persistent TLS WebSocket.

## Status: M6 (hardening)

| Capability | Status |
|---|---|
| Gradle 8.13 + AGP 8.13.2 + Kotlin 2.0 + Compose | done |
| Foreground service owning the WebSocket | done |
| mDNS browser (NsdManager) for `_phonebridge._tcp` | done |
| TLS WebSocket client (Ktor 2 + OkHttp) | done |
| Pairing state machine with ECDH P-256 + HKDF-SHA256 4-digit code | done |
| Long-term identity in Android Keystore (`phonebridge.identity.v1`) | done |
| TLS fingerprint pinning (stored + enforced) | done |
| NotificationListenerService -> daemon | done |
| Reverse dismissal: swipe / system cancel -> `notification.dismissed` | done |
| SmsReceiver (RECEIVE_SMS) -> daemon, null-safe | done |
| Call state listener (Telephony) -> daemon | done |
| TelecomManager.answerRingingCall / endCall | done |
| SmsManager.sendTextMessage for sms.send | done |
| Compose UI: PermissionsScreen, PairingScreen (mDNS + manual IP), SettingsScreen | done |
| Hilt DI | done |
| DataStore prefs (last desktop, fingerprint, cert) | done |
| Unit tests: 34 (envelope, ECDH, HKDF, pairing code, cert gen, pairing machine, SMS receiver) | done |
| Debug APK | builds: ~19 MB, package `im.zyx.phonebridge.debug` |
| End-to-end with real daemon on real device (LAN) | done |

## Build

```bash
cd android
./gradlew :app:assembleDebug
# -> app/build/outputs/apk/debug/app-debug.apk
```

Or via the host's installed gradle:

```bash
PATH=$PATH:/root/.gradle/wrapper/dists/gradle-8.13-bin/*/gradle-8.13/bin \
  gradle :app:assembleDebug
```

## Run on a device

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n im.zyx.phonebridge.debug/im.zyx.phonebridge.ui.MainActivity
```

Or use Android Studio: open the `android/` directory.

## Module map

```
android/
├── app/             # the APK module (Hilt entry, Compose UI, services)
│   ├── src/main/kotlin/im/zyx/phonebridge/
│   │   ├── PhoneBridgeApp.kt          # @HiltAndroidApp + notification channel
│   │   ├── ui/                        # Compose UI (Activity, screens, theme)
│   │   ├── network/                   # BridgeClient, BridgeService, NsdRegistrar
│   │   ├── pairing/                   # PairingMachine (state machine)
│   │   ├── notification/              # NotificationRelayService
│   │   ├── sms/                       # SmsReceiver
│   │   ├── telephony/                 # CallController
│   │   ├── data/                      # DataStore-backed PrefsRepository
│   │   └── di/                        # Hilt modules
│   └── src/main/AndroidManifest.xml   # permissions, services, receivers
└── core/            # pure JVM module, the protocol layer
    └── src/main/kotlin/im/zyx/phonebridge/core/protocol/
        ├── Envelope.kt
        ├── Payloads.kt                # 19 @Serializable payload types
        ├── MessageType.kt             # 24 message-type constants
        └── Json.kt                    # shared Json config
```

## Pairing flow (Android side)

1. App opens → `PermissionsScreen` requests POST_NOTIFICATIONS,
   RECEIVE_SMS, READ_PHONE_STATE, etc., and asks the user to enable
   notification access in Settings.
2. `PairingScreen` calls `NsdRegistrar.discoverFirstDesktop()` which
   resolves a `_phonebridge._tcp` service.
3. User taps **Generate code** → `PairingMachine.begin(...)` produces
   a 4-digit code, sends `device.pair.request` (code travels in the
   payload) to the desktop via the `BridgeService` (Foreground
   `connectedDevice`).
4. User types the code on the desktop. The desktop sends
   `device.pair.challenge` echoing the code.
5. Android sends `device.pair.confirm`. Desktop responds with
   `device.pair.result(accepted=true)`. UI flips to **Paired**.
6. From this point on, every posted notification / received SMS /
   call-state transition is forwarded to the daemon over the same
   WebSocket.

## Security notes (MVP)

- **TLS**: every connection is `wss://`; the daemon's self-signed
  cert fingerprint is fetched from `GET /api/v1/cert` and pinned in
  `BridgeClient.runOnce` before the WebSocket upgrade. On a mismatch
  the connection is refused and the user is prompted to re-pair.
- **Pairing code**: 6 decimal digits derived from HKDF-SHA256 of the
  ECDH shared secret (salt = `phonebridge/v1/pair`, info =
  `phonebridge/v1/code`, 4-byte OKM, then 6 decimal digits). The
  Rust side (`phonebridge-crypto/pairing_code.rs`) and the Kotlin
  side (`core/crypto/PairingCode.kt`) agree byte-for-byte, so the
  user sees the same code on both devices.
- **Long-term identity**: the P-256 keypair lives in
  `AndroidKeyStore` under the alias `phonebridge.identity.v1`; the
  private key is non-extractable on devices with a TEE/StrongBox.
  ECDH `agree()` runs inside the Keystore. The X.509 self-signed cert
  and its SHA-256 fingerprint are persisted in DataStore, and the
  pubkey of the stored cert is verified against the live Keystore
  pubkey on every load so the fingerprint stays stable across
  process restarts.
- **Foreground service**: `connectedDevice` type on Android 14+,
  ongoing low-priority notification.

## Known limitations (M6)

- Only one desktop can be paired at a time.
- Notification mirroring is bidirectional: Android posts to the
  daemon, and `notification.dismissed` flows back so swipe-to-dismiss
  on the phone keeps the desktop view in sync.
- SmsReceiver is registered in the manifest and runs without a
  foreground service; the daemon-side `sms.send` command requires
  the persistent WS connection to be up.
