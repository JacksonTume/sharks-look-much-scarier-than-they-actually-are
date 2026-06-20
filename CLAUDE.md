# CLAUDE.md

Guidance for working in this repository.

## What this is

**SLMSTTAA** ("Sharks Look Much Scarier Than They Actually Are") — a small Rust
3D rendering engine on `wgpu` (WebGPU) + `winit`. Builds **native** (desktop) and
**web** (wasm/WebGPU) from one codebase. Crate name `slmsttaa`; demo binary
`slmsttaa-demo`.

Read [`ARCHITECTURE.md`](ARCHITECTURE.md) before changing the init/render flow —
it documents the cross-platform gotchas (web `spawn_app`, canvas sizing, backend
selection, wgpu/spec drift) that are easy to reintroduce.

## Commands

```sh
# Native
cargo run --bin slmsttaa-demo          # run the demo (Esc / close to quit)
cargo build                            # debug build
cargo build --release                  # optimized
cargo clippy --all-targets             # lint
cargo fmt                              # format

# Web (wasm) — requires `cargo install wasm-pack` once
wasm-pack build --target web --out-dir web/pkg
python -m http.server -d web 8080      # then open http://localhost:8080

# Type-check the wasm target without packaging
cargo build --target wasm32-unknown-unknown --lib
```

Logging honors `RUST_LOG` (e.g. `RUST_LOG=slmsttaa=debug`); on the web it goes to
the browser console.

## Verifying changes

There are no tests yet. To confirm a change works:

- **Always** `cargo build` (native) **and** `cargo build --target
  wasm32-unknown-unknown --lib` — the two targets diverge via `#[cfg]`, so one
  can break while the other compiles.
- For visual changes, run the native binary and/or rebuild the wasm package and
  hard-refresh the browser. The dev server serves `web/` live; no restart needed
  after a rebuild.

## Conventions

- `web/pkg/` is a build artifact (wasm-pack writes its own `.gitignore` there) —
  never edit or commit its contents.
- Keep native/web parity: anything touching instance/adapter/device/surface/event
  loop likely needs a matching `#[cfg(target_arch = "wasm32")]` branch.
- Match the surrounding rustdoc style — modules and public items are documented;
  keep that up.
- Prefer keeping `wgpu` reasonably current (browsers track the live WebGPU spec).

## Gotchas (quick reference)

- Web uses `event_loop.spawn_app(app)`, native uses `run_app`. `run_app` on the
  web throws a control-flow exception.
- The web canvas backing size must be set explicitly and resynced when the async
  renderer arrives, or the surface is 1x1.
