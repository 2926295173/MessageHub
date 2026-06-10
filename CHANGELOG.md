# Changelog

All notable changes to PhoneBridge are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/) where it makes
sense; the project is pre-1.0 so the rules are relaxed (a 0.x release
may include breaking changes without a major bump).

---

## [Unreleased]

### Changed — `--no-tls` default bind port shifted from 8443 to 8080

When the message-center is started with `--no-tls` (plain HTTP, dev mode)
and the user did **not** pass `--bind` explicitly, the default bind
address is now shifted from `0.0.0.0:8443` to `0.0.0.0:8080`. The
shift is logged at startup:

```
INFO --no-tls: defaulting bind to the HTTP-convention port
     (8443 is HTTPS-convention). Pass --bind to override.
```

#### Why

Port 8443 is universally associated with HTTPS (a "non-standard
HTTPS" port used to distinguish TLS-wrapped HTTP from the standard
443). When the message-center runs in plain HTTP mode, squatting
on 8443 misleads operators and tools — anything seeing `:8443` in
a URL, a proxy config, or a `netstat` listing assumes TLS and
either refuses to connect, prints warnings, or auto-upgrades to
`https://` and gets nowhere. The HTTP convention port 8080 carries
no such baggage.

#### Behavior

| Invocation | Bind |
|---|---|
| `message-center` (TLS, default) | `0.0.0.0:8443` (unchanged) |
| `message-center --no-tls` | `0.0.0.0:8080` (was `0.0.0.0:8443`) |
| `message-center --no-tls --bind 0.0.0.0:9000` | `0.0.0.0:9000` (respect user choice) |
| `message-center` with `bind = "0.0.0.0:8443"` in `config.toml` | `0.0.0.0:8443` (respect user choice) |

The auto-shift only fires when **all three** of these hold:

1. `--no-tls` is set.
2. The user did **not** pass `--bind`.
3. The effective bind (after config-file load) is exactly the
   default `0.0.0.0:8443`.

Any other case — TLS mode, explicit `--bind`, or a non-default port
in `config.toml` — is left alone. The user is in control.

#### Compat notes

- The shift is **not** a breaking change for operators running TLS:
  they keep 8443 by default.
- Operators running `--no-tls` who want 8443 anyway (e.g. they
  reverse-proxy it with a TLS terminator in front) can pin the port
  with `bind = "0.0.0.0:8443"` in `config.toml` or pass
  `--bind 0.0.0.0:8443` on the command line.
- The Android agent discovers the port via mDNS, so no app-side
  change is required. Users with a manually-entered port in
  `prefs` (the rare case where mDNS is broken) need to update the
  port in the app's "主机 / 端口" manual entry fields.

#### Internal

- `crates/message-center/src/main.rs`: a 3-line guard in `main()`
  performs the shift after `args.bind` is applied but before
  logging the bind address.
- `scripts/setup.sh` and `docs/dev-setup.md`: the sample config
  now has a comment explaining the convention so a fresh
  `config.toml` doesn't surprise the operator.
- No tests added — the guard is a 3-line `if` with 3 trivial
  conditions and is exercised by the manual smoke flow documented
  in this entry.

---

### Renamed — central binary: `phonebridge-daemon` → `message-center`

The Rust binary previously known as `phonebridge-daemon` (the central
broker that fans Android events out to the web console and the desktop
notifier) has been renamed to **`message-center`**. The rename brings
the binary's name into alignment with the rest of the architecture and
replaces the Unix-era "daemon" term with a role that maps directly onto
the three-component model:

```
Android Agent        Message Center            Desktop Notifier
(im.zyx.phonebridge) (message-center)          (phonebridge-display)
```

#### ⚠️ BREAKING — user-facing changes

| Before | After |
|---|---|
| `phonebridge-daemon` (binary) | `message-center` |
| `cargo run -p phonebridge-daemon` | `cargo run -p message-center` |
| `RUST_LOG=phonebridge_daemon=info,phonebridge_net=debug` | `RUST_LOG=message_center=info,phonebridge_net=debug` |
| systemd unit: `phonebridge-daemon.service` | `message-center.service` |
| `ExecStart=/usr/local/bin/phonebridge-daemon` | `ExecStart=/usr/local/bin/message-center` |

A bare invocation of the old name will silently fail with "command not
found"; there is **no shim / deprecation alias** by design (one hard
cutover, no two-name maintenance window).

#### Migration checklist

1. Replace the install path: `cp target/debug/message-center
   /usr/local/bin/` (or your equivalent).
2. Update your systemd / launchd / supervisor unit to point at the
   new binary path.
3. Update any `RUST_LOG` directives from `phonebridge_daemon=...` to
   `message_center=...`.
4. Restart the service. No DB schema, no config-file, no data-dir
   migration is needed — see "compat" below.

#### ✅ NOT changed (intentionally preserved for compatibility)

These are deliberately untouched so a user upgrading the binary does
not lose any data or have to redo any setup:

- Config directory: `~/.config/phonebridge/`
- Data directory: `~/.local/share/phonebridge/`
- SQLite database file (`phonebridge.db`) and all migrations
- mDNS service type: `_phonebridge._tcp` (the service type was
  deliberately kept on the `phonebridge` brand)
- WebSocket paths: `/ws`, `/ws/console`, `/ws/display`
- Display-endpoint token file: `~/.config/phonebridge/display.token`
- Android application package: `im.zyx.phonebridge`
- Android client class names: `BridgeClient`, `BridgeService`,
  `BridgeStatus`, etc. (the "bridge" half of the project name is kept
  here, since it is the project name, not the binary name)
- Other Rust crate names: `phonebridge-proto`, `phonebridge-core`,
  `phonebridge-crypto`, `phonebridge-net`, `phonebridge-storage`,
  `phonebridge-bus`, `phonebridge-display`

#### Internal renames (no user-facing impact; listed for traceability)

- Crate directory: `crates/phonebridge-daemon/` →
  `crates/message-center/`
- Crate name: `phonebridge-daemon` → `message-center`
- Type: `DaemonSink` → `CenterSink`
- Type: `DaemonIdentity` → `CenterIdentity`
- File: `crates/message-center/src/daemon_sink.rs` →
  `crates/message-center/src/center_sink.rs`
- Tracing target string: `phonebridge_daemon` → `message_center`
- Self-signed cert common-name string: `"phonebridge-daemon"` →
  `"message-center"` (cert fingerprints therefore change for fresh
  installs; existing pinned fingerprints are unaffected because the
  private key + cert on disk are not regenerated by the rename)
- Doc comment sweep: every "the daemon" / "desktop daemon" /
  "PhoneBridge daemon" / 守护进程 in source files, tests, docs
  (`README.md`, `docs/dev-setup.md`, `docs/protocol-v1.md`,
  `docs/threat-model.md`, `docs/android-permissions.md`), scripts
  (`scripts/setup.sh`, `scripts/e2e-smoke.sh`,
  `scripts/fake_android_notify.py`), and Android
  (`values-zh/strings.xml`, Kotlin doc comments) replaced with
  "message-center" / equivalent.

#### Why

The word "daemon" carries Unix-specific connotations and didn't
visually communicate the binary's role in the three-component
architecture. `message-center` directly maps to the user's mental
model: "the center that routes messages between the Android phone and
the desktop notifier."

---

## [M6] — Android hardening (already shipped before the rename)

- `BootReceiver` for post-reboot auto-start
- `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` prompt + persistent opt-out
- `SYSTEM_ALERT_WINDOW` + Compose-based `FloatingConsoleService` for
  the always-visible quick-action panel
- `WorkManager`-backed keep-alive (constrained `KeepAliveWorkScheduler`,
  `SelfCheckWorker`)
- `HeartbeatController` (app-level heartbeats + 3-miss reconnect)
- `PinnedTrustManager` for TOFU certificate pinning of the daemon
- Network security config allowing cleartext on LAN only
- `PR7`: `/ws/display` endpoint on the daemon, full-duplex
  `DisplayEvent` / `DisplayAction` protocol
- `PR8`: `phonebridge-display` Rust binary with Linux (zbus) backend
  (macOS / Windows backends in PR9 / PR10, not yet shipped)
- `hardware_id` dedup: Android `Settings.Secure.ANDROID_ID` sent in
  `device.hello` so reconnects after `pm clear` collapse onto the same
  device row instead of duplicating
- Multiple history rows for the same phone pruned to the latest
- Locale-aware daemon-i18n: 144 keys served from the daemon at
  `/api/v1/i18n?locale=zh|en`; the web console and the desktop notifier
  fetch dictionaries at runtime; language-switch reloads the page
- Web console: identity / fingerprint / pubkey / device id moved to
  the About page (the only place they are exposed); Devices page
  shows name + paired status + last-seen only; Settings page has the
  language picker + audit log
