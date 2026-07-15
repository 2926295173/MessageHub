# AGENTS.md — PhoneBridge (MessageHub)

Compact orientation for OpenCode sessions. Long-form guidance lives in
[`docs/dev-setup.md`](docs/dev-setup.md); this file is the agent-specific
shortlist (things you would otherwise get wrong on first read).

## Project shape

Three components, all AGPL-3.0-or-later, repo
`github.com/2926295173/MessageHub`:

| Component | Path | Type | Binary |
|---|---|---|---|
| Desktop broker | `crates/message-center` | Rust, axum + sqlite | `message-center` |
| Library crates | `crates/phonebridge-proto` `…-core` `…-crypto` `…-net` `…-storage` `…-bus` | Rust libs | — |
| Desktop notifier | `DeskDisplay/` (**repo root, not under `crates/`**) | Rust, zbus (Linux) | `deskdisplay` |
| Android client | `android/` (Gradle modules `:app` + `:core`) | Kotlin + Compose, Hilt | `app-debug.apk` |
| Web console | `frontend/` | Next.js 16 (App Router, static export) | embedded into `message-center` |

Wire surface: `phonebridge-proto` (JSON-Schema-backed, in `schema/`).
Rust ↔ Kotlin enums must mirror the schema — do not hand-edit payload
structs without updating `schema/protocol.schema.json` and re-running
`scripts/validate-schema.sh`.

## Default ports (and the trap)

- `message-center` (TLS, default): `0.0.0.0:8443`
- `message-center --no-tls`: auto-shifts to `0.0.0.0:8080` **only if** the
  user did not pass `--bind` and the config does not pin a port. 8443 is
  HTTPS-convention; squatting on it in plain-HTTP mode breaks tools.
  Override with `config.toml` `[server].bind` or `--bind`.
- `frontend/`: dev server on `:3000` (`npm run dev`). CORS allows
  `localhost:3000` against the message-center.

## Rust toolchain

Pinned via `rust-toolchain.toml`: **channel `1.96.0`**, with
`rustfmt`, `clippy`, `rust-src`. Cargo `rust-version` is `1.78` (MSRV).
`cargo build` / `cargo test` will use the pinned toolchain automatically —
do not invoke `rustup` to install another channel.

The release profile in `Cargo.toml` sets `lto = "thin"`, `codegen-units = 1`,
`strip = "symbols"`, `panic = "abort"`. A workspace `cargo test --release`
will be slow; default to `cargo test` (dev profile).

## Build / test / lint

Rust (run from repo root):

```bash
cargo build -p message-center      # broker
cargo build -p deskdisplay         # notifier
cargo test --workspace             # full suite
cargo test -p deskdisplay --lib    # 46 headless tests (mock + XML)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Android (run from `android/`):

```bash
./gradlew :app:assembleDebug
./gradlew :app:installDebug
./gradlew :core:test               # JVM-only unit tests (no device)
./gradlew :app:testDebugUnitTest   # JVM-only unit tests for :app
./gradlew :app:lint
# /!\ ci-android.yml runs :core:testDebugUnitTest — but :core has no
# `testDebugUnitTest` task by default; use `:core:test` for the JVM suite.
# Treat the CI command as authoritative if you are reproducing a CI failure.
```

Web console (`frontend/`):

```bash
npm install
npm run dev        # localhost:3000/console (needs message-center on :8443)
npm run build      # writes out/, which message-center embeds via include_dir!
npm run lint
npm run type-check
```

If `cargo build -p message-center` is failing on a missing `frontend/out/`
asset, that is the cause — run `npm run build` first.

The web console reads `NEXT_PUBLIC_API_BASE` and `NEXT_PUBLIC_WS_URL` at
`next build` time. Baked into production builds; set them in
`frontend/.env.local` for dev.

## End-to-end smoke

`scripts/e2e-smoke.sh` pre-builds `message-center` before the 20s readiness
wait — if you run it from a cold cache, do not be surprised that the first
~minute is cargo, not test work. Pair with `scripts/fake_android_notify.py`
to push a synthetic notification.

## Dev-host specifics (this machine)

- Rust 1.95 installed via rustup; toolchain will self-upgrade to 1.96.0.
- Node + JDK 21 + Android SDK at `/opt/android-sdk` already present.
- A real Android phone is on the LAN at `192.168.123.60`:
  `adb connect 192.168.123.60:5555` → `adb devices` → `./gradlew :app:installDebug`.
- `android/local.properties` must contain `sdk.dir=/opt/android-sdk` on a
  fresh checkout (see §4 of dev-setup.md).
- The default `gradle wrapper` distribution lives at
  `/root/.gradle/wrapper/dists/`.

## CI / release workflow

Only one workflow is event-driven:

- `.github/workflows/ci-release.yml` — runs on `release: published`,
  builds `message-center` + `deskdisplay` + Android debug APK, attaches
  them to the GitHub Release.

The other two are `workflow_dispatch` only (manual trigger from the
Actions tab):

- `.github/workflows/ci-desktop.yml` — Rust fmt + build + test + smoke.
- `.github/workflows/ci-android.yml` — `:core:testDebugUnitTest :app:testDebugUnitTest` + APK assembly.

**Do not** add `push:` or `pull_request:` triggers to the manual ones —
`ci-release.yml` deliberately owns the build pipeline.

No `gh` CLI is installed on this host; rely on the local toolchain for
builds. Don't try to query Actions status from the agent.

## Android quirks an agent will trip over

- `themes.xml` uses legacy `parent="android:Theme.Material.Light.NoActionBar"`.
  It has no `values-night/` counterpart and no `colors.xml`. The Compose
  `Surface` (in `MainActivity.kt`) drives actual coloring; the XML theme
  only provides the pre-Compose splash window background. In dark mode this
  pair has produced "black-screen on open" reports on some devices — the
  Compose `background` color is `0xFF1B191A`, visually near-black, and
  the XML `windowBackground` does not follow system night mode.
- `android/app/build.gradle.kts` enables R8 + resource shrinking on the
  **debug** build type (see the comment around line 32). This is
  intentional (the merged dex otherwise exceeds D8's buffer once
  `material-icons-extended` pulls in thousands of unused `ImageVector`
  classes), but it means debug APKs run through the same proguard rules
  as release. `proguard-rules.pro` only keeps kotlinx.serialization
  metadata; Hilt's plugin auto-generates the Hilt-side keep rules.
- The Android app has both `:app` and `:core` modules. `:core` is a
  pure JVM library (no Android dependencies) holding the protocol types
  and crypto primitives shared with the wire schema. `:app` depends on
  it via `implementation(project(":core"))`. Keep new protocol types in
  `:core` so JVM tests can run without an emulator.
- `PairingViewModel.init` runs `PairingMachine.ensureIdentity` (Android
  Keystore + BouncyCastle self-signed cert) on the main thread. First
  launch is ~1–2 s; cold start can push `BridgeService.startForeground`
  close to the 6-s FGS limit on Android 15. The init is wrapped in
  try/catch on the listener path so a Keystore failure is non-fatal
  in the foreground.

## DeskDisplay backend test pattern

`DeskDisplay/src/backends/mock.rs` is a non-cfg-gated in-process capture
backend. The Linux and Windows backends share translation logic with the
mock so the 46 unit tests assert the same `(title, body, actions)` triple
the OS surface would see — if those tests fail, the user-visible toast
format has drifted. The Windows back-end is shipped **without** the
`windows` crate in `DeskDisplay/Cargo.toml` so Linux CI does not pull
Windows metadata. To exercise the live `ToastNotificationManager.Show()`
path on a real Win 11 box, add the dep locally per the instructions in
`docs/dev-setup.md` §6.1.

## Things explicitly NOT to do

- Do not add `push:` / `pull_request:` triggers to any workflow other than
  `ci-release.yml`.
- Do not introduce a `values-night/` or `colors.xml` without also
  updating the Compose color scheme in `ui/theme/Theme.kt` — the two
  need to agree on what dark mode means.
- Do not edit the wire protocol (`crates/phonebridge-proto/`,
  `schema/protocol.schema.json`, or the Kotlin mirror in
  `android/core/src/main/kotlin/im/zyx/phonebridge/core/protocol/`)
  on one side without updating the others. Run
  `scripts/validate-schema.sh` to catch drift.
- Do not move `DeskDisplay/` under `crates/` — it is a workspace member
  at the repo root on purpose (see top of `DeskDisplay/Cargo.toml` and
  the README's "Repository layout" section).
- Do not commit the AGPL-3.0 license with modifications that add a
  commercial / dual-license path. The project is AGPL-3.0-or-later only;
  see `LICENSE` and the SPDX headers in every Rust source file.

## Pointer docs

- [`docs/dev-setup.md`](docs/dev-setup.md) — full per-component workflow
  + common-pitfalls table.
- [`docs/protocol-v1.md`](docs/protocol-v1.md) — wire protocol.
- [`docs/threat-model.md`](docs/threat-model.md) — pairing + TLS pinning.
- [`docs/android-permissions.md`](docs/android-permissions.md) — runtime
  permissions matrix.
- [`CHANGELOG.md`](CHANGELOG.md) — tracks the `phonebridge-daemon` →
  `message-center` rename and the `8443 → 8080` `--no-tls` default
  shift. Read this before changing the broker's name, port, or wire
  shape; both are user-visible breaking changes.
