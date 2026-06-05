# GitHub Actions workflows

CI runs on every push and pull request against `main`/`master`:

- `ci-desktop.yml` — Rust `fmt --check`, `clippy -D warnings`, `cargo test --workspace`, embedded Next.js build, and an end-to-end smoke run against `scripts/e2e-smoke.sh`.
- `ci-android.yml` — Gradle `:core:testDebugUnitTest`, `:app:testDebugUnitTest`, `:app:assembleDebug`, then uploads the debug APK as a build artifact.

Both jobs cache the Cargo registry, the `target/` directory, and the Gradle wrapper to keep runs fast.

The release pipeline (cross-platform daemon binaries + universal/split APKs) is the **M9** milestone; see [`docs/dev-setup.md` §7](../docs/dev-setup.md#7-releasing-placeholder-for-m9).
