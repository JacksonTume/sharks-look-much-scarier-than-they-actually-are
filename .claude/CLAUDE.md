# CLAUDE.md

Guidance for working in this repository.

## What this is

**SLMSTTAA** ("Sharks Look Much Scarier Than They Actually Are") — a small Rust
3D rendering engine on `wgpu` (WebGPU) + `winit`. Builds **native** (desktop) and
**web** (wasm/WebGPU) from one codebase. Crate name `slmsttaa`; consumers
implement the `Application` trait and call `run(app)`. Demos live in `examples/`
(separate crates that see only the public API) — the `triangle` example is the
reference consumer.

Read [`ARCHITECTURE.md`](../ARCHITECTURE.md) before changing the init/render flow —
it documents the cross-platform gotchas (web `spawn_app`, canvas sizing, backend
selection, wgpu/spec drift) that are easy to reintroduce.

Read [`ROADMAP.md`](../ROADMAP.md) before adding features — it records the goal (an
easy API for cool 3D, with the engine hiding all GPU/windowing plumbing), the
guiding principles (engine decoupled from consumers via inversion of control;
demo-first/outside-in; push only generic plumbing into the engine; KISS), and the
demand-driven slice sequence. New work should be pulled into existence by a demo
roadblock, never added speculatively.

**UI work is a separate track.** The immediate-mode toolkit lives in its own
zero-dependency crate, [`slmsttaa-ui`](../slmsttaa-ui/README.md), with its own
[`ROADMAP.md`](../slmsttaa-ui/ROADMAP.md). Read those before touching anything
UI-shaped — and see the placement rule under *Conventions*.

## Commands

```sh
# Native
cargo run --example terrain            # the capstone: layered Perlin + stream-power erosion
cargo run --example triangle           # the smallest consumer (Esc / close to quit)
cargo build                            # debug build
cargo build --release                  # optimized
cargo clippy --all-targets             # lint
cargo fmt --all                        # format (bare `cargo fmt` trips on examples/terrain/)
cargo test -p slmsttaa-ui              # the tests (UI layout + hit-testing; see below)

# Web (wasm) — requires `cargo install wasm-bindgen-cli` once, at a version
# matching the `wasm-bindgen` dependency in Cargo.lock.
cargo xtask serve                      # build (native + wasm) + host terrain at :8080
cargo xtask serve cube                 # a specific example; also --release / --port N
# `cargo xtask serve` wraps: build the example for wasm, run wasm-bindgen into
# web/pkg/ as app.js, and serve web/ from a built-in static server (xtask/).

# Type-check the wasm target without packaging
cargo build --target wasm32-unknown-unknown --lib
```

Logging honors `RUST_LOG` (e.g. `RUST_LOG=slmsttaa=debug`); on the web it goes to
the browser console.

## Verifying changes

The only tests live in `slmsttaa-ui/tests/` — the zero-dependency toolkit is the
one part of the repo testable without a GPU, via the `RecordingPainter` double.
The engine half is still verified by building and looking at it. To confirm a
change works:

- **Always** `cargo build` (native) **and** `cargo build --target
  wasm32-unknown-unknown --lib` — the two targets diverge via `#[cfg]`, so one
  can break while the other compiles.
- `cargo test -p slmsttaa-ui` for anything touching UI layout or hit-testing.
- For visual changes, run the native example (`cargo run --example triangle`)
  and/or rebuild the wasm package and hard-refresh the browser. The dev server
  serves `web/` live; no restart needed after a rebuild.

## Conventions

- `web/pkg/` is a build artifact (`web/pkg/.gitignore` ignores its contents) —
  never edit or commit what `wasm-bindgen` emits there.
- Keep native/web parity: anything touching instance/adapter/device/surface/event
  loop likely needs a matching `#[cfg(target_arch = "wasm32")]` branch.
- Match the surrounding rustdoc style — modules and public items are documented;
  keep that up.
- Prefer keeping `wgpu` reasonably current (browsers track the live WebGPU spec).
- **Where UI goes.** Widgets, layout, theming, and interaction belong in
  `slmsttaa-ui/`, and so do their docs — do not add them to `ARCHITECTURE.md` or
  the engine `ROADMAP.md`, which keep only the engine half (the overlay pass, the
  glyph atlas, the `Painter` seam). `slmsttaa-ui` must stay **zero-dependency**:
  it never imports `wgpu`, `winit`, or the engine crate. When the toolkit needs
  something the painter can't draw, widen the `Painter` trait and implement it in
  `renderer/overlay.rs` — never reach through to renderer internals.
- Anything the engine ships as a widget must be re-implementable by a demo from
  public API alone. A widget with no demo roadblock behind it is polish; label it
  as such rather than filing it as infrastructure.

## Gotchas (quick reference)

- Web uses `event_loop.spawn_app(app)`, native uses `run_app`. `run_app` on the
  web throws a control-flow exception.
- The web canvas backing size must be set explicitly and resynced when the async
  renderer arrives, or the surface is 1x1.
