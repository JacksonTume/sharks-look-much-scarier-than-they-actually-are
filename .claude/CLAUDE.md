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
cargo run --example workspace          # the scene inset in a UI pane (set_scene_rect)
cargo run --example editor             # click/drag to pick and move objects (pointer_ray)
cargo run --example scene              # articulated figures (instancing, material, primitives)
cargo run --example triangle           # the smallest consumer (Esc / close to quit)
cargo build                            # debug build
cargo build --release                  # optimized
cargo clippy --all-targets             # lint
cargo fmt --all                        # format (bare `cargo fmt` trips on examples/terrain/)
cargo test --workspace                 # UI layout/hit-testing/typography + primitive geometry
#                                        (plain `cargo test` runs only the engine crate)

# Web (wasm) — requires `cargo install wasm-bindgen-cli` once, at a version
# matching the `wasm-bindgen` dependency in Cargo.lock.
cargo xtask serve                      # build (native + wasm) + host terrain at :8080
cargo xtask serve cube                 # a specific example; also --release / --port N
# `cargo xtask serve` wraps: build the example for wasm, run wasm-bindgen into
# web/pkg/ as app.js, and serve web/ from a built-in static server (xtask/).

# Photograph a demo, deterministically — the mechanical half of "run it and look
# at it". Needs Xvfb + ImageMagick + xdotool (the session-start hook installs
# them). Output lands in capture/ and is gitignored.
cargo xtask shoot workspace                    # one PNG at frame 120
cargo xtask shoot terrain --frames 400 --size 1280x720
cargo xtask shoot workspace --script capture/workspace.script   # clicks between shots

# Type-check the wasm target without packaging
cargo build --target wasm32-unknown-unknown --lib

# Re-bake the font atlas. Runs by hand, roughly never; its output
# (slmsttaa-ui/src/font/{atlas.bin,metrics.rs}) is committed and reviewed.
cargo run -p fontbake --release
cargo run -p fontbake --release -- --preview atlas.pgm   # dump a viewable atlas
```

Logging honors `RUST_LOG` (e.g. `RUST_LOG=slmsttaa=debug`); on the web it goes to
the browser console.

## Verifying changes

Tests live in the places that don't need a GPU: `slmsttaa-ui/tests/` (the
zero-dependency toolkit, via the `RecordingPainter` double), and inside the
engine wherever the logic is pure — `src/renderer/primitives.rs` (mesh builders
are CPU geometry), `src/renderer/graph.rs` (pass ordering), `src/camera.rs` and
`src/renderer/mod.rs` (the cursor→NDC mapping picking rests on), `src/time.rs`,
and `src/input.rs` (the keyboard's press-edge, auto-repeat and event-ordering
rules — the winit→engine translation is split from the accumulation precisely so
the accumulation half is reachable without a window). Everything else owns a
surface or a device and is verified by building and looking at it.

Tests constrain but do not replace looking at the screen: four separate bugs (UI
Slices 1, 3, 5 and 7) passed the whole suite and were caught by running the demo.
The last one is the sharpest argument for the habit — every test in the toolkit
passed because the toolkit believed what the host told it, and the *host* was
wrong (Windows reports `text: Some("a")` for `Ctrl+A`, so "select all" typed an
`a`).
The reverse also happens — two primitive bugs (an inverted pole degeneracy, a
zero-length capsule emitting degenerate triangles) looked *fine* in a still frame
and were caught by the outward-winding assertion. To confirm a change works:

- **Always** `cargo build` (native) **and** `cargo build --target
  wasm32-unknown-unknown --lib` — the two targets diverge via `#[cfg]`, so one
  can break while the other compiles.
- `cargo test --workspace` for anything touching UI layout, hit-testing, text
  metrics, or the primitive mesh builders. **`--workspace` matters**: the engine
  is the root package, so a bare `cargo test` skips `slmsttaa-ui` entirely.
- For visual changes, run the native example (`cargo run --example triangle`)
  and/or rebuild the wasm package and hard-refresh the browser. The dev server
  serves `web/` live; no restart needed after a rebuild.
- **`cargo xtask shoot <example>` when you cannot see a screen**, or when you want
  a before/after you can diff. It pins the frame clock, so two runs of the same
  commit are pixel-identical and `compare -metric AE a.png b.png` is a real
  answer rather than noise — `terrain` differed by 0.6% between hand-taken
  screenshots and by zero through the harness. A `--script` adds clicks at exact
  frames; `capture/workspace.script` is Slice 19's picking check written down.
  It does **not** replace looking: it proves pixels did not move, which is a
  different claim from "this is right", and every bug in the list above was found
  by a person noticing something was wrong rather than merely different.

## Conventions

- `web/pkg/` is a build artifact (`web/pkg/.gitignore` ignores its contents) —
  never edit or commit what `wasm-bindgen` emits there.
- Keep native/web parity: anything touching instance/adapter/device/surface/event
  loop likely needs a matching `#[cfg(target_arch = "wasm32")]` branch.
- Match the surrounding rustdoc style — modules and public items are documented;
  keep that up.
- Prefer keeping `wgpu` reasonably current (browsers track the live WebGPU spec).
- **Where UI goes.** Widgets, layout, theming, typography, and interaction belong
  in `slmsttaa-ui/`, and so do their docs — do not add them to `ARCHITECTURE.md`
  or the engine `ROADMAP.md`, which keep only the engine half (the overlay pass,
  the atlas upload, the `Painter` seam). `slmsttaa-ui` must stay
  **zero-dependency**: it never imports `wgpu`, `winit`, or the engine crate. When
  the toolkit needs something the painter can't draw, widen the `Painter` trait and
  implement it in `renderer/overlay.rs` — never reach through to renderer
  internals. Check with `cargo tree -p slmsttaa-ui`, which must print exactly one
  line.
- **Text metrics have exactly one home.** `slmsttaa_ui::font` — never a `Painter`
  method, and never a widget's own arithmetic. Two implementations of "how wide is
  this string" agreed for four slices only because the font was a monospace grid,
  and would have diverged silently the moment it wasn't: the tests measure through
  `RecordingPainter`, so a divergence shows up as a green suite and a broken
  screen. Likewise, a run is **not** `px` tall — use `font::line_height` to size a
  row and `font::centered_top` to centre one, never `(h - px) / 2`.
- **`fontbake/` is the only crate allowed a font rasterizer**, and its output is
  committed. Don't add `fontdue` (or any rasterizer) to `slmsttaa-ui`, the engine,
  or `xtask` — including as a `build-dependency`, which would still show up in the
  dependency graph and break the claim above.
- Anything the engine ships as a widget must be re-implementable by a demo from
  public API alone. A widget with no demo roadblock behind it is polish; label it
  as such rather than filing it as infrastructure.

- Web pixels cannot be checked in a cloud session: headless Chrome captures a
  blank canvas from a WebGPU surface (verified with unmodified code as a
  control). The web half of the Definition of Done is therefore a *build and
  boot* check there — say so in the roadmap rather than implying a picture was
  looked at.

## Gotchas (quick reference)

- Web uses `event_loop.spawn_app(app)`, native uses `run_app`. `run_app` on the
  web throws a control-flow exception.
- The web canvas backing size must be set explicitly and resynced when the async
  renderer arrives, or the surface is 1x1.
