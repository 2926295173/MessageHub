# Development Setup

> Everything you need to go from a fresh checkout to `cargo run` + Android APK installed on a device.

## 1. Toolchain overview

| Component       | Version  | Where installed                         | Why                                 |
|-----------------|----------|------------------------------------------|-------------------------------------|
| Rust (stable)   | ≥ 1.78   | `rustup`                                 | message-center + types              |
| `cargo`         | bundled  | with rustup                              | Build, test, lint                   |
| Node.js         | ≥ 20.x   | system / nvm                             | Web console build                   |
| npm             | bundled  | —                                        | Dependency installer                |
| JDK             | 17 or 21 | system                                   | Android Gradle Plugin               |
| Android SDK     | API 35   | `/opt/android-sdk` (this host)           | Compile target, build tools, NDK    |
| Gradle          | 8.13     | `gradle/wrapper/gradle-wrapper.properties | Project's wrapper, pinned           |
| `adb`           | platform-tools 37 | `/opt/android-sdk/platform-tools` | Device install + log capture  |

This host already has Rust 1.95, Node (verify with `node --version`), JDK 21, Android SDK at `/opt/android-sdk`, and Gradle 8.13 downloaded at `/root/.gradle/wrapper/dists/`.

## 2. First-time setup

```bash
# From repo root
cd /root/mykdeconnect

# 1. Prepare runtime dirs
bash scripts/setup.sh
# Creates:
#   ~/.config/phonebridge/config.toml         (sample)
#   ~/.local/share/phonebridge/                (db, certs, logs)

# 2. Build the web console (M1 will embed this into the message-center)
cd frontend
npm install
npm run build      # writes frontend/out/  (consumed by message-center at build time)
cd ..

# 3. Build the message-center
cargo build
# 4. Run it
cargo run -p message-center
# → listens on https://0.0.0.0:8443
# → https://localhost:8443/console/  (web UI)
# → https://localhost:8443/api/v1/health  (REST health)

# 5. Build the Android agent (M4+)
cd android
echo "sdk.dir=/opt/android-sdk" > local.properties
./gradlew :app:assembleDebug
./gradlew :app:installDebug      # requires `adb devices` to show a device
```

## 3. Per-component workflows

### 3.1 Message center (Rust)

```bash
# Run with default config
cargo run -p message-center

# Run with custom config
cargo run -p message-center -- --config /path/to/config.toml

# Run unit + integration tests
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`config.toml` keys (filled in by `setup.sh`):

```toml
[server]
# 8443 is the HTTPS-convention port (recommended for TLS mode).
# When you start the message-center with `--no-tls` (plain HTTP, dev
# only), the binary auto-shifts this default to "0.0.0.0:8080" — the
# HTTP-convention port — unless you pass `--bind` to override. Pin
# the port here to lock the choice regardless of mode.
bind = "0.0.0.0:8443"
data_dir = "~/.local/share/phonebridge"

[discovery]
service_type = "_phonebridge._tcp"
enabled = true

[storage]
# SQLite path; defaults to {data_dir}/phonebridge.db
db_path = ""

[logging]
level = "info"     # trace, debug, info, warn, error
file = "~/.local/share/phonebridge/message-center.log"
```

### 3.2 Web console

```bash
cd frontend
npm run dev        # dev server on http://localhost:3000/console
# message-center must be running on :8443; CORS allows localhost:3000
npm run build      # writes out/ which the message-center embeds
npm run lint
npm run type-check
```

Environment variables (set in `.env.local` for dev):

```
NEXT_PUBLIC_API_BASE=https://localhost:8443/api/v1
NEXT_PUBLIC_WS_URL=wss://localhost:8443/ws
```

In production builds these are baked in at `next build` time.

### 3.3 Android client

```bash
cd android
./gradlew :app:assembleDebug           # build APK
./gradlew :app:installDebug            # install to connected device
./gradlew :core:test                   # JVM unit tests
./gradlew :app:lint                    # Android Lint
./gradlew :app:test                    # connected tests (requires device or emulator)

# Logs
adb logcat -s PhoneBridge:V BridgeService:V NotificationRelay:V
```

Target device: this development environment has a real Android phone on the LAN at `192.168.123.60`. Connect with:

```bash
adb connect 192.168.123.60:5555
adb devices
```

If the phone is not running an `adb` daemon, enable USB debugging and either:
- Connect via USB and `adb tcpip 5555` from the device shell, or
- Use a wireless ADB session via `adb pair` (API 30+).

## 4. Common pitfalls

| Symptom | Cause | Fix |
|---------|-------|-----|
| `cargo run` panics: "address already in use" | Port 8443 occupied | Edit `config.toml` `[server].bind` or stop the other process. |
| `npm run build` errors with "Cannot find module" | Stale lockfile | `rm -rf frontend/node_modules frontend/out && npm install`. |
| Web console shows blank page at `/console` | Browser cached old static | Hard refresh (Cmd/Ctrl+Shift+R). |
| `gradle` errors with "SDK location not found" | `local.properties` missing | `echo "sdk.dir=/opt/android-sdk" > android/local.properties`. |
| `adb` shows `unauthorized` | Device needs ADB auth tap | Tap "Allow USB debugging" on the device. |
| NSD browse never returns devices on MIUI | Battery saver killing the foreground service | Settings → Apps → PhoneBridge → Battery → "No restrictions". |
| mDNS browse works but TLS handshake fails | message-center cert regenerated, phone still has old pin | Open PhoneBridge Android app → Settings → "Forget desktop" and re-pair. |

## 5. Network layout

The dev machine and the phone must be on the same L2 segment (same Wi-Fi, no AP isolation). mDNS is multicast on `224.0.0.251` UDP/5353.

For testing across VLANs or in environments with AP isolation, use the manual IP entry path in the Android pairing screen (M6).

## 6. IDE recommendations

- **Rust:** `rust-analyzer` (CLion / VS Code).
- **TypeScript:** VS Code + the official Next.js extension.
- **Kotlin:** Android Studio Hedgehog or newer.
- **Schema:** any JSON editor; we recommend `vscode-json-schema` for autocompletion against the JSON Schema meta.

### 6.1. Testing the `deskdisplay` toast back-ends

The `DeskDisplay` crate ships with three back-ends
sharing a single translation layer (title / body /
buttons) and an in-process mock that captures every
`present_xxx` translation as a typed `MockToast`:

- **Linux (`LinuxBackend`)** — talks to the real
  `org.freedesktop.Notifications` D-Bus surface via
  `zbus`. Requires a running notification daemon
  (GNOME / KDE / dunst / mako / …) on a graphical
  session to actually display anything, but the
  translation logic is testable without it.
- **Windows 10/11 (`WindowsBackend`)** — talks to
  `ToastNotificationManager` via the official
  `windows` crate (WinRT). The crate dependency is
  **gated on `target_os = "windows"`** so Linux
  contributors do not download Windows metadata on
  every `cargo update`. The shared code path is
  compiled everywhere so the XML template builder
  and the translation layer stay in sync across
  platforms; the live `Show()` call is the only piece
  that needs a real Windows machine to exercise.
- **macOS / BSD (`StubBackend`)** — logs the event
  and drops it on the floor. Will be replaced by
  `objc2` / `UNUserNotificationCenter` in a future
  release.

#### Headless tests (any host, no GUI required)

```bash
cargo test -p deskdisplay --lib
```

This runs the full mock + XML-template test suite
(46 tests as of M6 hardening). Every assertion is on
the exact `(title, body, actions)` triple the OS
surface would have been told. If a test ever fails,
that is the signal that the user-visible toast format
has drifted, not that the test is running a parallel
re-implementation.

#### Verifying real Linux toasts (optional)

To watch a real toast land on your desktop, you need a
graphical session with a notification daemon running.
The simplest setup on Debian-family distros is
`sudo apt install dunst` and a session that auto-starts
it (most do).

```bash
# 1. Build & start message-center on the dev host:
bash scripts/setup.sh
./target/debug/message-center --no-tls --bind 0.0.0.0:8080 &

# 2. Start deskdisplay pointing at it (port 8080, HTTP):
mkdir -p /tmp/dd-conf
cat > /tmp/dd-conf/display.toml <<EOF
[daemon]
url = "http://127.0.0.1:8080"
token_file = "$HOME/.config/phonebridge/display.token"

[i18n]
locale = "en"
EOF
RUST_LOG=info,phonebridge_net=debug,deskdisplay=info \
    ./target/debug/deskdisplay --config /tmp/dd-conf/display.toml &

# 3. Push a synthetic SMS event so you can eyeball the
#    toast without pairing a real phone:
curl -k -sS http://127.0.0.1:8080/api/v1/pair/cli
# (use the pair_cli output for an end-to-end demo;
# see scripts/e2e-smoke.sh for the full handshake)
```

You should see a toast land in your notification
daemon with the "SMS from +86…" title and three
buttons. Clicking `[Reply]` will spawn `zenity` (or
`kdialog` if zenity is missing) to collect the
reply text; the reply is then sent back over the
WebSocket to `message-center` and forwarded to the
Android agent.

#### Verifying real Windows 11 toasts (manual, on a Win 11 box)

The Windows back-end ships **without** the `windows`
crate in `DeskDisplay/Cargo.toml` so that Linux CI
builds do not need to download Windows metadata.
To exercise the live `ToastNotificationManager.Show()`
path on a real Windows 11 machine, do the following:

1. `git pull` this branch on your Win 11 box.
2. Add the platform-specific dep to
   `DeskDisplay/Cargo.toml`:

   ```toml
   [target.'cfg(target_os = "windows")'.dependencies]
   windows = { version = "0.58", features = [
       "UI_Notifications",
       "Data_Xml_Dom",
       "Foundation",
   ] }
   ```

3. Replace the no-op body of
   `DeskDisplay/src/backends/windows.rs::WindowsBackend::start()`
   with the real
   `ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))`
   call, and the no-op rendering inside
   `show_translated()` with the actual
   `ToastNotificationManager::Show()` WinRT call.
   Wire the `Activated` `TypedEventHandler` to map
   `phonebridge://action/{key}` back to a
   `DisplayAction` via the same `action_for_key` table
   the Linux back-end uses (mirrors
   `crates/phonebridge-display/src/backends/linux.rs`).
4. `cargo build -p deskdisplay` — should compile
   cleanly on Win 11 MSVC.
5. Pair the dev machine with an Android client and
   trigger a real `sms.received` event. You should
   see an Action Center toast with three buttons;
   clicking a button should post the corresponding
   `DisplayAction` back to `message-center`, which
   the Android agent then turns into a real SMS reply
   / mark-as-read / dismiss.

The XML shape that the live `Show()` will send is
already covered by
`backends::windows::tests::build_toast_xml_*` and
`xml_for_an_sms_toast_contains_address_and_three_buttons`,
so a test that passes locally is strong evidence
that the WinRT call will render the same thing once
wired up.

## 7. Releasing (placeholder for M9)

Once a tag is pushed, the CI workflow builds:

- `message-center` (Linux x86_64 + aarch64, macOS x86_64 + aarch64, Windows x86_64)
- `deskdisplay` (Linux x86_64 + aarch64, macOS x86_64 + aarch64, Windows x86_64)
- `phonebridge-android` (universal APK + split per ABI)

Artifacts are attached to the GitHub release.
