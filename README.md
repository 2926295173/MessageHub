# PhoneBridge

> LAN-first, self-hosted, cross-platform bridge to manage multiple Android phones from a single desktop daemon.

**Status:** 🚧 Pre-alpha / under active development. MVP scope: device pairing, notification sync, SMS send/receive, call control. Milestones: M0 ✅ scaffold · M1 ✅ daemon · M2 ✅ discovery/pairing/WS · M3 ✅ business channels · M4 ✅ CI/OpenAPI/live push · **M5 ✅ Android client** · M6 hardening.

## What it is

A two-component system:

- **Desktop Daemon** (Rust): single binary, no native GUI. Hosts a local web console + WebSocket server + mDNS responder. Talks to phones over TLS+WebSocket.
- **Android Client** (Kotlin + Jetpack Compose): registers on LAN via mDNS, maintains a foreground service, exposes notifications / SMS / call state to the daemon.

No cloud. No telemetry. No account. Works fully offline on a local network.

Inspired by KDE Connect and Microsoft Phone Link; explicitly focused on stable notification + SMS sync.

## Architecture

```
Browser  ──HTTP/WS──▶  Desktop Daemon (Rust)  ──TLS WS──▶  Android Client
                          │     │
                          │     ├─ SQLite (devices, notifications, sms, calls)
                          │     ├─ mDNS responder (`_phonebridge._tcp`)
                          │     └─ Embedded Next.js web console
```

## Repository layout

```
mykdeconnect/
├── crates/                       # Rust workspace
│   ├── phonebridge-proto/        # Wire protocol types (JSON Schema backed)
│   ├── phonebridge-core/         # Config, paths, logging, errors
│   ├── phonebridge-crypto/       # ECDH P-256, HKDF, self-signed certs
│   ├── phonebridge-net/          # mDNS + WS handlers
│   ├── phonebridge-storage/      # sqlx migrations + models
│   ├── phonebridge-bus/          # In-process event bus (plugin hook reserve)
│   └── phonebridge-daemon/       # Main binary
├── frontend/                     # Next.js 16 (App Router, static export)
├── android/                      # Kotlin + Compose client
├── schema/                       # protocol.schema.json (source of truth)
├── docs/                         # Protocol, threat model, permissions, dev setup
└── scripts/                      # setup.sh, dev-run.sh, e2e-smoke.sh
```

## MVP scope

- **Android:** device registration, LAN discovery (mDNS), pairing (6-digit code, ECDH), notification listening, SMS receive/send, call state monitoring, answer/hang-up.
- **Desktop:** device management, WebSocket connection management, notification center, SMS center, call control, pairing management, embedded web console.

Out of scope (architecture must accommodate, but no implementation): plugin system, ADB control, AI auto-classification, automation rules, webhooks, Telegram bot, Home Assistant, OpenAPI, multi-user, remote gateway.

## Quick start (development)

Prerequisites and per-component build instructions live in [`docs/dev-setup.md`](docs/dev-setup.md).

```bash
# 1. Prepare daemon config dirs
bash scripts/setup.sh

# 2. Build and run daemon (foreground)
cargo run -p phonebridge-daemon

# 3. Build the web console (separate terminal, dev mode with hot reload)
cd frontend
npm install
npm run dev   # http://localhost:3000/console

# 4. Install Android client (requires connected device or emulator)
cd ../android
./gradlew :app:installDebug
```

## Security

All inter-device traffic is TLS; device identity is bound to an ECDH-derived long-term certificate pinned at pairing time. See [`docs/threat-model.md`](docs/threat-model.md).

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
