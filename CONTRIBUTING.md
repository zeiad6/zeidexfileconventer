# Contributing to Zeidex Convert

Thank you for contributing to **Zeidex Convert**. Because this project is offline-first and privacy-focused, every change must respect strict crate boundaries, zero-telemetry guarantees, and full English/Arabic parity.

## Workspace Architecture

| Crate / Folder | Purpose | Notes |
|---|---|---|
| `crates/core` | Format registry, `Op` model, planner (`plan.rs`), executor (`runner.rs`), engine registry (`tools.rs`) | **Zero external dependencies.** Must build and test completely offline (`cargo test -p zeidex-core --test offline`). |
| `crates/cli` | `zeidex-convert` headless CLI | Thin wrapper over `zeidex-core`. |
| `crates/localdrop` | LAN QR file transfer | Own Cargo workspace; no HTTP/TLS client crates allowed. |
| `app/src-tauri` | Tauri v2 desktop backend, overlay wheel, drag hooks, on-demand engine manager | Own Cargo workspace; delegates all file processing to `zeidex-core`. |
| `app/src` | React 19 + TypeScript + Tailwind v4 UI | RTL Arabic & LTR English; all IPC calls go through `app/src/lib/api.ts`. |
| `packaging/` | `engines.json`, `fetch-engines.sh`, `fetch-engines.ps1`, Linux/Flatpak/package-manager manifests | Every downloadable artifact must have a verified SHA-256 digest. |

---

## 1. Adding a New Conversion Target or Format in `crates/core`

1. **Register the format (`crates/core/src/format.rs`):**
   - Add extension mappings in `ext_of` / `category_of_ext` (`Category::Image`, `Video`, `Audio`, `Document`, `Pdf`, `Subtitle`, `Archive`, etc.).
   - Add the format to the target lists (`TARGETS` / `targets_for`) so both the CLI and the Shift-drag wheel expose it.
2. **Gate availability (`crates/core/src/ops.rs`):**
   - Update `engine_for` / `convert_ready` so the target is only offered when its backing engine is installed on the machine. Never show a format whose engine is missing.
3. **Plan the conversion (`crates/core/src/plan.rs`):**
   - Add the conversion branch in `convert(...)`. Keep `plan.rs` completely pure (no filesystem I/O or process spawning—only `Step` construction inside `tmp`).
   - Add unit tests in `plan::tests` verifying the exact generated command arguments.
4. **Update `crates/core/src/runner.rs` (if a custom `Step` or probe is needed):**
   - Always normalize file paths with `tools::normalize_engine_path` so Windows `\\?\` verbatim prefixes never reach C/C++ binaries.

---

## 2. Integrating a New CLI Engine

1. **Add a `Tool` variant (`crates/core/src/tools.rs`):**
   - Add the variant to `Tool`, `Tool::ALL`, `Tool::label`, `Tool::slug`, `Tool::env_key`, `Tool::names`, `Tool::tier` (`EngineTier::Core` or `EngineTier::Optional`), `Tool::install_hint`, and `Tool::known_dirs`.
   - If the engine relies on relocated data directories (like `TESSDATA_PREFIX` or `VIPSHOME`), handle it in `relocated_env` and normalize the returned `PathBuf`.
2. **Register in `packaging/engines.json`:**
   - Pin the exact `version`, HTTPS `url`, `size` (bytes), `kind`, and verified uppercase `sha256` per platform (`windows-x64`, `linux-x64`, `linux-arm64`, `osx-x64`, `osx-arm64`). Never invent or guess a checksum.
3. **Update fetch scripts & bundle tests:**
   - Add the download/extraction block to `packaging/fetch-engines.sh` and `packaging/fetch-engines.ps1` (under `full` unless it belongs in `lite`).
   - Add the expectation in `crates/core/tests/bundle.rs` (`platform_extras` or `BUNDLED`).
   - Verify with `cargo test --workspace`.

---

## 3. Adding Bilingual Localization Keys (`app/src`)

Every user-facing string in the desktop UI must support **both English (LTR) and Arabic (RTL)**.

1. **UI copy (`app/src/lib/i18n.tsx`):**
   - Add a `[english, arabic]` pair to the `COPY` table:
     ```ts
     myFeatureTitle: ['My Feature', 'ميزتي الجديدة'],
     ```
   - Use `const { t } = useI18n();` in React components (`t('myFeatureTitle')`).
2. **Backend tool & option labels (`crates/core/src/ops.rs` & `app/src-tauri/src/lib.rs`):**
   - Every `ToolDef`, `Param`, and `Choice` has both `label` (English) and `label_ar` (Arabic). Use `zl(choice)` in React to render the active language.
3. **Error diagnostics (`crates/core/src/runner.rs` / `diagnostics.rs`):**
   - Provide clear English and Arabic actionable messages for user-facing failure modes.

---

## 4. Formatting, Linting, and Verification Checklist

Run these checks before opening a pull request:

```bash
# Rust formatting, lints, and tests (root workspace)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Tauri app crate formatting and tests
rustfmt --check --edition 2021 --config-path rustfmt.toml app/src-tauri/src/*.rs app/src-tauri/build.rs
cargo test --manifest-path app/src-tauri/Cargo.toml

# LocalDrop workspace
cargo fmt --manifest-path crates/localdrop/Cargo.toml --check
cargo clippy --manifest-path crates/localdrop/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/localdrop/Cargo.toml

# Frontend TypeScript & Vitest suite
cd app
npx tsc --noEmit
npm test
npm run build
```

## Privacy & Offline Rules

- Never add networking crates (`reqwest`, `ureq`, `hyper`, etc.) to `crates/core`, `crates/cli`, `crates/localdrop`, or `app/src-tauri/Cargo.toml`. `crates/core/tests/offline.rs` enforces this automatically.
- Never make unprompted network requests on startup or during file conversion. Any optional download (such as the On-Demand Engine Manager) must require explicit user interaction and verify SHA-256 before extraction.
