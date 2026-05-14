# AGENTS.md

## Project

Phire (Phi Recorder) — a Rust rhythm game. Multi-platform: Windows, Android, iOS, WASM.

## Workspace

| Crate | Role |
|-------|------|
| `phire` | Core library: game engine, chart parsing, UI primitives, rendering |
| `phire-ui` | UI layer: online features, login, charts view, scenes. Builds as `cdylib` for Android JNI |
| `phire-main` | Thin binary entry — calls `phire_ui::quad_main()` |
| `prpr-avc` | Audio/video codec support (links native static libs) |
| `prpr-pbc` | CLI chart format converter: `prpr-pbc <input> <output>` |

## Key Commands

```bash
# Build (desktop)
cargo build

# Build (release, strips symbols)
cargo build --release

# Build WASM
./build_wasm.sh          # requires wasm-bindgen CLI, wasm32-unknown-unknown target

# Chart converter
cargo run -p prpr-pbc -- <input_chart> <output.pbc>
```

No test suite, no CI, no lint/typecheck scripts exist in this repo.

## Build Quirks

- `phire-ui/build.rs` runs `dotenv_build` — a `.env` file at workspace root is expected. Without it, build may fail or use defaults. No `.env.example` is checked in.
- `prpr-avc/build.rs` looks for native static libs via `PRPR_AVC_LIBS` env var, falling back to `<crate>/static-lib/<TARGET>/`. The `static-lib` dir is gitignored.
- `.cargo/config.toml` hardcodes Android NDK paths (`/home/hlmc/android-ndk-r27c/...`). These are developer-machine-specific; override or remove for other environments.
- Custom forks: `macroquad` and `miniquad` are pinned to `github.com/2278535805/prpr-*`. Audio via `sasa` from `github.com/2278535805/sasa`.

## Formatting

`rustfmt.toml`: `max_width = 150`, `fn_call_width = 150`. Run `cargo fmt` before committing.

## Architecture Notes

- Entry flow: `phire-main/src/main.rs` → `phire_ui::quad_main()` (exported as `extern "C"` for JNI) → `the_main()` async loop.
- The `closed` feature gates `inner.rs` modules in both `phire` and `phire-ui` — this is proprietary/encrypted resource loading code. It will not compile without the file (`inner.rs` is gitignored).
- `phire/src/bin.rs` is NOT a binary — it's the binary chart format serializer/deserializer module.
- Global mutable state: `phire-ui` stores `DATA` as `static mut Option<Data>` — accessed via `get_data()`/`get_data_mut()` unsafe wrappers.
- Platform conditionals are pervasive (`cfg(target_os = "android")`, `cfg(target_arch = "wasm32")`, etc.). Check all three platform blocks when modifying shared code.
- Anti-addiction system (`aa` feature) is Chinese regulatory compliance — JNI callbacks to Android Java layer.
- Localization: `phire/src/l10n.rs` with Fluent `.ftl` files; locale auto-detected via `sys-locale`.

## File Conventions

- `phire/src/core/` — game engine core (chart model, animation, judge lines, notes)
- `phire/src/parse/` — chart format parsers (Phigros, RPE, PEC)
- `phire/src/scene/` — game scenes (main menu, gameplay, etc.)
- `phire/src/ui/` — UI widget library
- `phire-ui/src/page/` — UI pages
- `phire-ui/src/scene/` — UI scene implementations
- `phire-ui/src/client/` — online API client
