# Android Permissions Reference

> Every permission PhoneBridge requests, why, when, and what OEM behavior we have observed. Keep this document honest; if a permission is optional, say so.

## Categories

1. **Required for pairing & connection** — non-skippable.
2. **Required for a specific feature** — gated behind an in-app "Enable X" toggle so users only pay the cost for what they use.
3. **Optional / recommended** — improves reliability, not blocking.

---

## 1. Required

### `INTERNET`
- **Why:** WebSocket to the desktop daemon.
- **Risk:** None. Already granted at install time on all Android versions.
- **OEM notes:** None.

### `ACCESS_NETWORK_STATE`
- **Why:** Detect when the device has any network at all (for the foreground service's reconnect logic) and distinguish Wi-Fi vs cellular.
- **Risk:** None. Normal permission.
- **OEM notes:** None.

### `CHANGE_WIFI_MULTICAST_LOCK`
- **Why:** Acquire a `WifiManager.MulticastLock` so the OS does not filter out mDNS multicast packets. Without this, NSD discovery silently fails on many devices, especially when the screen is off.
- **Risk:** Low. Holding the lock costs ~1% battery over an evening. We release it when the foreground service is destroyed.
- **OEM notes:**
  - **MIUI** may aggressively kill the process holding this lock. Users on MIUI need to disable battery saver for our app.
  - **EMUI / HarmonyOS** similar: foreground service + "App Launch" set to "Manage manually".
  - **Stock Android / Pixel** works as documented.

### `FOREGROUND_SERVICE`
- **Why:** Run the WebSocket connection as a foreground service. Required from Android 9 (API 28).
- **Risk:** None. Normal permission.
- **OEM notes:** None for the permission itself, but **OEM battery savers** are the real obstacle (see below).

### `FOREGROUND_SERVICE_CONNECTED_DEVICE` (API 30+)
- **Why:** Android 11 introduced typed foreground service permissions. `connectedDevice` is the correct type for "maintaining a connection to an external device". This is the one we declare.
- **Risk:** None.
- **OEM notes:** None.

### `POST_NOTIFICATIONS` (API 33+)
- **Why:** We use a persistent low-priority notification for the foreground service ("PhoneBridge: connected to Living Room PC"). Android 13+ requires this user-granted permission.
- **Risk:** None. User can revoke; if they do, the foreground service will be killed by the system within minutes.
- **OEM notes:** None.

### `RECEIVE_BOOT_COMPLETED`
- **Why:** Re-launch the foreground service after a device reboot so notifications resume automatically.
- **Risk:** None.
- **OEM notes:** OEM boot optimizers (MIUI, OPPO ColorOS, Vivo Funtouch) may delay or suppress this broadcast. We document the manual start workaround.

### `WAKE_LOCK`
- **Why:** The WebSocket client's CIO engine does not strictly need a wake lock, but a partial wake lock during an active mDNS browse window helps ensure the NSD callback fires promptly. Held only while discovering, not during steady-state.
- **Risk:** Low. We release within seconds of each browse cycle.
- **OEM notes:** Same as `CHANGE_WIFI_MULTICAST_LOCK`.

---

## 2. Feature-gated

These permissions are declared in the manifest, but the user must opt into the corresponding feature in the in-app Settings page before the app starts using the underlying API.

### `READ_PHONE_STATE` — for call state
- **Feature:** Call control.
- **Why:** Distinguish `CALL_STATE_RINGING` / `OFFHOOK` / `IDLE`. Without this, the WebSocket cannot relay incoming call events.
- **Risk:** Medium. This is a "dangerous" runtime permission. We request it only when the user toggles "Call control" on.
- **OEM notes:**
  - On **Xiaomi** you may also need to enable "Display pop-up windows" and "Read phone status" in the app's permission detail page.
  - On **Samsung** One UI 5+, granted normally.

### `ANSWER_PHONE_CALLS` — for `TelecomManager.acceptRingingCall()`
- **Feature:** Call answer from desktop.
- **Why:** Without this, the system rejects the accept call with `SecurityException`.
- **Risk:** Medium. "Dangerous" runtime permission.
- **OEM notes:**
  - **MIUI / EMUI** users have reported inconsistent behavior even with the permission granted; if a user is blocked, we fall back to instructing them to pick up the call manually.
  - **Stock Android** works as documented on API 28+.

### `CALL_PHONE` — for `TelecomManager.placeCall()`
- **Feature:** Outgoing dial from desktop.
- **Why:** Place an outgoing call programmatically (no `ACTION_CALL` chooser).
- **Risk:** Medium. "Dangerous" runtime permission.
- **OEM notes:** None known.

### `READ_SMS` / `RECEIVE_SMS` — for SMS history + incoming
- **Feature:** SMS send/receive.
- **Why:** `RECEIVE_SMS` lets us catch incoming messages; `READ_SMS` lets us fetch history.
- **Risk:** High. "Dangerous" runtime permission. We request only when the user toggles "SMS" on.
- **OEM notes:** None known.

### `SEND_SMS` — for sending
- **Feature:** SMS send.
- **Why:** Required to call `SmsManager.sendTextMessage`.
- **Risk:** Medium. "Dangerous" runtime permission.
- **OEM notes:** None known.

### `BIND_NOTIFICATION_LISTENER_SERVICE` — for notification listener
- **Feature:** Notification sync.
- **Why:** The system Settings page lists PhoneBridge as a notification access app; the user must manually enable it. We surface a deep link to the correct settings screen.
- **Risk:** The app sees the full content of every notification posted. We **must** filter out our own notifications and respect `FLAG_NO_PEEK`.
- **OEM notes:**
  - **MIUI** has its own "Notification access" sub-menu under "Notifications & Control Center". Path differs.
  - **EMUI** similar.

### `READ_PHONE_NUMBERS` (API 26+)
- **Feature:** Identifying the active SIM for dual-SIM messages.
- **Why:** Distinguish SIM 1 vs SIM 2 subscription id in the protocol payload.
- **Risk:** Low. "Dangerous" but limited to subscription info, not call/SMS content.
- **OEM notes:** None known.

---

## 3. Optional

### `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` (API 23+)
- **Why:** Ask the user to whitelist PhoneBridge from battery optimization. Without it, OEM battery savers will kill the foreground service within hours.
- **Risk:** None. The user can decline.
- **OEM notes:** All OEMs honor it to varying degrees. Stock Android gives a working exemption. MIUI/EMUI/ColorOS honor it for first-party-listed apps only — we document the manual "App battery saver → No restrictions" path.

### `SYSTEM_ALERT_WINDOW`
- **Why:** Show an in-call overlay ("Call from Alice, tap to answer on phone") when a call is incoming and the user is not in the app. **Not requested in MVP.** Tracked for v2.

---

## 4. OEM behavior table (high-priority features)

| Feature              | Stock AOSP | MIUI    | EMUI / HarmonyOS | ColorOS | Funtouch |
|----------------------|-----------|---------|-----------------|---------|----------|
| mDNS browse          | OK        | OK w/ lock + battery | OK w/ lock | OK     | OK      |
| Foreground service   | OK        | OK w/ "No restrictions" | OK w/ "App Launch = Manual" | OK | OK |
| Notification listener| OK        | OK w/ "Notification access" toggle | OK | OK | OK |
| `acceptRingingCall`  | OK        | Flaky   | Flaky            | OK     | OK      |
| Dual-SIM SMS         | OK        | OK      | OK               | OK     | OK      |
| Boot auto-start      | OK        | Manual enable | Manual enable | Manual enable | Manual enable |

When a feature is flaky, we document the manual workaround in the app's help screen and in the README.

---

## 5. Privacy posture

- No analytics. No third-party SDKs. No crash reporter.
- The notification listener only listens to the apps the user explicitly enabled the feature for (we do not silently start listening after install).
- All data flows stay on the LAN. The daemon never opens an outbound connection to any remote host.
- On unpair, all stored credentials (cert, key) are deleted from Android Keystore / DataStore.

---

## 6. Required manifest entries (summary)

```xml
<!-- Always-on -->
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_LOCK" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE_CONNECTED_DEVICE" />
<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />
<uses-permission android:name="android.permission.WAKE_LOCK" />

<!-- Feature-gated, declared but requested at runtime -->
<uses-permission android:name="android.permission.READ_PHONE_STATE" />
<uses-permission android:name="android.permission.ANSWER_PHONE_CALLS" />
<uses-permission android:name="android.permission.CALL_PHONE" />
<uses-permission android:name="android.permission.READ_SMS" />
<uses-permission android:name="android.permission.RECEIVE_SMS" />
<uses-permission android:name="android.permission.SEND_SMS" />
<uses-permission android:name="android.permission.READ_PHONE_NUMBERS" />

<!-- BIND_NOTIFICATION_LISTENER_SERVICE is a service-level permission, declared on the service. -->

<!-- Optional -->
<uses-permission android:name="android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS" />
```

The minimum SDK is 26 (Android 8.0). NotificationListenerService has been stable since 21; mDNS via NsdManager since 16; everything above works.
