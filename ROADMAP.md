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

### Slice 4 — The terrain vertical (the thesis) ✅ done

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

*Proof:* `cargo run --example terrain` erodes a procedural heightmap and lets you
explore the time continuum with a slider (faint post-process → steady-state
mountain range). The whole algorithm lives in the demo
(`examples/terrain/erosion.rs`): a Priority-Flood river network with depression
breaching, drainage-area accumulation, the 1D analytical stream-power solution
evaluated down each river tree (advection origin `D` + uplift integral `S`),
driven to a fixed point and accelerated by a multigrid V-ramp, plus the paper's
hillslope (Eqn. 26) and thermal (Eqns. 28–29) terms and the §4.3 slope
correction. Normals + diffuse lighting were indeed pulled in (it *did* look flat)
— but **CPU-baked into vertex color in the demo**, so the engine's position+color
pipeline stays untouched (principle 3). `wgpu`/`winit` are nowhere in the demo.

### Slice 5 — On-screen UI: a debug/HUD text overlay ✅ done

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

*Proof:* the terrain demo shows a live HUD (FPS, grid size) **and** a full
parameter panel over the 3D scene, on native and web. The engine gained: a
screen-space overlay pass (`src/renderer/overlay.rs`) — the first *second pass*,
loading rather than clearing the color target, depth off, alpha-blended; an
embedded bitmap font baked into a glyph atlas (then `src/renderer/font.rs`, the
public-domain `font8x8`, no font file or rasterizer dependency — replaced in UI
Slice 5 by a distance-field bake of Inter that the *toolkit* owns); and a frame
clock
(`src/time.rs`, `Renderer::dt`, wasm-safe via `performance.now()`).

*The interactive-UI step came with it.* The driving demo didn't just need to
*display* numbers — it needed to *edit* erosion parameters, which is the natural
"clickable widgets" roadblock. So this slice also delivered a small **modular,
decoupled immediate-mode UI framework** (then `src/ui.rs`, since extracted into
the `slmsttaa-ui` crate — see *The UI split* below): widgets (`slider`,
`button`, `checkbox`, `label`, `title`) that edit a consumer's own `&mut`
values. It is decoupled twice over: downward from the renderer via the [`Painter`]
trait (the UI never sees `wgpu` — the overlay is just one `Painter` impl), and
upward from the consumer (the UI knows nothing of erosion; parameters live in the
demo). It stays immediate-mode with a tiny persistent `UiState` — deliberately
*not* a retained-mode toolkit (the "worse Bevy" trap, principle 2); it's the
smallest UI the demo actually demanded. Input grew an absolute cursor position and
press-edge query to support hit-testing. This slice also opened the
**render-graph** seam below (it's the first time there's more than one pass).

### Slice 6 — Layered terrain rebuild + wireframe render mode ✅ done

*Roadblock:* the analytical erosion from Slice 4 was impressive but a black box —
one monolithic solver you couldn't peel apart, inspect, or extend a layer at a
time, and its results "left a lot to be desired" without an obvious knob to fix.
The lesson: terrain is better *composed* than solved. And to debug any of it you
need to see the underlying grid, which the solid renderer can't show.

This slice deliberately **replaces** the Slice 4 analytical solver with an
explicit, demand-driven layer stack — same demo, rebuilt the way the principles
say it should have been: smallest visible step at a time.

- **Engine — a portable wireframe `RenderMode`.** The first roadblock was "I can't
  see the mesh." The engine gained a `RenderMode` (solid / wireframe) toggled via
  `Renderer::set_render_mode`. Crucially it is drawn with **line-list topology**
  from a deduplicated edge buffer derived at upload — *not* `PolygonMode::Line`,
  which needs a feature WebGL2 lacks and would break native/web parity. This is the
  generic-plumbing half of the roadblock (principle 3): every consumer wants to
  inspect geometry, so it belongs in the engine; what to draw stays in the demo.
- **Demo — layer 1, the base shape.** A fractal Perlin-noise heightmap
  (`terrain/heightmap.rs`). On its own: rolling hills, recognizable but lifeless.
- **Demo — layer 2, erosion on top.** Iterative **stream-power** erosion
  (`terrain/erosion.rs`): each timestep routes flow to every cell's lowest neighbor
  (priority-flood, depressions filled), accumulates *drainage area* down the
  network, and incises by the stream power `K·Aᵐ` (the stable implicit FastScape
  update), with an optional thermal/talus pass. Because erosion scales with
  accumulated area, water concentrates into shared trunk valleys — which is what
  produces the **dendritic** ridge/valley networks the reference papers show.
  (An earlier droplet-hydraulic attempt was abandoned: independent droplets never
  pool into a connected network, so it just roughened the noise instead of carving
  valleys. The flow-accumulation model is what both references actually use.)
  Unlike Slice 4's time-as-a-parameter solve, this is an honest
  accumulate-many-small-steps simulation — each layer independently tunable.

The UI was improved *alongside* (not as a separate project): the parameter panel
gained section headings and a titled header, grouping the per-layer knobs.

*Proof:* `cargo run --example terrain` shows a Perlin terrain carved by live
hydro-thermal erosion, every layer tunable from the panel, with a **wireframe**
toggle to inspect the grid — on native and web. Both terrain layers live entirely
in the demo; the engine only uploads the mesh, selects solid/wireframe, draws the
UI, and runs the camera. `wgpu`/`winit` are nowhere in the demo.

*On Slice 4:* its analytical solver is retired, not deleted from history — it
proved the vertical worked end-to-end (the thing that mattered then). Slice 6 keeps
that win and trades the algorithm for one that honors the "compose, don't solve"
and "always something visible" principles.

## The second vertical — a `scene` demo (Slices 7–11)

Terrain proved the thesis, but it is a **single deforming mesh in world space**.
Every consumer that wants *many distinct objects that move independently* falls
off a cliff the terrain demo never approached: meshes are uploaded pre-transformed
and `cube.rs` literally rotates its own corners on the CPU and re-uploads the mesh
every frame. That works for one cube and for nothing else.

The driving demo is **`examples/scene.rs`** — a small stage holding several
articulated figures assembled from primitive shapes, moving independently, lit,
with the whole scene pausable and scrubbable. It is deliberately content-free: no
terrain, no game, nothing but "several things, in different places, that a viewer
can tell apart."

*Where this came from, honestly.* The sequence below was **not** invented from a
blank page — it was distilled from an external project's request list (a
deterministic simulation with an event stream, wanting a renderer). Principle 2
forbids building from a wishlist, so nothing here is scheduled on that project's
behalf: each item was kept only because `scene.rs` independently hits the same
wall, and the rest was discarded (see *What stays in the consumer* below). If
`scene.rs` doesn't hit a wall, it isn't in this list.

### Slice 7 — Per-object transforms + an instance draw-list

*Roadblock:* the demo wants twenty objects, several of which are the *same* mesh
in different places. Today `set_meshes` takes geometry already baked into world
space, so "move something" means rebuilding its vertices and re-uploading — per
object, per frame — and the same box uploaded ten times is ten vertex buffers.

- Uploading a mesh returns a **handle**; the draw-list becomes a list of
  *instances* (`handle` + transform), so one mesh can be drawn many times.
- A per-object **model matrix** reaching the shader. The vertex shader stops
  assuming world-space input.
- Transforms exposed as plain data (position / rotation / scale, or a `[[f32; 4];
  4]`) — the consumer never sees a GPU type (principle 4).

*Parity risk to settle here:* the WebGL2 fallback runs under
`downlevel_webgl2_defaults`, which has **no storage buffers** — so per-instance
data must ride either an instance-step vertex buffer or a uniform with dynamic
offsets, not the storage-buffer approach most tutorials reach for. Decide it in
this slice, while the surface is small.

*Proof:* `scene.rs` places and moves many objects, reusing meshes, with **zero**
per-frame vertex re-upload; `cube.rs` loses its CPU corner-rotation.

### Slice 8 — Vertex normals + a lighting model in the pipeline

*Roadblock:* Slice 7 breaks the trick terrain relies on. The terrain demo bakes
diffuse shading into vertex color CPU-side, which is correct only because that
mesh never moves — rotate an object whose lighting is painted into its vertices
and the highlight rotates with it. Lighting has to be evaluated *after* the model
transform, which means it has to live in the shader.

This is the seam `ARCHITECTURE.md` already predicted: "a second lit demo would
justify pushing normals into the pipeline." `scene.rs` is that demo.

- `Vertex` grows a normal; the pipeline transforms it by the normal matrix.
- One directional light + ambient, Lambert diffuse. **Not** a lighting *system* —
  no point lights, no shadows, no PBR until something demands them.
- Terrain's CPU-baked shading is retired in favor of real normals, which also
  makes its wireframe/solid toggle honest.

*Proof:* a rotating object in `scene.rs` is lit consistently from a fixed world
direction, and terrain looks the same or better with the CPU bake deleted.

### Slice 9 — Per-instance material

*Roadblock:* two instances of the *same* mesh must be visually distinguishable,
and after Slice 7 they cannot be — color lives in the shared vertex buffer, so
every instance of a mesh is identical by construction. Duplicating the mesh per
color would undo the entire point of Slice 7.

- A small per-instance **material**: base color tint, and whatever minimum the
  lighting model needs (e.g. a specular/shininess scalar). Multiplied into vertex
  color rather than replacing it.
- Deliberately **not** a material system: no shader graph, no pipeline
  permutations, no texture binding (see *Beyond*). One struct on the instance.

*Proof:* one uploaded mesh, drawn many times, each instance a different color.

### Slice 10 — Primitive mesh builders

*Roadblock:* every figure in `scene.rs` is assembled from boxes, spheres, and
capsules, and after Slice 8 hand-rolling them is genuinely painful — correct
per-face normals mean splitting shared vertices, so a box is 24 vertices with
carefully paired normals rather than 8 tidy corners. Writing that by hand for the
fifth shape is the wall.

- `Mesh::box`, `Mesh::sphere`, `Mesh::capsule` (or cylinder), `Mesh::plane` —
  positions, indices, and correct normals, parameterized by size/segments.
- Zero dependencies, zero file I/O, no runtime asset loading. This is the
  **deliberate alternative to an asset pipeline**: composition of primitives under
  a transform, not glTF.

*On principle 3:* a mesh builder isn't GPU plumbing, so it's worth stating why it
lands in the engine anyway — it is entirely **content-free** geometry
construction. It encodes no consumer semantics (compare: a `Terrain` builder,
which would). Every consumer needs a box; none of them need *our* box.

*Proof:* `scene.rs` builds all its geometry from engine primitives and contains no
hand-written vertex arrays.

### Slice 11 — Fixed-timestep clock + time control

*Roadblock:* `scene.rs`'s motion is frame-rate coupled — it looks different at
60 Hz and 144 Hz, and replaying it doesn't reproduce. The demo wants to pause,
single-step, and scrub the scene, and `Renderer::dt()` (raw wall-clock) cannot
express any of that.

- An **accumulator** driving a fixed-step hook at a consumer-chosen rate, plus an
  **interpolation alpha** so rendering between steps is smooth rather than juddery.
- Consumer-facing time control: pause, time scale, single-step, and seek.
- **Determinism as a property of the seam.** The engine's contribution is simply
  that a consumer which advances its state *only* in the fixed hook gets the same
  result regardless of frame rate — the engine stops being the source of
  wall-clock nondeterminism. It does not make the consumer deterministic; that is
  the consumer's job.
- Terrain benefits too: its erosion iterations are frame-rate coupled today.

*Proof:* `scene.rs` runs identically at capped and uncapped frame rates, and its
transport controls (play/pause/step/scrub) drive the scene from the UI panel.

### What stays in the consumer

Recorded because this boundary will be asked about again, and it is the same
ruling that keeps the erosion solver in the terrain demo (principle 3).

A consumer that wants to *visualize its own simulation* will ask for two things
that look like engine work and are not:

- **A presentation model** — a pure function from that consumer's own state (or
  event log) to a view state. It is the consumer's data model, one layer above its
  simulation and one layer above us.
- **Spatial synthesis** — inventing continuous positions, facing, and posture
  where the simulation only has categories. That is an *algorithm*, exactly like
  stream-power erosion, and it belongs in the demo.

The engine's answer to both is the same answer terrain got: you compute it, we
draw it. What we owe such a consumer is the list above — transforms, lighting,
materials, primitives, a time model — and nothing that knows what the objects
*mean*.

## The UI split (post-Slice 6)

Slice 5 delivered a UI framework as a side effect of the terrain demo needing
knobs, and Slice 6 grew it again. That worked, but it put a widget toolkit inside
a 3D rendering engine — and the toolkit is the part most likely to expand without
limit, because it has the tightest iteration loop and the best-looking results per
hour spent.

So it moved out (**UI Slice 0**, done). **`slmsttaa-ui` is its own workspace
member**, a zero-dependency leaf crate that the engine depends on and re-exports
as `slmsttaa::ui`. The split is for *enforcement*, not insulation: the old
`src/ui.rs` already claimed it never saw `wgpu`, and a crate boundary turns that
claim into a compile error — the same trick `examples/` plays on the
engine/consumer boundary (principle 1).

The engine's side of the move was small and is the whole seam: it keeps
`impl Painter for Overlay`, and `Renderer::ui()` translates this frame's `Input`
into the toolkit's own `UiInput` snapshot (the toolkit can't import `Input` —
that would be the cycle). Consumers saw no change at all; `examples/terrain.rs`
was not touched.

It does **not** make UI work free of engine changes. Growing what the toolkit can
draw means growing `renderer/overlay.rs`, `overlay.wgsl`, and the `Vertex2D`
layout. The boundary just forces that to arrive as a deliberate widening of the
`Painter` trait instead of a private reach-through.

**All UI planning now lives in [`slmsttaa-ui/ROADMAP.md`](slmsttaa-ui/ROADMAP.md)**
— slices, scope limits, and the stopping rule that keeps the widget roster from
becoming the project. See [`slmsttaa-ui/README.md`](slmsttaa-ui/README.md) for the
design and the dependency-direction decision. This roadmap keeps the engine half
only: the overlay pass, the glyph atlas, and the input plumbing beneath the
`Painter` seam.

## Beyond (seams, not commitments)

Listed only so we recognize them when a future demo demands them — **not** to be
built ahead of need: MSAA, and a render graph once there's more than one pass.
(Transforms, a lighting model, and a minimal material moved out of this list and
into Slices 7–11 above, because `scene.rs` demands them.) Each of the rest waits
for a consumer to ask:

- **Consumer-supplied textures.** The overlay already samples a glyph atlas, so
  the shader work is adjacent, but no public API exists to upload an image and
  nothing has demanded one. Two things would: a 3D demo wanting surface detail
  that per-instance color can't express, and the UI crate's request for textured
  quads (icons, portraits) — see [UI `WISHLIST.md`](slmsttaa-ui/WISHLIST.md).
- **An asset pipeline (glTF/OBJ + runtime file loading).** Explicitly *not* taken:
  Slice 10 answers "geometry the consumer didn't compute" with primitives plus
  transforms instead, which costs zero dependencies and no wasm asset-fetching
  story. Revisit only when a demo needs authored art that primitives genuinely
  cannot compose.
- **Skeletal animation (joints, vertex skinning, clip playback).** Recognized and
  deferred whole. It is the single largest item any renderer consumer will ask
  for, and it is close to pointless without the asset pipeline above — you cannot
  author a rig with no importer. Slices 7–11 deliberately stop at rigid objects a
  consumer poses itself each frame; that is animation *by the consumer*, and it is
  as far as we go until a demo proves it insufficient.
- **An offscreen render target composited into a UI rect**, so the 3D scene is one
  panel among many rather than a fullscreen background with UI floating on top.
  This is the concrete thing that would turn the two hand-wired passes into a real
  render graph; it is named in [UI `WISHLIST.md`](slmsttaa-ui/WISHLIST.md) as
  engine-side work.

Painter capabilities the UI crate demands of the overlay are engine seams too —
but they're sequenced in the [UI roadmap](slmsttaa-ui/ROADMAP.md), since that's
what pulls them into existence. Four have already landed there and been paid for
here: ordered draw layers in `Overlay::flush` and a `scale_factor`-aware surface
(UI Slice 1), then rounded-rect and clip support in `overlay.wgsl` and the wider
`Vertex2D` that carries them (UI Slice 2), then a distance-field text mode plus a
linear atlas sampler (UI Slice 5). The overlay is still a single `draw_indexed`. The next one the UI is likely to ask for is textured quads, which
is the same shader work the "no texture support" entry above is waiting on.

The trend since is the point: UI Slice 3 (layout) cost one field — `UiInput`
gained `viewport`, filled by `Renderer::ui()` from the surface size over the scale
factor — and UI Slice 4 (theme tokens) cost **nothing at all**. No `Painter`
method, no shader change, no `Vertex2D` field. A whole styling system landed above
a seam that speaks in colors and rectangles and does not care where a color came
from, which is the clearest evidence yet that the seam is drawn in the right
place.

UI Slice 5 (typography) is the exception, and interesting for being one: it is the
only slice so far to make the seam **narrower**. `text_size` left the `Painter`
trait entirely and `src/renderer/font.rs` was deleted, because two independent
implementations of "how wide is this string" agreed only by the accident of a
monospace font and would have silently disagreed the moment the advances became
proportional. The engine no longer owns a font; it uploads the toolkit's atlas and
draws the quads it is given. A seam that can be *cut back* when a capability moves
above it is as good a sign as one that absorbs a change without moving.
