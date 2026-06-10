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

## 7. Releasing (placeholder for M9)

Once a tag is pushed, the CI workflow builds:

- `message-center` (Linux x86_64 + aarch64, macOS x86_64 + aarch64, Windows x86_64)
- `phonebridge-display` (Linux x86_64 + aarch64, macOS x86_64 + aarch64, Windows x86_64)
- `phonebridge-android` (universal APK + split per ABI)

Artifacts are attached to the GitHub release.
