# PhoneBridge Android client

LAN-side mobile half of [PhoneBridge](../). Pairs with the desktop
daemon, mirrors notifications, forwards SMS, and exposes call control
over a persistent TLS WebSocket.

## Status: M5

| Capability | Status |
|---|---|
| Gradle 8.13 + AGP 8.13.2 + Kotlin 2.0 + Compose | done |
| Foreground service owning the WebSocket | done |
| mDNS browser (NsdManager) for `_phonebridge._tcp` | done |
| TLS WebSocket client (Ktor 2 + OkHttp) | done |
| Pairing state machine (6-digit code, no crypto yet) | done |
| NotificationListenerService -> daemon | done |
| SmsReceiver (RECEIVE_SMS) -> daemon | done |
| Call state listener (Telephony) -> daemon | done |
| TelecomManager.answerRingingCall / endCall | done |
| SmsManager.sendTextMessage for sms.send | done |
| Compose UI: PermissionsScreen, PairingScreen, SettingsScreen | done |
| Hilt DI | done |
| DataStore prefs (last desktop, fingerprint) | done |
| Unit tests: 2 (envelope) + 6 (pairing) = 8 | done |
| Debug APK | builds: ~19 MB, package `im.zyx.phonebridge.debug` |
| End-to-end with real daemon on real device | **TODO M5+** |

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
   a 6-digit code, sends `device.pair.request` (code travels in the
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
  the next release. M5 ships a stub: the connection is established
  over TLS but the fingerprint pin is not yet enforced in the
  client (it is stored, but the comparison is not active). M5+
  enables pinning in `BridgeClient.runOnce`.
- **Pairing code**: 6 decimal digits, generated locally. M5+ will
  derive this from HKDF-SHA256 to match the Rust side; for now it's
  a 6-digit `Random` string.
- **Foreground service**: `connectedDevice` type on Android 14+,
  ongoing low-priority notification.

## Known limitations (M5)

- Only one desktop can be paired at a time.
- Notification mirroring is one-way (Android → desktop). Reverse
  dismissal is a M5+ feature.
- TLS fingerprint pinning is not yet enforced; M5+ activates the
  comparison in `BridgeClient.runOnce`.
- SmsReceiver is registered in the manifest and runs without a
  foreground service; the daemon-side `sms.send` command requires
  the persistent WS connection to be up.
