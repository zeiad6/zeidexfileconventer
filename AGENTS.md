# AGENTS.md

Project memory for agents working in this repository. Facts below are checked against the
code; when a command changes, update this file in the same commit.

## What this is

Zeidex Convert — a cross-platform file converter: a Tauri v2 desktop app (React + TypeScript
front end, Rust back end) plus a headless CLI. Documents, ebooks, images, audio, video and PDFs
are routed to external engines (FFmpeg, libvips, Ghostscript, qpdf, Pandoc, LibreOffice,
Calibre, Tesseract, whisper.cpp, Real-ESRGAN, ZBar, qrencode, ExifTool, rhwp).

## Layout

| Path | What lives there |
|------|------------------|
| `crates/core` | Format registry, `Op` model, job planning, engine runner. **Zero dependencies**, builds and tests offline. |
| `crates/cli` | `zeidex-cli`: `convert`, `describe`, `doctor`, `tool` subcommands. |
| `crates/localdrop` | Encrypted local file handoff to a phone. Own workspace (see below). |
| `app/` | Vite + React + TypeScript UI (`src/wheel`, `src/shelf`, `src/home`, `src/editor`, `src/jobs`), vitest suites under `test/`. |
| `app/src-tauri` | The Tauri shell: commands (`describe`, `convert`, `doctor`, `install_engine`, drag hook), window setup. **Own workspace.** |
| `packaging/` | `fetch-engines.sh` (macOS/Linux) and `fetch-engines.ps1` (Windows) stage engines into `app/src-tauri/engines/`; `engines.json` pins every URL, version, size and SHA-256. `make-icons.py` renders all app icons and the NSIS artwork from `app/src-tauri/icons/icon.svg`. |
| `android/` | Git submodule (separate repository). Leave the pointer alone unless the task is Android. |
| `docs/AUDIT.md`, `specs/` | Design and audit notes that explain why engines are bundled or optional. |

Two workspaces by design: the root `[workspace]` is `crates/core` + `crates/cli` (no
dependencies, so they compile offline); `app/src-tauri` and `crates/localdrop` are separate
workspaces because they pull in the GUI and crypto graphs. `cargo test` at the root does **not**
cover them.

## Commands

| Command | Purpose |
|---------|---------|
| `cargo fmt --all --check` | Formatting for the root workspace |
| `rustfmt --check --edition 2021 --config-path rustfmt.toml app/src-tauri/src/*.rs app/src-tauri/build.rs` | Formatting for the app crate (not in the root workspace) |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints, warnings are errors |
| `cargo test --workspace` | Core + CLI tests |
| `cargo check --manifest-path app/src-tauri/Cargo.toml` | Compile the Tauri shell |
| `cargo test --manifest-path app/src-tauri/Cargo.toml` | App-crate tests |
| `cd app && npx tsc --noEmit` | Type check the UI |
| `cd app && npx vitest run` | UI tests (~70 s; the wheel suite waits out a real 3.4 s idle timer) |
| `cd app && npm run build` | `tsc --noEmit` then `vite build` |
| `bash packaging/fetch-engines.sh` | Stage Unix engines (set `ZEIDEX_ENGINE_COMPONENTS` to fetch a subset) |
| `python3 packaging/make-icons.py` | Re-render every app icon (PNG/ICO/ICNS) and the NSIS header/sidebar from `app/src-tauri/icons/icon.svg`, using the `icon-small.svg` optical variant for 16–24 px; needs `rsvg-convert` |

### Building the Windows installer from Linux

Windows bundles are normally built on `windows-latest`. On a Linux host the same exe and NSIS
installer can be produced with `cargo-xwin` plus the system `lld-link`/`llvm-lib`/`clang-cl`
(`x86_64-pc-windows-gnu` is not viable: the WebView2 static lib is MSVC-only).

```
cd app/src-tauri && ZEIDEX_CROSS_RESOURCES=1 CARGO_BUILD_JOBS=3 \
  cargo xwin build --release --target x86_64-pc-windows-msvc
cd app && ./node_modules/.bin/tauri bundle --target x86_64-pc-windows-msvc --bundles nsis
```

`ZEIDEX_CROSS_RESOURCES=1` is required for an artifact: without it `build.rs` takes the
cross-check path and the exe ships with no icon, version block or manifest. `tauri bundle`
(not `tauri build`) is used because it does not recompile, so the exe it packages is the one
built with the resources. Bundling needs a host NSIS 3.x (`makensis`, `/usr/share/nsis`); the
installer is unsigned and must still be smoke-tested on a real Windows machine — a successful
cross build proves it compiles and packs, not that it runs.

The Windows engine tree has to be staged from `packaging/fetch-engines.ps1` on Linux
(`msiextract` for the MSIs, `7z` for the zips, micromamba `--platform win-64` for the conda
packages) and then packed exactly like CI does: `7z a -t7z -mx=9 -md=64m -ms=256m -mmt=2
engines.7z . -x!7z.exe -x!7z.dll -x!engines.7z`, leaving only `7z.exe`, `7z.dll` and the
archive beside it for `windows/hooks.nsh`. `packaging/fetch-engines.sh` then has to be re-run
to restore the Unix tree, because both share `app/src-tauri/engines/`.

CI (`.github/workflows/build.yml`) runs fmt, clippy, `cargo test --workspace`, the app crate
checks, `tsc`, vitest and the bundles on `ubuntu-24.04` + `windows-latest`. Keep a change green
against those steps before calling it done.

## How conversion works

1. `crates/core/src/formats.rs` maps extensions to formats.
2. `crates/core/src/ops.rs` defines `Op` (what the user asked for) and `TOOLS`, the ring the UI
   draws. `engine_for` / `ready` / `available` / `convert_ready` decide which tools and targets
   are offered — a tool with no engine is hidden, not shown and failed.
3. `crates/core/src/plan.rs` turns an `Op` plus inputs into a `Plan` of command steps.
4. `crates/core/src/runner.rs` executes the plan and surfaces `MissingTool` instead of a panic.
5. `crates/core/src/tools.rs` is the lookup registry: search order, per-platform `known_dirs`,
   bundled layouts (`bin`, `Library/bin`, `program`, `Contents/MacOS`, nested `*.app`), install
   hints and relocation env.

Adding an operation means touching all five: a format or `Op` variant, a `ToolDef` entry, an
`engine_for` mapping, a planner branch, and a runner `require(...)`. A new engine also needs an
entry in `packaging/engines.json` with a verified SHA-256 and a block in both fetch scripts, or
`crates/core/tests/bundle.rs` fails.

## Engine rules

- `app/src-tauri/engines/` is generated and gitignored; never commit engine binaries.
- Every download is pinned by URL + version + size + SHA-256 in `packaging/engines.json`. If a
  hash cannot be verified by an actual download or an official digest, the entry does not ship.
  Never invent a hash to make a script run.
- Windows bundles LibreOffice and Calibre (`msiexec /a` unpacks the MSI into the real
  `program/` + `share/` layout); Unix leaves them optional because their archives are 300–400 MB
  and are not relocatable. `rembg` is never bundled (needs a Python runtime); only its `u2net`
  model ships.
- The Windows fetch script packs the staged tree into `engines/engines.7z` when
  `ZEIDEX_PACK_ENGINES=1` (CI sets it; a plain run stays loose for `tauri dev`). NSIS cannot map
  a data block past 2 GB, and `app/src-tauri/windows/hooks.nsh` unpacks the archive during setup.
  Keep `7z.exe` and `7z.dll` out of the archive: the hook needs them.
- The app never downloads anything at run time. An engine that is absent is either bundled or
  installed by the user through the Engines panel's `install_engine` command.

## Conventions

- Rust: `rustfmt.toml` at the root governs both workspaces; no `unwrap()` on fallible I/O in
  library code, return `Result` and let `MissingTool` carry the tool name and install hint.
- TypeScript: strict, no `any` in new code; Tauri calls go through `app/src/lib/api.ts`, never
  `invoke()` from a component.
- Comments explain *why* a thing is done (a platform quirk, a workaround), not what the line does.
- Tests: a bug fix gets a regression test beside the unit that broke; engine expectations belong
  in `crates/core/tests/bundle.rs`.
- Do not commit, tag, build installers, or push unless the user asks for it in the current
  conversation.
