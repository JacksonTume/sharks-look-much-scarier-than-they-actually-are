# Roadmap

This document records *where SLMSTTAA is going and how we get there*. It is the
counterpart to [`ARCHITECTURE.md`](ARCHITECTURE.md): that one explains how the
code works today, this one explains the destination and the method.

It is deliberately a roadmap of **sequenced capability**, not dates. The horizon
is long (months to years, solo), so the order matters far more than any schedule.

## The goal

SLMSTTAA should be an **easy way to do cool 3D things**, while the engine absorbs
all the under-the-hood GPU, windowing, and cross-platform work.

The litmus test: a developer who wants to build, say, procedurally generated
terrain with hydro-thermal erosion should make **a few API calls**, write their
*algorithm*, and never touch `wgpu`, `winit`, surfaces, or event loops. They worry
about the terrain; the engine worries about the pixels.

This is not a goal to "finish" — it's a direction. Success is measured by one
*vertical* being shockingly easy at a time, not by feature breadth. We are not
trying to out-feature Bevy. We are trying to make a specific cool thing trivial,
then another, then another.

## Guiding principles

These are load-bearing. When a decision is unclear, it should be resolved by
appeal to one of these.

### 1. The engine is decoupled from its consumers

The engine must **not know or care who implements against it**. A demo (terrain,
water, whatever) is a *separate program* that USES the engine as a library — never
content baked into the engine.

Because `winit` + the wasm constraint (`spawn_app` throws control flow at the
browser; you cannot block the main thread — see `ARCHITECTURE.md`) force the
engine to own the event loop, decoupling is achieved by **inversion of control**:
the consumer implements a trait (e.g. `Application` with `init`/`update`) and the
engine calls *into* it. The engine sees `dyn Application` and nothing more.

This is **enforced**, not merely intended: demos live in Cargo `examples/`, which
compile as separate crates that can only see the public API. If a demo can't be
written from public items, the boundary has leaked and the build fails.

### 2. Demo-first / outside-in — "the example is the spec"

We build the demo first. When it hits a roadblock, *that roadblock is the API
gap*. We then add to the engine **only what was demanded** — no speculative
features, no system built before a real consumer needs it. This is the antidote
to drifting into rebuilding a worse Bevy.

### 3. At every roadblock: classify engine-shaped vs. demo-shaped

When a wall forces a change, ask: *would another consumer (a water demo, a voxel
demo) want this too?* Push **only the generic plumbing** down into the engine;
keep content and algorithms up in the demo.

- **Engine:** mesh upload, depth buffering, camera, resize — anything touching
  `wgpu`/`winit`/the GPU.
- **Demo:** heightmap generation, the erosion algorithm, "make it look like
  terrain."

Never shove a demo-specific hack into the engine just to unblock — that re-couples
it.

### 4. KISS = smallest public surface that holds the boundary

Keep it simple — but "simple" means the *smallest public API that preserves
decoupling*, not "no abstraction." A hack that lets the demo touch a
`wgpu::Buffer` feels simpler but breaks principle 1, so it is not actually the
simple choice. The demo never sees `wgpu`/`winit` types, even if that costs a thin
wrapper.

### 5. Always keep something on screen

Momentum is the scarce resource on a long solo build. Bias every chunk of work
toward a visible result. Architecture you can't see yet is where motivation goes
to die.

### 6. Pay the documentation tax

Every module gets real rustdoc; hard-won cross-platform gotchas go in
`ARCHITECTURE.md`; this roadmap stays current. Future-you forgets everything — the
docs are what let a session resume in minutes after a gap instead of giving up.

## Definition of done (every slice)

A slice is not finished until:

- It **builds on native** (`cargo build`) **and** wasm
  (`cargo build --target wasm32-unknown-unknown --lib`) — the targets diverge via
  `#[cfg]`, so both must pass.
- `cargo clippy --all-targets` is clean and `cargo fmt` has been run.
- The driving demo runs and shows the new capability on screen.
- Any new public API has rustdoc; `ARCHITECTURE.md` is updated if the
  init/render flow changed.
- The engine still contains **zero** consumer-specific content.

## The slices

The driving vertical is the **terrain + erosion demo**. Each slice is pulled into
existence by the next thing that demo cannot do.

### Slice 0 — Invert control (bootstrapping)

*Roadblock:* you cannot run **any** consumer at all today — `run()` *is* the demo
and the triangle is baked into `renderer/mod.rs`.

- Add an `Application` trait (`init`/`update`) and a `run(app)` entry point; the
  engine owns the loop and calls into the consumer.
- Move the demo triangle **out** of the engine into `examples/` (the smallest
  possible consumer — clear the screen / draw the existing triangle via the new
  API).
- The engine no longer knows about any geometry.

*Proof:* an example written against only the public API renders, on native and
web.

### Slice 1 — Mesh + indexed drawing ✅ done

*Roadblock:* a terrain grid is thousands of shared vertices; you cannot hard-code
it, and the current pipeline has no index buffer.

- Public `Mesh` (vertices + indices) that the consumer builds CPU-side and hands
  over; the engine uploads it.
- A scene / draw-list the renderer iterates, replacing the single baked buffer.

*Proof:* `Mesh` (`src/renderer/mesh.rs`) + `Renderer::set_meshes` upload a vertex
+ index buffer per mesh; `render()` iterates the draw-list with `draw_indexed`.
The cube demo (below) builds an indexed cube — 8 shared corners, not 36 vertices.

### Slice 2 — Depth buffer + culling ✅ done

*Roadblock:* real 3D geometry renders with wrong occlusion — `depth_stencil` is
currently `None`.

- A depth texture, depth testing, and back-face culling once geometry is solid.

*Proof:* `cargo run --example cube` shows a tumbling solid cube whose near faces
occlude far ones and whose inward back faces are culled. (A spinning cube was
chosen over a tilted grid as the clearer combined proof of indexed drawing +
depth + culling; the real procedural terrain grid still arrives in Slice 4.)

### Slice 3 — Camera the consumer can drive ✅ done

*Roadblock:* you can't look *at* the terrain — the camera is fixed.

- Input-driven orbit/fly camera, exposed through the engine so the consumer
  controls the viewpoint without touching `winit` events directly.

*Proof:* `cargo run --example grid` shows an orbitable height-mapped terrain grid:
drag the left mouse button (or use the arrow keys) to orbit, scroll to zoom. The
engine gained a winit-free `Input` (`src/input.rs`, engine `Key`/`MouseButton`
enums) read via `Renderer::input()`, plus `Renderer::camera_mut()` /
`Camera::look_from_to` to aim the camera — the *orbit math lives in the demo*, the
engine only exposes input and the camera. (No `OrbitController` was pushed into the
engine: a single consumer doesn't justify one yet — demo-first / KISS.) Delta-time
was deliberately deferred until a demo needs frame-rate-independent simulation.

### Slice 4 — The terrain vertical (the thesis)

*Roadblock:* none left — this is the payoff that proves the goal.

- Demo generates a procedural heightmap grid `Mesh` (in the demo).
- The per-frame `update` hook advances the erosion and mutates vertex heights;
  the engine re-uploads and redraws. The algorithm is based on Tzathas et al.,
  *Physically-based analytical erosion for fast terrain generation* (Computer
  Graphics Forum 43(2), Eurographics 2024; `reference/Analytical_Terrains_EG.pdf`)
  — analytical solutions of the stream power law where **time is a parameter**
  (advance `t` per frame and re-evaluate), not a long simulation. It stays in the
  **demo**, not the engine.
- Shading is pulled in here on demand: start with height-based vertex color
  (KISS), and only add normals + simple diffuse lighting when "it looks flat"
  becomes the next roadblock.

*Proof:* terrain visibly erodes over time, written as a handful of engine calls
plus the consumer's own algorithm — `wgpu`/`winit` nowhere in sight.

### Slice 5 — On-screen UI: a debug/HUD text overlay

*Roadblock:* the scene now moves and changes — you orbit the camera (Slice 3) and
the terrain erodes (Slice 4) — but you can't *see* any of it as numbers. There is
no way to draw in screen space: every vertex goes through the 3D camera transform,
and the only "UI" so far is the gallery's DOM buttons, which are a `web-sys` hack
that **doesn't exist on native at all** (see the gallery's auto-cycle fallback).

- A 2D **screen-space overlay**: the engine's first *second pass*, drawn after the
  scene — orthographic, depth-test off, composited on top.
- **Text rendering** the consumer can call without touching `wgpu`: a glyph atlas
  + textured quads behind a small API (something like `renderer.draw_text(text,
  screen_pos)`; the exact surface is decided at the roadblock, KISS).
- Engine-drawn, so it renders identically on **native and web** — unlike the DOM
  buttons, this finally gives native real on-screen UI.

*Proof:* the grid/terrain demo shows a live HUD (e.g. FPS, camera
yaw/pitch/distance, erosion iteration count) over the 3D scene, on both targets.

*Why text first:* it's the smallest step that holds the boundary — text before
interactive widgets. The engine gets the overlay pass + glyph plumbing (generic);
the demo decides *what* to display (content). Clickable widgets — which would let
us retire the gallery's DOM-button hack and give native a real scene-switcher —
and any layout/widget model are **later** UI slices, each pulled by its own demo
roadblock. The goal is never a general retained-mode toolkit (that's the "worse
Bevy" trap, principle 2); it's the smallest UI each demo actually demands. This
slice also opens the **render-graph** seam below (it's the first time there's more
than one pass).

## Beyond (seams, not commitments)

Listed only so we recognize them when a future demo demands them — **not** to be
built ahead of need: a material abstraction, multiple meshes with transforms,
MSAA, basic lighting model, interactive UI widgets (clickable regions / sliders,
building on Slice 5's text overlay), and a render graph once there's more than one
pass. Each waits for a consumer to ask.
