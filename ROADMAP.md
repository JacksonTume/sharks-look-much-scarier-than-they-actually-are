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
- The driving demo runs and shows the new capability on screen. `cargo xtask
  shoot <example>` is the mechanical way to do that (see *The harness* below);
  looking at it yourself still counts, and still catches things the harness
  cannot.
- Any new public API has rustdoc; `ARCHITECTURE.md` is updated if the
  init/render flow changed.
- The engine still contains **zero** consumer-specific content.


## The harness

The line above — "runs and shows the new capability on screen" — is the one this
file leans on hardest and the one that was, until Slice 19, entirely manual. The
record justifies the emphasis: **six bugs are on file that passed the whole test
suite and were caught by a human looking at a window** (UI Slices 1, 3 and 5;
Slice 19's collapsed pane and its frame-stale click gate). It is also the step
that costs the most in a cloud container, where there is no GPU, no display and
no screenshot tool until something installs them.

`cargo xtask shoot` makes it executable. What it is *not* is a replacement for
looking: it photographs what the engine draws, and a picture nobody looks at
proves only that the pixels did not change.

**It was Linux-only, and is not any more.** The harness owned an `Xvfb` display
and drove it with ImageMagick's `import` and `xdotool`, none of which exist on a
bare Windows box. Two things made that land badly, both fixed while verifying
Slice 20 from a Windows workstation:

- The example path was built without `std::env::consts::EXE_SUFFIX`, so on
  Windows it looked for `examples/terrain` on a machine that had just finished
  building `terrain.exe` and reported a missing binary that was sitting right
  there. A platform bug that only the harness's *own* portability depended on,
  which is why nothing caught it.
- The prerequisite check happened three steps in, after a full build. It now
  runs **before** the build and names every missing tool at once, rather than
  spending minutes compiling to arrive at `could not run Xvfb`.

**Then it grew a Windows half**, because "run the demo and look at it" is not a
workable answer when the person who has to look is the one asking for the
capture. The protocol was already portable — a pinned clock, checkpoints on
stdout, a newline to release — so only three things were X11-shaped: somewhere to
render, a way to photograph, a way to poke. Those are now a seam (`xtask/src/harness/`)
with two implementations, and `shoot` itself contains no `#[cfg]` at all.

The Windows side is deliberately unlike the Linux one, and the differences are
the interesting part:

- **No virtual display, so the window is parked off the desktop** — past the left
  edge of the virtual screen, never activated. DWM keeps composing it, which is
  all `PrintWindow(PW_RENDERFULLCONTENT)` needs, so a capture runs with nothing
  visible and focus undisturbed. If a driver returns black for that — a real
  possibility for a swap chain, and not an error it reports — the shot is retaken
  from the screen and the window comes back into view for it, with a note saying
  why.
- **Input is posted to the window, not to the desktop.** `xdotool` warps the real
  pointer; `PostMessageW` does not. That is a correctness improvement as well as
  a courtesy — a synthetic click can no longer land in whatever happens to have
  focus — and it is now a convention in `.claude/CLAUDE.md`.
- **No dependencies, still.** Win32 is declared by hand, and the PNG writer is a
  hundred lines using stored (uncompressed) deflate blocks, which is a real zlib
  stream that every reader accepts. `capture/` is gitignored, so the size does
  not matter and the compressor nobody needs was not written.

A verification tool that fails confusingly is worse than one that is merely
unavailable — but one that works everywhere is better than either, and the first
thing it did on Windows was find two bugs in itself (see *Slice 9* in the UI
roadmap).

### What it does

```sh
cargo xtask shoot triangle --frames 120          # one PNG, at exactly frame 120
cargo xtask shoot terrain  --frames 400 --size 1280x720
cargo xtask shoot workspace --script capture/workspace.script
```

On Linux it starts its own `Xvfb`, runs the example against it (lavapipe answers
as a software Vulkan adapter), and photographs the window with ImageMagick's
`import` at frame numbers the engine announces. On Windows it parks the window
off the desktop and asks the compositor for it instead. A script adds
`move`/`click`/`wheel`/`key` steps keyed to those same frames.

### The two things that made this worth building

**A run is now reproducible.** `SLMSTTAA_CAPTURE_DT` pins the frame delta, so
`elapsed` — and with it the ripple field, the `Timeline`'s step payout and every
UI animation — depends on how many frames have run rather than how fast the
machine ran them. The measurement that justifies it: capturing `terrain` twice by
hand differed by **0.6% RMSE**, all of it in the water, which is more than enough
to hide a real regression. Through the harness the same two captures differ by
**zero pixels**. `triangle` does too, but `triangle` was never the hard case.

**A run now has an observable moment.** `SLMSTTAA_CAPTURE_FRAMES` makes the
engine print `slmsttaa: capture <n>` and *freeze* — re-presenting the same
picture, simulating nothing — until a line arrives on stdin. Before this, a
harness could only `sleep` and hope, which is both slow and a lie about what
frame it caught.

Both live in `src/capture.rs`, are native-only, and are driven **entirely by
environment variables**, so capture mode adds nothing to the API a consumer can
see. That was a constraint, not a convenience: `examples/triangle.rs` exists to
prove the public surface is sufficient, and a `Renderer::set_capture` that only
tests use would have quietly widened the thing that example measures.

### What fell out for free, and is the nicest part

A frozen frame skips `Input::end_frame`, which is what clears press edges. So a
click delivered *while the engine is parked* still has its edge set on the frame
that follows. **The freeze is an input window**, which is what lets a script
click a specific object on a specific frame rather than at a specific moment.
`capture/workspace.script` is Slice 19's picking check written down: pause the
ring, click the sphere, confirm the cage appears, then click a panel and confirm
it does *not* reach the scene.

One wrinkle had to be paid for: motion accumulates across a freeze too, so the
cursor warping to a click target arrives as one enormous `mouse_delta` and snaps
any camera that orbits on it. Resume therefore discards motion while keeping
press edges (`Input::discard_motion`).

### Waiting on a roadblock

Recognized, **not** scheduled — the same rule the rest of this file runs on. The
failure mode here is well understood and is the one `slmsttaa-ui/README.md` was
written to prevent: the tooling becoming the project.

- **Committed golden images, and `shoot --check`.** Everything needed is here —
  captures are exact now — but a golden is only worth its review cost if
  something runs it unprompted, and this repo has no CI. The `.gitignore` keeps
  `capture/*.script` and drops the PNGs, so the shape is ready when the answer
  changes.
- **Engine-side readback** (`copy_texture_to_buffer` plus an image writer), so a
  capture needs no X server at all and grabs the surface rather than a window.
  That would also make a capture callable from a `#[test]`, which is the real
  prize. It is a bigger job than it sounds: `Renderer::new` takes an
  `Arc<Window>` and owns a `Surface`, so headless means a second construction
  path.
- **A synthetic input queue.** Feeding events straight into `Input` instead of
  through `xdotool` would drop the X dependency for interaction and land a click
  on an exactly known frame. There is a concrete reason to expect this to be
  needed: with no window manager under Xvfb nothing holds keyboard focus, so
  `xdotool key` may not arrive at all. Pointer input via XTEST is proven; keys
  are not, and the script grammar accepts `key` on trust.

  **Measured, while verifying UI Slice 10, and it bounds what a diff can claim.**
  Two runs of `capture/terrain-plot.script` from *the same binary* are byte-
  identical, including the shots after a click — the property this file advertises,
  and it holds. Across a **rebuild**, the two input-free shots stayed identical and
  the two taken after a click did not. So a click's delivery frame is stable within
  a build and not guaranteed across one, which is precisely the exactness this
  entry promises. Until it lands, a golden image taken after an input step is a
  same-build comparison, not a same-commit one.
- **A press and a release, so a script can drag.** Found writing
  `capture/terrain-plot.script` for UI Slice 10, and it is the wheel's story
  again: `Action::Click` is a press *and* release with no update in between, so
  `Response::held` is never true for a single frame — and **every drag widget in
  the toolkit acts on `held`**. A button can be clicked; a slider cannot be moved
  at all, which means the transport's scrub, both erosion `log_slider`s, and every
  numeric knob in every demo are unreachable by the harness. The script worked
  around it by letting the demo *play* to the pass it wanted, which happens to be
  available here and will not always be. The fix is small — split `Click` into
  `press`/`release` steps, or add a `drag` that holds across a frame — and it is
  listed rather than done because the workaround cost nothing this time and the
  grammar is worth changing once, deliberately.
- **Web capture.** Headless Chrome here photographs a blank canvas from a WebGPU
  surface — verified against *unmodified* code as a control, so it is
  environmental rather than a defect. Until it moves, the web half of the
  Definition of Done stays a build-and-boot check, and slices should say so
  rather than implying a picture was looked at.
- **Perf capture** from the same run. Slice 14 spent 10 ms a frame CPU-animating
  water and it was noticed as a feeling before it was measured; frame timings
  alongside the frames would have made it a number.

## The slices

The driving vertical is the **terrain + erosion demo**. Each slice is pulled into
existence by the next thing that demo cannot do.

### Slice 0 — Invert control (bootstrapping) ✅ done

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
(`set_meshes` was retired in Slice 8; `Mesh` and the indexed draw survive it
unchanged.)

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

### Slice 7 — A water surface: lakes and rivers ✅ done

*Roadblock:* the water is **computed every pass and thrown away**. `flow_route`
(`terrain/erosion.rs`) floods every depression to build `filled[]`, and the
drainage accumulation builds `area[]`. Lake depth is `filled[c] - z[c]`; rivers
are the cells where `area` is large. Both already exist, once per pass, and both
are dropped on the floor when the pass ends. The demo has been simulating water it
never draws — a terrain shaped entirely by rivers, with no rivers on it.

One change, **entirely in the demo**: `erode` hands back the final pass's water as
a `Water { depth, area }`, and the demo builds a second `Mesh` from it —
lake surfaces at the flooded height wherever `filled > z`, river ribbons wherever
`area` crosses a threshold, both lifted by an ε to sit off the terrain rather than
z-fight with it. Blue vertex color, opaque. `set_meshes(&[terrain, water])`.

*This is not a particle system.* `erosion.rs`'s module docs record why
droplet-hydraulics was tried and abandoned — independent droplets never pool into
a connected network, so they roughen noise instead of carving valleys. Nothing
about that ruling changed. The water drawn here is the flow-accumulation network
the model was already using.

*What this slice deliberately does not do:* **add anything to the engine.** No new
public API, no `Painter` method, no shader change. That is unusual enough to
state outright — every prior slice bought a capability. This one is justified by
principle 5 (always keep something on screen) and constrained by principle 2:
water demands no engine work, so water gets none. If it turns out to demand some,
that discovery is the slice's real output.

*Drawing lakes changed the erosion model, which was not expected.* The implicit
update raises a pit toward the rim it was breached from, so **one pass used to
pack every depression with rock** — the reason there were no lakes to draw.
Skipping submerged cells fixes that and is the honest reading anyway (no stream
over a lake bed, so nothing to cut with). But on its own it nearly stopped the
model: a fifth of the map sat underwater and inert, and sixty passes moved the
mean height 2.5%.

The missing term was **deposition**. Rivers drop their load where they meet
standing water, so lakes silt up, spill, and hand their basins to the drainage
network — *drainage integration*, the mechanism the Cordonnier reference is built
on, arrived at from the wrong end. With it, lake coverage falls from 22% at pass 0
to 13% by pass 60 to nothing by pass 120, which is why the pass count now doubles
as a wetness control: a low count leaves lakes, a high one leaves only rivers.

*Diagnosis note for future slices:* that stall was found with a throwaway probe
example that ran the solver headless and printed relief, lake fraction, and
per-pass movement every twenty passes. It was **not** visible in a screenshot —
the terrain looked fine, it just wasn't changing. When a simulation looks wrong,
measure it before re-tuning it.

*Proof:* `cargo run --example terrain` shows lakes standing in the basins and a
dendritic river network threading the valleys of the eroded terrain, wireframe
included, at 92fps on a 128² grid. `wgpu`/`winit` are still nowhere in the demo,
and **the engine is unchanged**: no new public API, no `Painter` method, no shader
edit. `set_meshes` turned out to already take a slice, so the water went in beside
the terrain as a second element of a call that has existed since Slice 1.

*Fixed later: the water turned into straight lines.* Watched to the end of the
axis, a mature run drew bundles of dead-straight parallel diagonals across every
drained basin instead of a channel. The cause was in `flow_route`, and it had
been there since this slice: Priority-Flood **+ ε** lifts each cell a hair above
whichever neighbour reached it first, and on level ground the flood is a
breadth-first wave, so "whoever got here first" is a BFS parent tree whose
branches are straight rays. Drainage area piled onto the rays and the demo drew
them. Lakes hid it early on; once they silted up it was all that was left, so it
read as *the water turning into lines*.

The fix drops the ε — a filled depression is now exactly level — and gives the
level ground a direction with the flat-resolution of Barnes, Lehman & Mulla 2014:
two breadth-first sweeps, one measuring steps to the flat's exit and one steps
from the higher rim, with flow taking the neighbour that gets closer to the exit
and stays furthest from the rim. That second term is what makes flow lines *merge*
into one channel rather than race side by side. Measured on the default terrain:
the longest run of unchanging D8 direction in the river network fell from 31 cells
to 21, and runs of 12 or more from 52 to 16. Lake coverage and the shape of the
arc are unchanged (22.6% at pass 0, nothing by ~pass 110), so the table below
still stands; river coverage rose slightly, 5.8% to 6.5%, because converging flow
puts more cells over the drawing threshold. It costs about 50% more per pass
(1.8ms to 3.0ms at 128², 8.7ms to 13.4ms at 256²), which at eight passes a second
is noise.

Two smaller things fell out of it. The ε used to strew fake shallow depths over
dry ground, which is what `MIN_POND` was really for; that job is gone and the
constant is back to meaning what it says. And the minimap divided raw depth by
`LAKE_OPAQUE_DEPTH` where the water surface subtracts `MIN_POND` first, so at the
end of the run the map painted lakes over basins the 3D view correctly showed as
dry — two views of one field, disagreeing.

*Deferred out of this slice, on purpose:*

- **Animating it.** The original plan for this slice was the erosion *running* on
  screen — passes spent off a clock, the mesh rebuilt every frame, lakes visibly
  silting up and draining. It was built, and it was pulled: it looked bad. Worth
  recording what was learned before it went, because the next attempt starts here
  rather than from scratch. **A pass is not a frame** — one pass per frame sounds
  right and is useless, since sixty passes at 75fps is over in 800ms and the
  terrain still teleports, just via a blur; pacing has to come off a wall clock at
  a passes-per-second rate. And that pacing is a *paper-over* of the frame-rate
  coupling Slice 12 exists to fix: it stabilises how fast you watch, not what you
  get. The mesh work is all reusable — `erode` is a loop over a private one-pass
  `step`, so re-exposing it is a one-line change. What is missing is a reason to
  believe the result is worth watching, and that is a question about how the water
  and terrain *look* in motion, not about the stepping.

  *This was retried and shipped as **Slice 13**, and the diagnosis above was
  right about the cause and wrong about the cure — see below.*

- **Translucent water** waited for Slice 10 (per-instance material) and **landed
  there**. It needed alpha, which the engine had nowhere to put — `Vertex` was
  position + RGB and the scene pipeline was `BlendState::REPLACE`. A water-only
  alpha path would have jumped the queue for one demo; holding it until a second
  consumer wanted the same capability is what made it a design instead of a patch,
  and water picked it up for the cost of one `with_material` call.
- **Waves, refraction, and reflections** are further out still — see *Beyond*.

## The second vertical — a `scene` demo (Slices 8–12)

Terrain proved the thesis, but even with water it is **two deforming meshes that
share one world space** — both rebuilt from scratch, in place, every time they
change. Every consumer that wants *many distinct objects that move independently*
falls off a cliff the terrain demo never approached: meshes are uploaded
pre-transformed and `cube.rs` literally rotates its own corners on the CPU and
re-uploads the mesh every frame. That works for one cube and for nothing else.

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

### Slice 8 — Per-object transforms + an instance draw-list ✅ done

*Roadblock:* the demo wants twenty objects, several of which are the *same* mesh
in different places. `set_meshes` took geometry already baked into world space, so
"move something" meant rebuilding its vertices and re-uploading — per object, per
frame — and the same box uploaded ten times was ten vertex buffers.

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

*Proof:* `cargo run --example scene` shows 25 boxes turning and bobbing
independently over a ground plane, from **one** uploaded box mesh, in **two** draw
calls, with **zero** per-frame vertex uploads — the HUD reports all four numbers
and they are real. `cube.rs` lost its `rotate()` and its per-frame re-upload;
`gallery.rs` stopped rebuilding its spinning scene and now uploads all four scenes
once, so switching names a different handle. Verified on native and on wasm.

**What shipped, and what it cost:**

- **Three types and three methods.** `MeshHandle` / `Transform` / `Instance` in
  `src/renderer/instance.rs`; `upload_mesh` → handle, `update_mesh(handle, &Mesh)`,
  `set_instances(&[Instance])`. `set_meshes` is **gone** rather than kept as a
  convenience — the same one-surface trade UI Slices 3 and 4 made for the slider
  and button builders, and it cost five call sites.
- **Handles are append-only, and that is the whole lifecycle.** Nothing is ever
  freed, so a handle cannot dangle and no generational slot map is needed. A
  removal API waits for a demo that actually spawns and destroys objects; none
  does, and inventing one now would be the speculative build principle 2 forbids.
- **`update_mesh` is the terrain demo's half of this slice, and it is the more
  interesting half.** Erosion changes a landscape's *shape*, which no transform can
  express, so terrain keeps two stable handles and refills them. That split —
  "moves" versus "changes shape" — is the real boundary the API draws, and it is
  clearer than the one the slice set out to draw.
- **Transforms are plain arrays, and rotation is Euler.** `position` /
  `rotation` (radians, applied Y→X→Z) / `scale`, following the rule
  `Camera::look_from_to` and `Vertex` already set: a demo places twenty objects
  without depending on `glam`. Gimbal lock is the accepted cost, recorded on the
  type; a quaternion is the correct fix and an unpleasant thing to author by hand,
  which is exactly what this API exists to avoid.
- **No hierarchy.** The engine takes world transforms; composing a parent into a
  child is arithmetic, and arithmetic is the demo's (principle 3 — the same ruling
  that keeps the erosion solver out of the engine). `Transform::matrix()` is public
  so a consumer can read what it built; nothing yet needs to *hand back* a composed
  matrix, and the articulated figures of Slice 11 are what will decide whether
  `Instance` should accept one.

*The parity risk was real, and it was not the one flagged.* The slice predicted
storage buffers as the trap, and they were — `downlevel_webgl2_defaults` has none,
so per-instance data rides an instance-step vertex buffer (the model matrix as four
`vec4` columns, locations 2–5). But the sharper edge was **`first_instance`**:
WebGL2 has no non-zero variant, so the obvious way to draw the second mesh's run —
`draw_indexed(.., first..last)` — compiles, runs perfectly on native, and fails only
in the browser fallback. Each run is selected by offsetting the *buffer binding*
instead, always drawing from instance zero. Recorded in `ARCHITECTURE.md`, because
a bug that only appears on one target is the kind that file exists for.

*What it exposed.* Every box in `scene.rs` is the same color, and there is no way
to change that: color lives in the shared vertex buffer every instance reads.
That is **Slice 10 arriving with evidence attached**, exactly as predicted. So is
Slice 9 — the boxes' shading is baked into their corners, so it turns with them,
which is visible the moment one rotates. Both were written down before this slice;
both are now things you can see rather than things the roadmap claims.

### Slices 9 & 10 — Lighting and material (taken together) ✅ done

**These were written as two slices and are being built as one.** Both reopen the
same two structures — `Vertex` and the per-instance buffer — and Slice 10's own
text already said so ("the two changes should be made together rather than
churning the format twice"). Splitting them would mean migrating six demos'
vertex arrays twice, in consecutive slices, to reach a state neither half is
useful without: normals with no material means `scene.rs` is lit but still
monochrome, and material with no normals means colored boxes that still carry
their highlight around with them. The two roadblocks below are kept separate
because they *are* separate, and each is independently what pulled its half in.

#### The lighting half

*Roadblock:* Slice 8 breaks the trick terrain relies on. The terrain demo bakes
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

*The normal matrix is not the model matrix, and `scene.rs` is why.* Every box on
that stage is scaled non-uniformly (`[0.6, height, 0.6]` — the height is what
makes the objects tell apart), and a non-uniform scale skews normals: the upper
3×3 of the model matrix stretches them along with the geometry, so a squashed box
shades as though its faces were tilted. The fix is the 3×3 **inverse-transpose**,
computed CPU-side per instance and shipped as three more instance-step
attributes. That is the correct-under-any-transform answer rather than the
cheap-and-usually-fine one, taken deliberately because the demo already breaks
the cheap one on day one — this is not a defect discovered later, it is visible
in the first frame.

#### The material half

*Roadblock:* two instances of the *same* mesh must be visually distinguishable,
and after Slice 8 they cannot be — color lives in the shared vertex buffer, so
every instance of a mesh is identical by construction. Duplicating the mesh per
color would undo the entire point of Slice 8.

- A small per-instance **material**: base color tint, and whatever minimum the
  lighting model needs (e.g. a specular/shininess scalar). Multiplied into vertex
  color rather than replacing it.
- Deliberately **not** a material system: no shader graph, no pipeline
  permutations, no texture binding (see *Beyond*). One struct on the instance.

*The second consumer — water, and why it waited.* Terrain's water surface (Slice
7) ships **opaque**, and translucency is the one thing it obviously wants. It was
held back to here rather than given a water-shaped alpha path of its own, because
two demos asking for the same capability is the difference between a design and a
patch. What water needs beyond a color tint is small and worth naming now, since
it is the part `scene.rs` alone would not have demanded:

- **An alpha channel at all.** `Vertex` is position + RGB (`renderer/vertex.rs`)
  and the scene pipeline is `BlendState::REPLACE` (`renderer/mod.rs`). **Alpha
  rides the material, not `Vertex`** — a `[f32; 4]` tint per instance. That is
  the decision the fused slice lets us make once: transparency is a property of
  *this placement of a mesh*, not of the mesh's corners, which is exactly the
  split Slice 8 drew. Water is one uploaded surface that wants to be see-through;
  nothing wants per-corner opacity, and adding a fourth float to every vertex of
  a 128² terrain to express a whole-object property would be the wrong end.
- **A transparent draw rule:** blended meshes drawn after opaque ones with
  depth-write off. The existing `line_pipeline` is the precedent for a pipeline
  variant selected per draw, so this is a known shape, not new ground. Sorting is
  not needed for water specifically — one non-overlapping surface — and a general
  sort should wait for a demo that actually needs one.

#### What lands, concretely

- `Vertex` becomes position + **normal** + color (attributes 0–2). Every demo
  that builds a mesh supplies normals; terrain's come from the heightmap gradient
  it already computes for the CPU bake it's deleting.
- `Instance` grows a `Material` — an RGBA `tint` multiplied into vertex color.
  Not a material *system*: no shader graph, no pipeline permutations, no textures
  (see *Beyond*). **No shininess scalar either**, which the slice as drafted
  expected to need: the lighting model is Lambert, Lambert has no specular term,
  and a field nothing reads is storage for a promise.
- The instance-step buffer carries the model matrix (locations 3–6), the normal
  matrix (7–9), and the tint (10). **Eight instance attributes plus three vertex
  attributes is eleven**, against the sixteen WebGL2 guarantees — room, but no
  longer generous, and worth knowing before Slice 11 wants to add anything.
- A blended pipeline variant, selected per draw-list run, drawn after the opaque
  runs with depth-write off.

*Parity note, learned the hard way in Slice 8:* the browser fallback is where
instance-buffer work breaks, and `--target wasm32-unknown-unknown --lib` will not
catch it — `first_instance` compiled and ran perfectly on native. This slice gets
looked at in an actual browser, not just type-checked.

*Proof:* `scene.rs` shows one uploaded box drawn 25 times, each instance a
different color, every one lit consistently from a fixed world direction as it
turns and whatever its scale — still two draw calls and zero vertex uploads per
frame. Terrain's rivers and lakes are translucent with the drowned ground visible
through them, set by a `Material` rather than by any water-specific engine code.
The honest test was terrain's CPU shading bake being **deleted**: a before/after
capture shows the landscape unchanged, which is the result that says lighting
genuinely moved into the pipeline rather than being re-tuned to look similar.

**What shipped, and what it cost:**

- **One type and one field.** `Material { tint: [f32; 4] }` and
  `Instance::material`. Alpha rides the material rather than widening `Vertex` to
  RGBA, because see-through is a property of a *placement*, not of a mesh's
  corners — and nothing wants per-corner opacity.
- **The drafted shininess scalar was never built.** Lambert has no specular term,
  so the field would have been storage for a number no shader reads. Worth
  recording as the slice's one deliberate subtraction: it was in the plan, and the
  plan was wrong in a small way that only writing the shader revealed.
- **The light is a shader constant, not an API.** No demo asked to move the sun,
  and a setter with no caller is the speculative build principle 2 forbids. The
  constants are exactly the ones terrain used to bake by hand, which is what makes
  the before/after comparison meaningful.
- **The fused slice paid off twice, not once.** The predicted win was migrating
  six demos' vertex arrays one time instead of two. The unpredicted one is that
  fusing is what let alpha be designed *against* the normal work — with both open
  at once it was obvious that one belonged on the instance and the other on the
  vertex, where either slice alone would have put both in the same place.

*What it exposed, and it is Slice 11 with evidence attached.* A lit box cannot
share its eight corners: a corner touches three faces pointing three ways, and a
vertex carries one normal. Every box in the tree is now written out as 24
vertices **by hand, in three separate demos** (`cube.rs`, `gallery.rs`,
`scene.rs`) — the exact duplication Slice 11 predicted would become the wall, now
sitting in the codebase rather than in this document.

*On the parity risk.* `scene.rs` was checked in a browser and renders correctly —
but Chrome served it the **WebGPU** backend, so the WebGL2 fallback that Slice 8's
`first_instance` bug lived in was *not* exercised. The instance buffer now uses 11
of the 16 attributes WebGL2 guarantees, which is within limits by inspection, and
the buffer-offset draw is unchanged from Slice 8. That is reasoning, not a test,
and it is the one claim in this slice that is weaker than the others.

### Slice 11 — Primitive mesh builders ✅ done

*Roadblock:* every figure in `scene.rs` is assembled from boxes, spheres, and
capsules, and after Slice 9 hand-rolling them is genuinely painful — correct
per-face normals mean splitting shared vertices, so a box is 24 vertices with
carefully paired normals rather than 8 tidy corners. Writing that by hand for the
fifth shape is the wall.

- `Mesh::cuboid`, `Mesh::sphere`, `Mesh::capsule`, `Mesh::plane` — positions,
  indices, and correct normals, parameterized by size/segments. (`box` is a
  reserved word, hence `cuboid`.)
- Zero dependencies, zero file I/O, no runtime asset loading. This is the
  **deliberate alternative to an asset pipeline**: composition of primitives under
  a transform, not glTF.

*On principle 3:* a mesh builder isn't GPU plumbing, so it's worth stating why it
lands in the engine anyway — it is entirely **content-free** geometry
construction. It encodes no consumer semantics (compare: a `Terrain` builder,
which would). Every consumer needs a box; none of them need *our* box.

*Proof:* `cargo run --example scene` shows nine figures walking on the spot, each
turning at its own rate, built from **four** meshes in **four** draw calls with
**zero** vertex uploads per frame — and `scene.rs` contains no vertex array at
all. `cube.rs` and `gallery.rs` lost their hand-written boxes too, so the 24-vertex
duplication Slice 9/10 left in three files is gone from all three.

**What shipped, and what it cost:**

- **Four builders and one shared lathe.** `sphere` and `capsule` are the same
  surface-of-revolution routine with different latitude lists — the capsule just
  repeats its equator, and the band between the two copies *is* the cylinder wall.
  Keeping the winding and the pole handling in one place is the difference between
  one correct implementation and two nearly-correct ones.
- **Primitives emit white.** Color is a per-instance `Material` now, so baking one
  into shared geometry would be the exact mistake instancing exists to avoid. A
  demo wanting per-vertex color (terrain's height palette) is describing something
  the builders don't, and still writes it by hand through `Mesh::new`.
- **The hierarchy question got answered.** Slice 8 deferred "should `Instance`
  accept a composed matrix?" to here, and the figures settled it: **yes**, via
  `Transform::then` / `then_matrix` plus `Instance::from_matrix`. `then` cannot
  return a `Transform` — compose a rotation with a non-uniform scale and the result
  is a shear, which position/rotation/scale cannot represent — so it returns a
  matrix, and the engine does the multiply so the demo still needs no math
  dependency. `Instance` now stores the composed matrix internally, which is what
  lets both entry points coexist without the draw path caring.
- **The engine grew its first tests, deliberately.** `CLAUDE.md` says the engine
  half is verified by building and looking at it, because it needs a GPU.
  Primitives don't: they are pure CPU geometry. The six tests check unit normals
  and that every triangle of a convex shape winds *outward* — and they immediately
  earned it, catching an inverted pole-degeneracy guard and a zero-length capsule
  that emitted a whole band of degenerate triangles. Neither was visible in a
  still frame.

*What it exposed.* Nothing new, which is itself worth recording — this is the
first slice since 7 that ends without evidence for the next one. Slice 12
(fixed-timestep clock) is still argued for by `scene.rs` moving on the wall clock,
exactly as written down before this slice started.

### Slice 12 — Fixed-timestep clock + time control ✅ done

*Roadblock:* `scene.rs`'s motion is frame-rate coupled — it looks different at
60 Hz and 144 Hz, and replaying it doesn't reproduce. The demo wants to pause,
single-step, and scrub the scene, and `Renderer::dt()` (raw wall-clock) cannot
express any of that.

*Half of that roadblock was wrong, and finding out which half is where the slice
started.* `scene.rs` was **not** frame-rate coupled: it had done `self.time += dt`
since Slice 8, and the field's own doc comment said so. The demos that genuinely
carried the defect were `cube.rs` and `gallery.rs`, both adding a flat `0.01` per
*frame* — so the spin really was twice as fast on a 144 Hz machine, in the two
places nobody had looked. The rest of the roadblock held exactly as written: no
run reproduces, and `dt()` cannot express pause, step, or scrub.

*Terrain will want this too.* Slice 7's deferred animation is blocked on exactly
this: advancing erosion passes on the frame clock means the same settings erode
further on a fast machine than a slow one — the identical defect, arrived at from
the other vertical, and with a landscape's worth of state behind it rather than a
rotation angle. If the erosion animation is retried before `scene.rs` exists, this
slice moves up to meet it.

- An **accumulator** driving a fixed-step hook at a consumer-chosen rate, plus an
  **interpolation alpha** so rendering between steps is smooth rather than juddery.
- Consumer-facing time control: pause, time scale, single-step, and seek — with
  the caveat, settled below, that a seek moves *the engine's clock* and cannot
  rewind a consumer.
- **Determinism as a property of the seam.** The engine's contribution is simply
  that a consumer which advances its state *only* in the fixed hook gets the same
  result regardless of frame rate — the engine stops being the source of
  wall-clock nondeterminism. It does not make the consumer deterministic; that is
  the consumer's job.
- Terrain benefits too: any retry of its erosion animation is a simulation
  advancing on the frame clock, which is this slice's whole subject.

*Proof:* `cargo run --example scene` shows a HUD reading **sim time**, **wall
time**, and a cumulative **steps** count. At vsync (75 fps) they read 11.95s /
11.97s / 723; flipped to `AutoNoVsync` at **1270 fps** — seventeen times the
frame rate — they read 12.05s / 12.06s / 723 at the same wall instant. The
transport controls work: pause froze sim time at 12.42s and the step count at 745
while wall time ran on from 13.00s to 16.45s and the FPS readout kept updating;
six clicks of **step** moved it to exactly 751 steps and 12.52s (six sixtieths,
not five or seven). Verified on native and on web under `BrowserWebGpu`.

**What shipped, and what it cost:**

- **One type, one trait method, one accessor pair.** `Timeline` (`src/time.rs`)
  beside the existing `Clock`; `Application::fixed_update(renderer, dt)`,
  defaulted; `Renderer::time()` / `time_mut()`. The default is what kept the cost
  down — `triangle.rs`, `grid.rs` and `terrain.rs` were not touched at all, which
  is the first trait change since Slice 0 to break nothing.
- **`time_mut()` is a handle, following `camera_mut`.** Pause, scale, single-step,
  seek and rate are one subject, and five more setters on `Renderer` would have
  said so less clearly.
- **Seek moves the engine's clock and nothing else, and that is stated rather
  than hidden.** The engine cannot un-erode a landscape, so it does not pretend a
  scrub rewinds a consumer. `scene.rs` pays the honest cost in two lines
  (`self.time = t; renderer.time_mut().seek(t)`) and can only do that because its
  state is a pure function of time. A consumer carrying irreversible state should
  ship no scrub control, which is the same ruling that keeps the erosion solver
  in the terrain demo.
- **The interpolation alpha is a number, not a system.** At 1270 fps roughly
  nineteen frames in twenty run *zero* steps, and the stage still moves smoothly —
  because `scene.rs` renders at `time + alpha * step`. That is not blending two
  snapshots; the pose is a pure function of time, so it evaluates the function at
  a sub-step instant. Which of those a consumer does is the consumer's business,
  and the engine hands over one `f32` either way. (This is the same shape UI
  Slice 6 recorded: the engine provided a number of seconds, not an animation
  system.)
- **A step cap, because `set_scale` outruns the existing stall clamp.** `Clock`
  already caps a frame at 100 ms — six steps at 60 Hz — but a time scale
  multiplies it, so `MAX_STEPS_PER_FRAME` is what makes the bound hold whatever
  the scale. Past it the remainder is dropped and simulation time falls behind
  wall time, which is the correct failure; carrying it forward is the classic
  spiral.
- **The engine's first tests outside `primitives.rs`, and for the same reason.**
  `Timeline` touches no platform API and needs no GPU, so it has eight unit tests
  — including the one that matters, that a second of wall time yields the same 64
  steps however it is chopped into frames. They are written against
  exactly-representable step sizes (1/64, 1/128) on purpose: a 1/60 step against
  1/144 frames leaves the count one either side of a boundary at the mercy of
  float rounding, which is a flaky test rather than a real assertion.

**Two things were cut after being built, both by looking at the screen.**

The first was a bug: the transport row used `ui.horizontal`, and a button
allocates *"whatever is left of the line"* — so `pause` took the whole row and
`step` was pushed off the panel, clipped mid-word at the edge. `ui.columns(2)`
divides the line up front and fixes it. Every test passed; a screenshot did not.
This is the fifth time in this project's record that the demo caught what the
suite couldn't, and the second (after UI Slice 2) where clipping turned an
invisible overflow into a visible one.

The second was subtler and is the more interesting note. The HUD first reported
**steps/frame**, which is exactly right and completely useless: at 75 Hz against
a 60 Hz step the per-frame count strobes `0,1,1,1,1` forever, so a single sample
tells you nothing — and three consecutive screenshots all caught the zero, which
is what prompted looking. A 300-frame probe confirmed the distribution (61 zeros,
239 ones) and that nothing was broken. The readout became a **cumulative** step
count, which is monotone and is the number the frame-rate claim is actually
about. `Timeline::steps_last_frame` was then deleted rather than left unread — a
public accessor with no caller is the speculative build principle 2 forbids, and
the cap biting is already visible as sim time falling behind wall time.

*On the parity risk.* `Timeline` sits entirely above `Clock`, so it needed no
`#[cfg]` and adds no vertex attribute, draw call, or shader edit — there is no
instance-buffer surface here of the kind Slice 8's `first_instance` bug lived in.
It was checked in a browser anyway, and the browser found something real: a
throttled tab accumulates simulation time far more slowly than real time, because
`Clock` clamps each frame to 100 ms. That is the intended behavior — the
alternative is a tab that returns to the foreground and teleports — but it is
recorded in `ARCHITECTURE.md` because "wall time" summed from frame deltas is not
wall time in a tab that wasn't drawing.

*What it exposed.* Nothing new for the engine, which makes two slices in a row
(11 and 12) that end without evidence for a next one. That is worth stating
plainly rather than filling: **the second vertical is complete**, `scene.rs` can
do everything Slices 8–12 were pulled into existence for, and the honest next
move is a demo that hits a wall none of them cover — not another item invented
from this file. The nearest candidates already have their evidence recorded under
*Beyond*: terrain's deferred erosion animation is now unblocked (this slice was
its stated blocker) and waits only on a reason to believe it is worth watching,
and consumer-supplied textures have two independent consumers asking.

*The first of those became **Slice 13**, immediately below, and it is the first
consumer of `fixed_update` other than the demo it was built for.*

## Back to terrain — Slice 13

### Slice 13 — Erosion as a scrubbable time axis ✅ done

*Roadblock:* Slice 7 built the erosion animation, looked at it, and **pulled it**.
Slice 12 then removed its stated blocker, so what was left was the harder half:
"a reason to believe the result is worth watching." That is not a thing this file
could settle — it is a question about what the landscape *looks like* in motion.

**The pulled attempt's diagnosis was right about the cause and wrong about the
cure.** It said the terrain "teleports, just via a blur" and concluded that pacing
had to come off a wall clock. Pacing was necessary and was not sufficient: the
missing piece is that the interesting thing on screen is not the *rock*, it is the
**water**, and a pass count is a position in *time* rather than a parameter.

**It started with a probe, not with code.** The roadmap's own rule from Slice 7 —
*when a simulation looks wrong, measure it before re-tuning it* — applied exactly.
A headless run over 150 passes at 128² said:

| | pass 1 | pass 20 | pass 60 | pass 110 | pass 150 |
|---|---|---|---|---|---|
| movement, % of relief / pass | 0.110 | 0.048 | 0.021 | 0.019 | 0.019 |
| **lake coverage** | **22.6%** | **19.0%** | **13.5%** | **0.0%** | **0.0%** |
| river coverage | 5.8% | 5.7% | 5.8% | 5.9% | 6.0% |
| cells changing wet/dry per pass | — | 0.36% | 0.57% | 0.10% | 0.01% |

Three things fell out of that table, and each one decided a design question:

- **The show is the lakes draining.** Coverage falls monotonically from 22.6% to
  nothing while the river network barely changes extent. That is the arc, it runs
  about 110 passes, and at eight passes a second it is a fourteen-second watch —
  which is what "worth watching" turned out to mean. The rock is nearly static
  after pass 40; had the demo been judged on the terrain alone it would have been
  pulled a second time.
- **The axis has a natural end.** Past pass 110 there is no water left and the
  land lowers at a flat 0.019% a pass forever. `MAX_PASS` is 150 — a little past
  the last interesting thing — so the run *finishes* instead of trailing off.
- **The water needed no special handling.** Only 0.2–0.6% of cells change wet/dry
  in a pass, rivers usually under 0.1%. A soft threshold to stop the river network
  popping was designed and then **not built**, because the measurement said there
  was nothing to fix. Lerping depth and area is enough.

**The second measurement is the one that chose the architecture.** A backwards
scrub has to produce pass 30 after reaching pass 90, and erosion has no inverse.
Recomputing from the cached base is the obvious answer and it was timed first:

| | 1 pass | 60 passes | 150 passes |
|---|---|---|---|
| release, 128² | 1.9 ms | 120 ms | 336 ms |
| **debug, 128²** | **16.8 ms** | **1.0 s** | **2.7 s** |
| debug, 256² | 68 ms | 4.4 s | 11.8 s |

`cargo run --example terrain` is a **debug** build, so every drag of the slider
would have been a multi-second freeze. So the landscape at every pass is simply
kept: `history[pass]`, `4·n²` bytes each, 9.6 MB across the whole axis at 128² and
39 MB at the 256² maximum. **A rewind is an array index**, which is instant in any
build. Spending tens of megabytes to avoid seconds of recompute is not a close
call, and it is only obvious once both numbers are on the table.

*Proof:* `cargo run --example terrain` plays a flooded landscape draining into a
dendritic river network over ~19 s, at 75–86 fps on a 128² grid **with the mesh
rebuilt every frame**. The transport was driven and checked frame by frame: pause
froze the pass at 61 while the fps readout kept moving, three clicks of *step*
gave exactly 62, 63, 64, and dragging *pass* from 150 back to 20 restored the
lakes the mature run had drained — a real rewind, not a re-erode. Verified on
native and on web.

**What shipped, and what it cost:**

- **The engine gained nothing at all, for the second time.** Slice 7 was the first
  slice to buy no capability; this is the second, and it is a stronger result
  because this one *animates*. Everything it needed — a fixed step, a pause, a
  seek, a sub-step fraction — Slice 12 had already shipped for `scene.rs`. No new
  public API, no `Painter` method, no shader edit. A demo that lands on an
  existing seam without moving it is the best evidence that seam was drawn right.
- **`ErosionParams::iterations` was deleted, and that is the slice's real idea.**
  "How eroded" sat among the erodibility and the talus angle as though it were a
  property of the model. It is a *position on a time axis*: the landscape at pass
  60 is not configured differently from the one at pass 30, it is the same
  landscape thirty passes later. Once that is said out loud the batch `erode` has
  no callers and goes too, replaced by a public `step` and a `water_of`.
- **This is the first consumer to use `alpha` the way its docs describe.**
  `Timeline::alpha`'s docs name two cases — evaluate a pose function at a sub-step
  instant, or hold two snapshots and blend them — and `scene.rs` only ever
  exercised the first, because a pose *is* a closed form. A landscape is not:
  there is no formula for "pass 43.6". So terrain blends `history[k]` and
  `history[k+1]`, and the second half of that doc is no longer a claim.
- **Blending is a refinement, and the honest reading is smaller than expected.**
  It was justified by measurement (the fastest cell moves ~half a grid cell in
  pass 1) and it is worth having. But at eight passes a second against 75 fps the
  terrain advances well under one pass per frame anyway, and a single pass changes
  ~0.1% of screen pixels — so the strobing the pulled attempt feared was a product
  of running **75 passes a second**, not of the absence of interpolation. Stated
  plainly because the temptation is to credit the fix rather than the pacing.
- **`step` returns the water it was *given*, not the water it produced**, and that
  pairing is load-bearing. Walking the axis forward costs exactly one flow routing
  per pass, because the far end of this frame's blend becomes the near end of the
  next one. A scrub, which can land anywhere, pays for two. Getting this backwards
  doubles the cost of the common case.

*What the browser found, and it is a real cost rather than a bug.* The demo runs
at **21 fps in a debug wasm build and 50–57 in a release one**, against 75–86
native. The per-frame mesh rebuild that blending requires is the thing that got
more expensive — Slice 8 removed exactly this work for static scenes, and a
deforming landscape is the case that has to pay it back. So
`cargo xtask serve terrain --release` is not optional here the way it used to be,
and that is worth knowing before anyone concludes the web target regressed.

*One reading of the web that is not a defect.* A tab that is not painting advances
the erosion far more slowly than wall time — 2 passes in 18 s rather than 8 a
second. That is Slice 12's documented throttle (`Clock` clamps every frame to
100 ms, so a suspended tab simply banks no simulation time) meeting a demo whose
whole subject is elapsed passes. It looks alarming in a screenshot and is the
intended behaviour; the alternative is a tab that returns to the foreground and
teleports the landscape.

*What it exposed.* Nothing for the engine, and that is now three slices running
(11, 12, 13). The candidate with evidence attached is unchanged and unmet:
consumer-supplied textures, still with two consumers asking and no demo blocked on
it yet.

### Slice 14 — Water that looks like water ✅ done

*Roadblock:* two complaints about the same surface, and they turned out to need
opposite halves of the codebase. The water was a **staircase** — lakes with
axis-aligned edges and rivers made of squares — and it was **dead**, a flat blue
sheet that a wave could not have been seen on even if one had been there.

*The ceiling, stated first because it was asked about.* Real-time water in a
modern engine is planar reflections, screen-space refraction and caustics. All
three need an offscreen render target, which is the `render graph` entry under
*Beyond* and is not what this slice is. What is reachable without one is a
correctly-shaped, correctly-shaded, moving surface — and the distance from a flat
stair-stepped polygon to that turns out to be most of the way.

#### The staircase was a *sampling* problem, not a smoothing one

The old mesh classified whole grid **cells** as wet or dry and drew their corners,
so a shoreline could only ever land on a grid line. No amount of softening fixes
that; the boundary has to be allowed to land *between* samples.

So both kinds of water now write into one continuous **wetness field**, which is
then contoured with marching squares. Lakes write flood depth. Rivers write
something better than a threshold: each `c -> receiver[c]` link of the flow
network is splatted as a **segment with a width**, so a river follows its own
diagonal instead of the grid's, and a trunk carrying thirty tributaries is drawn
wider than they are. `Water` grew a `receiver` field to make that possible — the
network as a *graph* rather than as a per-cell mask.

The same field does a second job: its value is the surface's **opacity**, so the
water fades out as it shallows instead of ending on a line. Between that, sampling
the surface height from `terrain + depth` (which is zero at the waterline, so the
edge sits exactly on the ground), and tapering the lift, there is no seam left to
see.

#### Three bugs, each of which needed measuring rather than guessing

This slice is the clearest case yet for the project's own rule, because **all
three of my first diagnoses were wrong** and each was corrected by an experiment
rather than by thinking harder.

1. **The water came out faint and mottled.** The guess was a stray ε film; the
   measurement was the lake depth distribution, and it was decisive: the median
   lake shallows from `0.064` at pass 0 to `0.0041` by pass 60 — **sixteen-fold**
   — as siltation fills the basins. The opacity ramp had been sized for the
   *typical* depth, which left literally 0% of the lake at full opacity by pass
   60, exactly where the demo spends its time. Sizing it against the shallowest
   water worth seeing fixes it at both ends of the timeline. A fixed ramp is only
   safe once you know the quantity it is measuring is not itself a moving target.
2. **The rivers were strings of little triangular holes.** Guessed z-fighting
   twice — first blaming bilinear-vs-triangulated height sampling (a real bug, and
   fixing it changed nothing visible), then depth precision (wrong by two orders
   of magnitude: `Depth32Float` resolves ~2.5e-5 world units here against a
   0.0025 lift). The actual cause is geometric and exact: fan-triangulating a
   contoured cell splits it along `a–c`, while the terrain splits along `d–b`.
   The corner heights agree; the *interpolation between them* does not, so the two
   surfaces cross inside every cell and half the water is below ground. **No lift
   can fix it**, because the error scales with the quad's twist. Contouring the
   terrain's own two triangles instead of the cell makes both surfaces piecewise-
   planar on the identical partition, and the holes vanish.
3. **The ripples rendered as sharp scratches.** Not geometry at all — a Blinn-Phong
   exponent of 260 against a coherent sine wave train draws the locus where the
   half-vector aligns, which is a set of thin curves. Isolating it by switching the
   shading off entirely is what identified it in one build; a broader exponent and
   gentler slopes turn the same waves into a sheen.

#### What the engine gained, and one reversal

Three slices in a row had cost the engine nothing. This one could not: water reads
as wet almost entirely through **view-dependent** shading, and the fragment shader
had no idea where the viewer was.

- **`CameraUniform` carries the eye**, and the camera bind group became
  `VERTEX_FRAGMENT`. Leaving it at `VERTEX` is a pipeline-creation panic rather
  than a wrong picture, which is the good kind of failure.
- **The lighting model grew a Blinn-Phong specular term and a Schlick Fresnel
  edge**, driven by new `Material` fields. Both default to zero, so every other
  demo renders identically — verified by screenshot before any water work started.
  The Fresnel is honest about being a stand-in: with no second pass it tends
  toward a flat sky colour rather than an image of the scene.
- **`Vertex::color` became RGBA, reversing a decision Slice 10 recorded
  explicitly.** That slice put alpha on the `Material` and argued see-through is a
  property of a *placement*, not of a mesh's corners — "nothing wants per-corner
  opacity". The argument was right and remains right for uniformly translucent
  objects. What it did not cover is a surface whose transparency varies *across
  itself*, which is exactly a shoreline. The two compose: vertex alpha is the
  shape of the transparency, material alpha its overall strength.
- **`Material::blended()`**, because per-vertex alpha is invisible to the pipeline
  choice. Without it, dragging terrain's opacity slider to 1.0 drops the surface
  into the opaque pass and the soft shoreline snaps back to a hard line.
- **Thirteen of WebGL2's sixteen vertex attributes** are now spoken for, up from
  eleven. Recorded on `InstanceRaw::ATTRS`: the next thing wanting per-instance
  data should pack into the spare `w` channels rather than claim a slot.

*Proof:* `cargo run --example terrain` shows lakes with curved, soft shorelines
that fade into their banks, a continuous dendritic river network whose trunks are
visibly wider than their tributaries, and a moving sheen — at 66 fps on a 128²
grid with the mesh rebuilt every frame. The waves run on the **wall** clock, so
pausing the erosion leaves the water moving, which is the demo's clearest
illustration of the split the engine's own time docs draw. Measured rather than
asserted: consecutive frames differ over 4% of the 3D viewport, against 0.04–0.11%
for erosion alone.

*What it cost in frame rate.* 75 fps to 66 at 128². The wetness field, the splat
and the contouring all run per frame on the CPU because the mesh is rebuilt per
frame anyway (Slice 13). That is the honest price of doing water geometry on the
CPU, and it is the strongest argument yet for the shader-side time uniform under
*Beyond* — waves want to be a vertex-shader displacement, not a rebuilt buffer.

*On the parity risk, and this is the weakest claim in the slice.* Checked in a
browser and correct — 56 fps under a release wasm build, with the deep basins at
pass 0 reading dark navy where the mature shallow lakes read pale, which is the
depth palette doing visible work. **But Chrome served it WebGPU, so the WebGL2
fallback was again not exercised**, and this slice is precisely the kind that
lives there: two more instance attributes, a widened vertex attribute, and a
uniform that grew. Thirteen of sixteen is within limits by inspection. That is
reasoning, not a test — the same sentence Slice 9/10 had to write, now with less
headroom behind it.

### Slice 15 — Ripples on the GPU (and the performance that bought) ✅ done

*Roadblock:* Slice 14's water was rejected on sight, for two reasons that turned
out to be the same reason. It **banded** — every lake had straight parallel light
stripes across it — and it was **slow**, 66 fps where the demo used to hold 75.

*Measured first, and the measurement named the culprit immediately:*

| per frame, 128² | before |
|---|---|
| terrain mesh build | 0.76 ms |
| **water mesh build** | **10.2 ms** |

Ten milliseconds against a 13.3 ms budget, for a surface covering a fifth of the
screen. And the largest single item inside it was the wave train: four `sin_cos`
per vertex across ~50,000 vertices, recomputed every frame because the ripples
lived in the *mesh*.

**Both complaints have one cause: the waves were geometry.** Being geometry made
them expensive, and it capped their detail at the tessellation — a normal per
vertex, linearly interpolated across triangles far bigger than a ripple, which is
precisely what draws stripes instead of water.

- **The engine's clock reached the shader.** `CameraUniform` grew a `frame` field
  (wall-clock seconds). This is the "time uniform" *Beyond* has been predicting
  since Slice 7, and it arrived exactly where that entry said it would.
- **`Material` grew `ripple_strength` / `ripple_scale`**, and the fragment shader
  grew a six-octave ripple field. It is framed as generic animated normal detail
  rather than as water, because that is what it is — a moving normal is equally a
  shimmer or a heat haze. **The two parameters cost zero new vertex attributes**:
  they ride the spare `w` channels of the two vectors added in Slice 14, which is
  what the "thirteen of sixteen" note was warning would be necessary.
- **The banding fix is in the octave layout, and each part is load-bearing.**
  Every octave is rotated ~113° off the last so none shares an axis with the grid;
  the frequency ratio is 1.87 so no two octaves share a period; and longer waves
  travel *faster* (real deep-water dispersion), which is what stops the whole
  field sliding as one moiré band. Amplitude falls 0.55 as frequency rises 1.87,
  so every scale contributes the same slope — equal roughness at all scales is
  the property that reads as water.

**Two more CPU fixes, both from the same profile:** the vertex and index buffers
are now sized from the previous frame instead of doubling their way up from empty
(about seventeen reallocations a frame for a count that barely changes), and a
cell whose four corners are all dry is rejected before either of its triangles is
contoured — water covers under a fifth of the map, so that skips most of the grid
on one comparison chain.

*Proof:* **10.2 ms → 2.3 ms** of water mesh build, and **66 fps → 75–80**, which
is faster than the demo ran *before* Slice 14 added water contouring at all. The
stripes are gone, replaced by per-pixel glints that do not depend on how finely
the surface is tessellated. Verified native; 105 tests, clippy clean, both wasm
targets build.

**One tuning pass was wrong and is recorded because it is the same trap twice.**
With ripples per-pixel the specular was pushed to 1.35 on the theory that fine
detail could absorb it. It saturated to flat white across whole lakes — worse than
the stripes. Per-pixel detail makes a highlight *safe to sharpen*, not *safe to
brighten*: shininess went up to 90 and strength came **down** to 0.5.

*What is still missing, stated plainly.* This is a lit, moving, correctly-shaped
surface, and it is not what a modern engine means by water. There is no
reflection and no refraction, so the Fresnel term still tends toward a flat colour
rather than an image of the scene — and at the demo's default camera angle water
is only ~3% reflective anyway, so the honest cue is barely there. Closing that gap
needs an offscreen render target: render the opaque pass to a texture, then let
the blended pass sample it for refraction and a screen-space reflection. That is
the render-graph entry under *Beyond*, it is the next real slice, and no amount of
tuning the current shader substitutes for it.

*It was the next slice, and it is **Slice 16**, below.*

### Slice 16 — An offscreen target, and the render graph that needed ✅ done

*Roadblock:* the one Slice 15 wrote down for itself. The water was lit, moving and
correctly shaped, and it still reflected nothing and refracted nothing, because
the shader had no way to see the scene it was sitting on. That needs the opaque
pass rendered to a **texture**, which needs a frame with more than one dependency
in it — the first thing this engine has built that the hand-wired `render()`
could not hold.

**The frame went from two passes to five**: sky, opaque, composite, blended,
overlay. Only one of those is water; the other two are the price of it.

#### The render graph, and the honest size of it

`renderer/graph.rs` declares resources with a format and passes with what they
read and write, then resolves the order, allocates the textures and re-allocates
them on resize. It is **`pub(crate)` and its pass list is a closed enum**, because
the trigger this file records for a *public* graph is "a second consumer wanting
its own pass" and there still isn't one. What pulled this in was the engine's own
fourth pass, so the engine is the only thing that can add to it.

Two things justified building it rather than adding a third `begin_render_pass`
by hand, and both are about mistakes rather than elegance:

- **A texture cannot be an attachment and a sampled input at once.** This is the
  obvious mistake when the water wants "the scene behind me" and the scene is
  right there, and the graph rejects it at startup. It is also *why* the composite
  pass exists at all: the opaque scene has to be copied to the swapchain before
  the water can draw over it while reading it.
- **Three attachments now have to track the surface size.** `ARCHITECTURE.md`
  already carried the one-attachment version of this as a gotcha learned the hard
  way; the graph makes it one call instead of three places to forget.

**The graph caught its own author inside an hour.** The first ordering rule said a
pass that `Keep`s a target depends on everything else that writes it — which, with
composite, blended and overlay all accumulating onto the swapchain, makes them
mutually dependent, and the cycle check refused to schedule the frame. The fix is
the distinction the rule was missing: a `reads` edge is a **data** dependency and
must order against every writer, while `Keep` is an **accumulation** order and can
only mean "whoever wrote this before me". A startup panic naming two passes is a
better outcome than a frame that composites in the wrong order and looks nearly
right.

#### What shipped in the shader

- **A sky.** One analytic gradient plus a sun, in `common.wgsl`, which is
  textually prepended to both shader modules because WGSL has no `#include` and
  the function has two callers that must agree exactly: the sky pass draws it, the
  water reflects it. Two copies that drifted would present as "the water colour is
  slightly off", which is a miserable thing to trace.
- **Refraction**, which displaces what is seen through the surface and tints it by
  Beer-Lambert absorption over the thickness the depth buffer implies.
- **Screen-space reflection**, marching the depth buffer and falling back to the
  sky when a ray misses or leaves the frame.
- **A second fragment entry point.** `fs_water` samples the scene; `fs_main` does
  not and cannot — the opaque pipeline *writes* that texture, and a shader that
  statically references a binding forces it into the layout. The split is what
  makes the conflict not exist, and it is forced rather than chosen.

#### Three things measured, and two of them were my own bugs

1. **Refraction made the water invisible, and the arithmetic said why before the
   screen did.** Beer-Lambert alone over lakes measured at ~0.004 world units deep
   returns about 2% water and 98% "exactly the scene behind" — a surface that
   composites to the pixel already there. This is *the same trap Slice 14 fell into
   and wrote up*: a fixed ramp against a quantity that is itself a moving target.
   The fix is to take whichever is larger of the absorption and the authored
   wetness alpha, so a deep basin is carried by its depth and a shallow lake by its
   coverage.
2. **The sky came out as grey fog, because those constants are linear.** The
   surface is `Bgra8UnormSrgb`, so the GPU encodes whatever the shader returns and
   a horizon picked by eye at 0.55 displays at 0.77. Every value in `common.wgsl`
   is now chosen in linear space and the file says so, because this will be got
   wrong again otherwise.
3. **SSR's first version was binary speckle, and it took cranking the term to see
   it.** At water's real 2% reflectance nothing about the reflection is visible at
   all — so it was isolated by temporarily raising `f0` to 0.65, which is what made
   the artifacts legible. Three causes, all real: the march used **fixed world
   steps** in a scene whose features are 0.004 units across, the crossing was taken
   at whole-step resolution (which draws banding that grows with the geometric step
   schedule), and the fully rippled normal sent neighbouring pixels to unrelated
   parts of the scene where each independently hit or missed. Steps are now a
   fraction of the *viewing distance* (scale-free, since the engine cannot know how
   big a consumer's world is), the crossing is bisected six times, and the trace
   uses a calmer normal than the one that shades the surface.

#### The result, stated at its real strength

*Proof:* `cargo run --example terrain` draws lakes and a dendritic river network
under a graduated sky, with soft shorelines, per-pixel ripples, depth-tinted water
and a traced reflection — at **75 fps** on a 128² grid, unchanged from Slice 15
despite two extra fullscreen passes. Wireframe still draws everything as opaque
lines. 105 tests, clippy clean, native and wasm both build.

**And the reflection is nearly invisible, which is the honest headline.** Water is
2% reflective face-on, the demo's camera looks down at it, and 2% of anything is
not a picture. The trace is correct, the sky fallback is correct, and at this
camera angle you cannot see either doing much — the artifacts and the payoff both
only appear when you make the material physically wrong. What genuinely improved
the water is **refraction and depth absorption**, which read at every angle. That
asymmetry was predicted before the slice started and is worth having proved rather
than assumed.

*What it cost elsewhere:* **every demo now has a sky** instead of a flat clear
colour. Slice 14 was careful to leave other demos pixel-identical and this one is
not — `scene.rs` and the rest gained a horizon. It looks better and it is a change
that was not forced by the roadblock, so it is flagged here rather than buried: a
`Renderer` knob to turn it off is about ten lines if the uniformity is worth more
than the sky.

*What is still missing.* Refraction reads the opaque scene *behind* the surface,
so it cannot show anything the opaque pass did not draw — no caustics, and nothing
refracted through two water surfaces. The reflection cannot show what is off
screen. Both are the defining limits of screen-space techniques rather than gaps
in this implementation, and escaping them means cube maps or a real reflection
pass, neither of which has a demo asking.

## The third vertical — a scene you can edit (Slices 17–18)

Slices 11, 12 and 13 each ended without evidence for a next one, and Slice 16
closed with its own list of things nothing had asked for. The roadmap's answer to
that state is written into it: *the honest next move is a demo that hits a wall
none of them cover — not another item invented from this file.* So the next slice
was chosen by picking a demo, not a feature.

### Slice 17 — Picking: letting the pointer reach the world ✅ done

*Roadblock:* every demo so far is a one-way street. The consumer computes a scene
and the engine draws it; input has only ever moved the camera or moved a slider
that moved a number. **Nothing has ever asked "what did I just click on?"** — and
the answer needs something the engine could not express.

The wall was visible in the source before a line of the demo was written.
`Camera::view_projection()` returns a `glam::Mat4` and `Camera`'s fields are
`Vec3`, so the only route from a cursor position to a world-space ray was for the
demo to take a `glam` dependency — the exact thing `look_from_to`'s own doc
comment says the API exists to avoid. Meanwhile `CameraUniform` **already carried
the inverse view-projection**, because Slice 16's sky pass needed it. The engine
was computing precisely the matrix picking wants and had no way to hand it over.

The driving demo is **`examples/editor.rs`** — click an object to select it, drag
it across the ground, inspect and edit it in a panel, spawn and delete. Content-
free in the same way `scene.rs` is: no game, no tool chain, just "objects you can
point at, pick up, change, and throw away."

**What the engine gained, and it is deliberately half an answer.**

- **`Ray { origin, direction }`** (`camera.rs`) and **`Renderer::pointer_ray()`**.
  Plain arrays, following the rule `Transform` and `look_from_to` already set.
  That is the entire public surface.
- **The split is the point.** Producing a ray needs the camera, the render
  target's size, and the scale factor — all the engine's. Deciding what the ray
  *hits* needs a model of the scene — the consumer's. So `editor.rs` owns its own
  ray-vs-box test, which is the same ruling that keeps stream-power erosion in
  the terrain demo. A bounding box is a decision about how forgiving clicking
  should be, not a fact about a mesh, and the engine has no business having an
  opinion.
- **`Camera::ray_through_ndc` stayed `pub(crate)`.** Nothing has asked to cast a
  ray through anywhere but the pointer, and a second public entry point would be
  the speculative build principle 2 forbids. It unprojects the near *and* far
  plane and joins them rather than starting from the eye — the eye is not a valid
  ray origin under an orthographic projection, and the near-plane point always is.
- **Four unit tests, for the same reason `Timeline` got eight.** This is pure
  math with no GPU behind it. The one that earns its place round-trips a world
  point through the projection and casts a ray back through the pixel it landed
  on: a flipped Y or the wrong near-plane depth convention both produce a
  plausible-looking ray that quietly selects the wrong object, and no screenshot
  would show it.

**Two predictions this slice made about itself, and how they came out.**

- **Object lifetime would pull in `MeshHandle` removal** — Slice 8 deferred it
  pending "a demo that actually spawns and destroys objects", and this is that
  demo. It did **not** pull it in, and that is the more interesting result: an
  object here *is* a placement of one of three shared meshes, so spawning and
  deleting cost no mesh traffic at all. The deferral was right, and it stays
  deferred until something uploads geometry per object.
- **Showing what is selected would need a per-instance render mode.** It did not,
  after the first attempt failed. A translucent swollen shell — the obvious
  inverted-hull trick — washes the object pale, which destroys exactly the
  property the inspector exists to edit; a hue slider is useless when selecting
  something drains its colour. The fix was **not** an engine feature but a
  **cage**: the twelve edges of the object's bounds, drawn as thin boxes from the
  cuboid the demo had already uploaded. It reads unmistakably as "selected",
  leaves the object's colour alone, costs one draw call, and is re-implementable
  by any consumer from public API — which is the test this file sets before
  anything is pushed down into the engine.

**The bug, and it is the best argument in this document for the web check.**

Picking worked perfectly on native and did **nothing at all** in Chrome. The
cause is not graphics: `handle_pointer` bailed out unless the left button was
held and only *then* looked for a press edge. A human click at 75 fps always
spans a frame, so the two are indistinguishable on the desktop. A browser can
deliver `mousedown` and `mouseup` between one frame and the next — so at frame
time the press edge is set and the held state is already false, and the entire
click is discarded. Handling the press first and asking about "still held" only
for the drag is the fix, and it is recorded in `ARCHITECTURE.md` because it is a
trap for the next consumer that reads a button.

This is the third web-only defect in the project's record (after Slice 8's
`first_instance` and Slice 5's canvas sizing), and the first that has nothing to
do with the GPU. `--target wasm32-unknown-unknown --lib` compiled it happily.

*Proof:* `cargo run --example editor` opens six objects on a ground plane at
75–87 fps. Clicking one selects it — verified with the ray printed beside the
cursor and the hit index, `593,465` → `0.60,-0.50,-0.62` → `hit #1` — draws a
cage around it, and fills the inspector with that object's real values. Dragging
carries it under the pointer across the ground (`x 1.30 → 3.38`, the object
staying where the cursor put it rather than sliding at a rate that depends on
camera angle). Spawn, copy and delete move the object count 6 → 7 → 8 → 7 and
clear the selection. Verified on native and in a browser under `BrowserWebGpu`,
where the same click selects the same object and draws the same cage.

*On the parity risk.* This slice adds no vertex attribute, no shader edit and no
pipeline, so the instance-buffer surface Slice 8's bug lived in is untouched —
and the fourteen-of-sixteen attribute budget is unchanged. Chrome again served
**WebGPU**, so the WebGL2 fallback is still not exercised; that is now the fourth
slice in a row to write that sentence.

*What it exposed, and it was worth chasing.* The whole picture rendered
**noticeably darker on the web than on native** — the ground plane mid-grey in a
native window and near-black in Chrome. Logging the formats settled it in one
run: a WebGPU canvas offers `[Bgra8Unorm, Rgba8Unorm, Rgba16Float]` and **not one
of them is sRGB**, where Vulkan lists `Bgra8UnormSrgb` first. `Renderer::new`
preferred an sRGB format and fell back to `formats[0]`, so on the web every
colour was displayed without its encode.

Fixed here rather than deferred, because native/web parity is a rule this file
sets rather than an aspiration, and because it was one slice's worth of
divergence away from being load-bearing for someone. The surface is configured
with the format it offered and every pipeline renders through an sRGB **view** of
it — `view_formats` exists for precisely this. It is gated on
`DownlevelFlags::SURFACE_VIEW_FORMATS`, since GLES/WebGL cannot re-view a surface
texture, so the WebGL2 fallback keeps the old too-dark behaviour rather than
failing to start.

Two things make the claim checkable rather than plausible: `add_srgb_suffix` is
the identity on an already-sRGB format, so **the native picture is unchanged**
(verified by screenshot before and after), and the web now logs
`surface format: Bgra8Unorm; rendering through Bgra8UnormSrgb`. Side by side the
two targets finally show the same ground, the same sky and the same panel.

*It is worth noting how long this hid.* It predates every slice that looked at a
browser, and each of those looked at **terrain**, which is dark, textured and
judged on shape rather than tone. A flat grey ground plane under six pastel
objects is what made it obvious — which is an argument for a demo whose colours
are boring on purpose.

### Slice 18 — A keyboard that reaches the consumer ✅ done

*Roadblock:* the engine's `Key` enum had **eight** variants, chosen in Slice 3 to
fly a camera, and its own doc comment said to add more only when a consumer
demanded them. One did. The demand did not come from a demo in this repository —
it came from the same second consumer that already has a UI toolkit wishlist, and
it is recorded in
[`slmsttaa-ui/WISHLIST.md` § Input and navigation](slmsttaa-ui/WISHLIST.md#input-and-navigation).
It could not be built around: there is no way for a consumer to reach a key the
engine does not report.

Three things were missing and all three are the engine's: **modifiers** (no
`Ctrl`, no `Shift`, so no shortcut can be bound at all), **typed characters** (a
`Key` is a position, not a letter, and deriving one from the other is how you ship
software that only works on a US layout), and **order** — everything `Input`
carried was a *level*, and a text field cannot be built on levels.

The driving demo is **`examples/editor.rs`**, extended rather than replaced. It
is the demo that already asked the pointer to reach the world; now it asks the
same of the keyboard.

**What the engine gained:**

- **`Key` widened to fifty variants** — all the letters and digits so a consumer
  can bind arbitrary shortcuts, plus the arrows and the editing keys. `COUNT` is
  derived from the last variant rather than typed out, so adding one in the
  middle cannot silently under-size the held-key table.
- **`Modifiers { shift, ctrl, alt, logo }`**, fed from `ModifiersChanged`, with a
  `command()` helper that is Cmd on macOS and Ctrl elsewhere — because a binary
  that hard-codes `ctrl` is wrong on one platform, and the check is one line.
- **An ordered `Event` log** beside the levels, carrying key transitions *and*
  typed characters, drained by `end_frame` with the other per-frame state. This
  is the only ordered thing in `Input` and it exists for exactly one reason: `ab`
  then Backspace and Backspace then `ab` are different, and flags cannot tell
  them apart.
- **`is_key_pressed`** — a press edge with auto-repeat excluded, mirroring
  `is_mouse_pressed`, for one-shot shortcuts. `repeat` still rides on the log,
  because a text field *wants* repeats.
- **`MouseButton::Back` / `Forward`.** Two enum arms and two `winit` arms, and
  the wishlist had named mouse-4 as one of the three ways back was unavailable.
- **Escape became the consumer's to claim.** `Application::quit_on_escape()`
  defaults to `true`, so every demo that has not thought about it is untouched;
  `editor.rs` returns `false` and binds Escape itself, and quits through the new
  `Renderer::request_exit()` — honoured at the end of the frame, so the frame it
  was asked on is still drawn. This is the first piece of *consumer
  configuration* the engine has, and it is deliberately one defaulted trait
  method rather than a config system.
- **A clipboard** (`src/clipboard.rs`), which is the one place this slice cost a
  dependency: `arboard` on native, `navigator.clipboard` on the web. It is
  engine-internal because the toolkit has no dependencies to reach an operating
  system with. Inbound needs no seam at all — a paste is pushed into `Input` as
  ordinary typed characters, so nothing downstream knows a clipboard was
  involved.

**The bug, and it is the second one in this file that only a real keyboard could
find.** Pressing `Ctrl+A` in the demo's name field inserted an `a`; `Ctrl+C`
inserted a `c`. **Windows reports `text: Some("a")` for `Ctrl+A`** — the platform
hands you a shortcut and a keystroke at once, and the toolkit had no way to tell.
The filter belongs here, on the engine's text channel: a keystroke under a
shortcut modifier is not typing. `Ctrl+Alt` is exempt, because that combination
is AltGr on a European layout and types real characters — which is why this is a
rule with an exception rather than a blanket ban on modifiers.

*Proof:* `cargo run --example editor` — objects have names typed into a field,
the scene list filters as you type and walks with the arrows, Escape backs out of
the field and then the selection, Delete removes it, Tab walks the panel and
Enter activates, `Ctrl+A`/`C`/`V` round-trip through the system clipboard, and
the camera stands down whenever the UI is listening. Verified in Chrome under
`BrowserWebGPU`, where the same keystrokes produce the same picture, Tab stays
inside the canvas instead of walking the browser's focus ring, and the console is
clean.

*On the web gap this exposed.* `winit`'s web backend calls `preventDefault()` on
every keydown, which is what keeps Tab and Space out of the page's hands — and
also cancels the default action that produces the browser's `paste` event. So web
paste falls back to the async `navigator.clipboard`, which Chromium gates behind
a permission and Firefox does not offer to web content at all. Copying *out*
works everywhere. It is the first place in this project where native and web are
not equal, it is documented in `ARCHITECTURE.md` rather than papered over, and
the fix is upstream.

**The UI half of this slice is [UI Slice
7](slmsttaa-ui/ROADMAP.md#slice-7--keyboard-focus-and-text-entry-done)** — focus
traversal, keyboard-operable widgets, and the `text_field` that all of the above
exists to feed.

## The fourth vertical — an application layout (Slice 19)

Slice 17 was found by picking a demo rather than by reading this file, and it
worked, so it is the method again. The demo this time is the one shape the
project has never drawn: **a workspace, where the 3D view is one pane beside
other panes** rather than a fullscreen backdrop with UI floating over it.

Terrain, scene, editor, gallery and grid are all the same layout. An application
is the inversion, and it is the shape the project's named second consumer (The
Matchmaker, catalogued in [UI `WISHLIST.md`](slmsttaa-ui/WISHLIST.md)) actually
needs. Choosing it also meant choosing the one *Beyond* entry with a real
consumer attached — which is a coincidence worth noticing rather than a
vindication of the list: the demo picked itself, and the entry happened to be
sitting under it.

### Slice 19 — The scene as a panel among panels ✅ done

*Roadblock:* there was no way to say **where** the scene goes. `Renderer` had
`size()` and `resize()` and an unstated assumption underneath everything else
that the picture filled the window.

The driving demo is **`examples/workspace.rs`** — two panels and the scene in the
space between them. Content-free in the same way `scene.rs` and `editor.rs` are:
a ring of primitives on a ground plane, deliberately boring, because the point is
the layout and boring pastels are what made Slice 17's sRGB bug visible.

**What the engine gained.** Three methods, and the middle one is the slice.

- **`Renderer::set_scene_rect(Option<[f32; 4]>)`** and **`scene_rect()`** —
  `[x, y, w, h]` in **logical points**, `None` for the whole window. Points
  because the caller gets the rect from a UI layout, and because `scale_factor()`
  is private on purpose; a pixel API would have forced it public and handed the
  conversion to every consumer.
- **`Renderer::window_size()`** — the window in points. Added because the demo
  needed something to lay out *against*, and `size()` is pixels. See the bug
  below; this method exists because of it.
- **`Renderer::pointer_in_scene()`** — whether the cursor is over that rectangle.
  The companion to `pointer_ray`, and the split is the same one Slice 17 made:
  "should this click count?" is policy and stays with the consumer, but
  *answering* it needs the cursor in pixels and the resolved rect in the same
  units, which only the engine has.

**The invariant that decided the design, and it is not the obvious one.**

The cheap implementation is to leave the offscreen targets at surface size and
just rasterize the scene into a sub-rect with `set_viewport`. It compiles, it
draws, and it quietly breaks the water. `shader.wgsl` rests on a single
assumption in five separate places — `project`, `depth_at`, `world_from_depth`,
`edge_fade`, and `fs_water`'s screen UV — and the assumption is:

> **the scene textures' extent *is* the camera's NDC frame.** A UV of `[0,1]`
> means the whole texture and the whole picture, and those are one sentence.

Rasterize into a sub-rect of a full-size target and that sentence splits in two.
Refraction samples an offset texel; the reflection marches across the wrong part
of the depth buffer. Both look plausible. Neither is right. So the targets shrink
to the rect instead, all five statements stay literally true, and **not one line
of shader changed**. That is the argument that this restructure is the right one
rather than merely a working one.

**It cost one more pass, and the reason is a wgpu rule rather than a choice.**
With `scene_depth` sized to the rect, the blended pass could no longer write the
window-sized swapchain — every attachment in a pass must share dimensions. So
blended moved onto a new `scene blend` target and a sixth pass, `present`, blits
that to the swapchain under the frame's one and only `set_viewport` call. The
frame is now sky → opaque → composite → blended → present → overlay. `present`'s
load-op clear is what fills the window around an inset scene, so no clear-colour
knob was needed; a consumer wanting a themed surround paints the regions outside
the rect, which is arithmetic it already did to compute the rect.

**The latent bug it existed to find.** `Renderer::pointer_ray` unprojected the
cursor through the **whole window**. That has been wrong since Slice 17 and was
undetectable, because the window and the scene were the same rectangle — the four
unit tests that slice wrote to catch a flipped Y could not see it, since a
rectangle that is the identity hides a wrong rectangle perfectly. Now it maps
through `scene_rect`, and the arithmetic moved into a free `ndc_in_rect` function
with four tests of its own, one of which is just the old behaviour stated as a
failure.

A cursor **outside** the rect still gets a ray, deliberately. The projection is
well defined out there and it is what keeps a drag continuous when the pointer
strays onto a panel — the same courtesy the toolkit already extends to a slider
dragged off itself. Returning `None` was tried first and is the more obviously
"safe" answer; it is also the one that freezes a drag mid-gesture.

**The graph tests, which should have existed two slices ago.** `src/renderer/`
had no tests at all, and this slice rearranged the frame. `resolve_order` is pure
— no device — so `build` was split into `validate` + `resolve_order` + `allocate`
and four tests now assert the frame schedules in the one correct order, that
`present` follows everything that writes the scene, and that a pass reading what
it writes still panics. They duplicate the pass declarations, which is the cost
of testing them without a GPU and the thing to watch: a pass added in `mod.rs`
and not there stops describing the frame.

**The bug, and it is the third slice running where running it found what the
tests could not.** The first run drew two panels and *nothing between them* — the
readout said the scene rect was `1 × 1`. The demo laid out against
`renderer.scene_rect()`, which is already the pane, so each frame computed a rect
slightly inside the last one until it hit the one-pixel clamp. Every test passed;
the screenshot was two panels and a black hole.

The fix is `window_size()`, and the interesting part is that the trap was in the
*documentation*: `scene_rect`'s first doc comment said it was "the sane thing to
measure a first layout against", which is true exactly once and wrong every frame
after. An API whose output is nearly the input it wants is one a consumer will
feed back into itself, so both methods now say which is which.

*A second, smaller one, also found by clicking.* The demo gated picking on
`ui.wants_pointer()`, copied from `editor.rs`. That value is last frame's — it is
only knowable once the panels have been declared, which happens after picking.
In a fullscreen demo the staleness is invisible, because the cursor sits over the
UI for many frames before a click lands. In a workspace the panels and the scene
are disjoint, so the first click after every panel interaction was swallowed.
`pointer_in_scene()` is the fresh question and the right one here; `wants_pointer`
stays for scroll, where "a widget is active" matters and a frame of lag does not.

*Proof:* `cargo run --example workspace` opens a `810 × 664`-point pane at
`(256, 12)` between two panels, aspect `1.22`, with the window around it cleared
to black. The spheres are round in that pane and round again at aspect `1.78`
when "fill window" is ticked, which is the aspect check. Clicking the cyan sphere
selects `#0` and cages it; clicking the far cube selects `#4`; clicking the
"light" checkbox on the panel changes the theme and leaves the selection alone.
Toggling "fill window" runs the `None` ↔ `Some` transition and the texture
re-allocation behind it, live, with no log output and no dropped frame.

*Regression, and this is the claim worth checking rather than asserting.*
`examples/triangle.rs` renders **pixel-identical** to the commit before this one —
zero differing pixels — which covers sky, opaque, composite and present. Terrain
run to its settled state (pass 150/150) differs by 0.6% RMSE, and a difference map
shows that difference confined **exactly to the water bodies** and the fps digits:
the ripple field is driven by wall-clock elapsed time, so two runs are never in
phase. Terrain is the only demo with water and therefore the only exercise of the
blended pass; that its silhouette, absorption and refraction land in the same
pixels is the evidence the restructure did not move it. `editor.rs` still picks
correctly, unmodified.

*On the parity risk, stated plainly rather than implied.* The wasm target builds,
`cargo xtask serve workspace` packages and serves it, the page loads under Chrome,
the WebGPU adapter is acquired and the surface configures through the sRGB re-view
with no console errors over twenty seconds. **The rendered pixels were not
checked**, because this environment's headless Chrome captures a blank canvas —
and the control is that *unmodified* `terrain` captures the same blank canvas from
the commit before this slice. So the web check is a build-and-boot check this
time, not a picture. The pane is deliberately asymmetric top-to-bottom for
whoever does look at it: a vertically centred rect hides a flipped Y completely,
and the GL path blits through an offscreen framebuffer. WebGL2 remains
unexercised, now for the fifth slice running.

*What it exposed.* One thing, and it is a UI question rather than an engine one.
The demo computes its pane by arithmetic over the theme's margin and its own two
panel widths, because `Ui::panel` anchors floaters and reserves nothing. That is
the correct answer under principle 3 and it is four exact lines — but it is also
the first time a consumer has had to ask "what is left over?", and a second one
asking is what would promote a `Ui::remaining()`. Recorded in the UI roadmap as
declined-with-a-reason, not scheduled.

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

## Slice 20 — a consumer's own window ✅ done

*Roadblock:* the smallest one yet, and the first taken directly from a shipping
consumer rather than from a demo. [UI `WISHLIST.md` §
Engine-side](slmsttaa-ui/WISHLIST.md#engine-side-not-this-crate) records it in one
line: **`run(app)` takes no configuration, so a shipped game's window says
"SLMSTTAA".** Trivial, and noticed by every player of anything built on this.

- `Config` — title, initial size, and three window flags.
- A defaulted `Application::config()` the engine reads once, before the window
  exists.
- `app::resumed` stops hard-coding a title and a size.

*Proof:* four tests in `src/config.rs`, one of which pins `Config::default()` to
the exact string and numbers `app::resumed` used to carry — so this slice cannot
silently move an existing consumer's window. Native and wasm both build.

**What shipped, and what it cost:**

- **It went on the trait, not on `run`.** `Application` already had a defaulted
  `quit_on_escape()`, so the convention existed and a second `run_with` entry
  point would have been two functions doing one job. The engine asks
  `dyn Application` and a consumer it has never heard of answers — the same
  inversion everything else here uses.
- **The default is the old hard-coded values, exactly.** Adopting this changes
  nothing until a field is set, which is why no existing example was touched.
- **Three of the five fields are speculative, and are labeled as such.** Only
  `title` had a consumer behind it; `size` came along because it was hard-coded
  on the adjacent line; `resizable` / `decorations` / `maximized` were added
  because introducing a struct is the cheapest moment to add fields. That is the
  stopping rule bent rather than followed, so it is written down in `Config`'s
  own rustdoc rather than filed as infrastructure.
- **`title` is `Option<String>`, and the `None` is the interesting part.**
  Natively `None` and `Some("SLMSTTAA")` are the same window. On the web they
  are not: the engine writes `document.title` only for `Some`, so a page keeps
  the `<title>` its own HTML gave it unless a consumer actually asked to be
  renamed. Writing the default there instead would have replaced
  `web/index.html`'s caption with a blander engine string on every demo — the
  fix making the thing it fixed worse. A test pins the two apart.
- **Naming the tab is the engine's three lines, not winit's.** winit's web
  backend puts `with_title` on the canvas's `alt` attribute — accessibility
  text, not a caption — so `Config::title` would have been silently inert for
  the one thing anyone expects it to do. `size` is ignored there twice over
  (winit drops `with_inner_size` on the web, and the canvas is viewport-sized),
  and the three flags describe a decoration a page does not have.

**What verifying it exposed — two things, neither of them this slice's fault.**

- **A consumer cannot quit on the web; it can only crash.** `request_exit` sets a
  flag, the loop calls `event_loop.exit()`, and on wasm winit unwinds by
  *throwing* the sentinel exception documented under
  [ARCHITECTURE § Gotchas](ARCHITECTURE.md). `web/index.html` catches that
  exception around `init()` — but this throw happens inside an animation-frame
  callback, which that catch does not cover, so it surfaces as an uncaught error.
  This predates the slice by a long way: every demo with the default
  `quit_on_escape` has behaved this way on the web since exit existed, and
  `editor.rs` only makes it visible because it rebinds quit to `Q`. The real
  question underneath is what "quit" should *mean* in a tab, where nothing can
  close the page — most likely a no-op with a logged warning rather than an
  unwind. Recorded rather than fixed, because it is a behaviour decision and not
  a bug in this slice.
- **A plausible platform claim in `Config`'s own docs was wrong, and it was
  hiding half a feature.** The docs said the title "crossed over" to the web.
  Checked against winit 0.30's source rather than assumed: its web backend sets
  the canvas's `alt` attribute, not `document.title`. So the slice as first
  written left the wishlist item **half solved** — a shipped game's *tab* still
  said whatever `index.html` said, which is the exact complaint the item was
  filed about. It was caught because the tab was looked at, the title on it was
  correct, and the correctness turned out to be a coincidence: it came from a
  static `<title>` in `web/index.html`, and the demo being served had no
  override to observe in the first place. **A verification that cannot fail is
  not a verification** — this one passed by construction and would have
  certified nothing. The fix is above; the general lesson is to check what the
  observed value is actually produced *by*.

**No bundled example overrides its title, and that is a constraint rather than an
oversight.** `cargo xtask shoot` finds the window with `import -window SLMSTTAA`
(`xtask/src/main.rs`), an exact-name match — so renaming any demo's window breaks
the capture harness. Making the harness resolve the window by id through
`xdotool` first would remove the coupling and is worth doing; it was not done here
because it cannot be exercised from a Windows workstation, and changing
verification infrastructure you cannot run is how you find out later. The feature
is proven by the trait and its tests; the demos are still called SLMSTTAA because
they *are* SLMSTTAA.

## Slice 21 — pixels a consumer supplies ✅ done

*Roadblock:* [`examples/terrain.rs`](examples/terrain.rs) wanting a top-down
thumbnail in its parameter panel. The 3D view shows one corner of a basin from one
angle, and the thing the erosion demo is *about* — where the water is, and where it
stops being — is a property of the whole map. The demo owns a heightmap and can
shade one; it had no way to put pixels on the screen. This is the entry *Beyond*
has carried as **consumer-supplied textures** since it was written, and the second
of the two demanders it predicted (the other being the UI crate's textured quads —
they turned out to be the same slice).

- **`Renderer::create_image(width, height, rgba) -> ImageId`** and
  **`Renderer::update_image(id, rgba)`**, beside `upload_mesh`/`update_mesh` and
  with the same contract: append-only, handles never invalidated, nothing ever
  freed.
- **A second texture in the overlay's existing bind group**, with a 1×1 dummy
  bound until a consumer supplies one, so the whole UI is **still one
  `draw_indexed`**.
- **`MODE_IMAGE` in `overlay.wgsl`**, tinting by the vertex color.

*Proof:* terrain's minimap — a 160² shaded-relief thumbnail with the water mask,
regenerated as the erosion runs, in `capture/terrain-plot.script`'s four shots.
Native and wasm both build.

**What shipped, and what it cost:**

- **One atlas, not many, and that is a real restriction rather than a stage.**
  The overlay's single draw call binds one texture; N images means splitting the
  index run at every texture change, which gives up a property `ARCHITECTURE.md`
  has claimed since UI Slice 1. So a frame may draw one image, a consumer packs
  what it needs into one sheet and addresses it with `uv`, and a frame that asks
  for a second gets an **edge-triggered** warning and a skipped primitive. Edge-
  triggered because this project already has 170,897 lines of evidence about what
  a per-frame complaint does to a log.
- **`Rgba8Unorm`, not `Rgba8UnormSrgb`, and the fallback is what settles it.** The
  overlay writes into a target that encodes, so a `Color` component is linear and
  an image texel has to arrive as exactly `byte / 255` to sit in the same space as
  the panel around it. On the WebGL2 path the surface cannot be re-viewed as sRGB
  and nothing is encoded — the documented, visible wrongness. `Rgba8Unorm` keeps
  an image wrong in precisely the same direction and amount as every color beside
  it; sRGB would have made it wrong in a *second, different* way on one backend
  only, which is worse than either.
- **The dummy texture is load-bearing, not tidiness.** The layout declares the
  binding whether or not anything ever draws an image, and a bind group with a
  missing entry does not validate. Binding the *glyph atlas* there instead would
  have validated — it is a filterable float texture too — and then quietly drawn
  the font wherever an image belonged.
- **The pipeline layout is fixed at construction; only the bind group moves**, and
  only when a frame draws a different image than the last one did. A UI that draws
  one image or none rebuilds nothing after startup.
- **`ImageId` is declared in `slmsttaa-ui`**, which is the opposite of what the
  keyboard did, deliberately. `Key` is declared on both sides and translated
  because nothing outside the engine ever holds one; a consumer holds an `ImageId`
  between creating an image and drawing it, so two types would have cost it a
  conversion in its own code for no benefit. The id is `Painter` vocabulary, like
  `Color` and `Rect`.

**What it deliberately does not do:** no destroy, no resize, no mipmaps, no second
sampler. `update_image` is same-size only — a thumbnail that regenerates every pass
wants one texture rewritten, and terrain's is a **fixed** 160² with the heightmap
resampled into it precisely because its grid moves between 32 and 256 and a
per-resolution image would leak one texture per drag of the slider.

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
built ahead of need: MSAA. (Transforms, a lighting model, and a minimal material
moved out of this list and into Slices 8–12 above, because `scene.rs` demands
them; the render graph and "water that looks wet" left it in Slice 16, because
terrain did.) Each of the rest waits for a consumer to ask:

- ~~**Picking / hit-testing**~~ — **landed as Slice 17.** Never listed here, which
  is worth noting: it was found by choosing a demo rather than by reading this
  file, and the seam it needed (`Renderer::pointer_ray`) took nine lines. The
  items below have been sitting here longer and are still waiting for the same
  thing — a consumer that is actually blocked.
- ~~**Consumer-supplied textures**~~ — **landed as [Slice
  21](#slice-21--pixels-a-consumer-supplies-done).** This entry predicted two
  demanders, "a 3D demo wanting surface detail that per-instance color can't
  express, and the UI crate's request for textured quads", and said the shader
  work was adjacent to the glyph atlas. The shader work *was* adjacent — one mode
  and one binding. What it got wrong is that it read as two separate demands
  needing two answers, and only one of them ever arrived: terrain wanted a
  **thumbnail of its own data**, which is neither surface detail nor an icon, and
  answering it answered the UI request in the same breath because a picture in a
  panel and an icon in a panel are the same primitive. The 3D half — a texture
  sampled by the *scene* shader — is still unbuilt and still has no demander.
- **An asset pipeline (glTF/OBJ + runtime file loading).** Explicitly *not* taken:
  Slice 11 answers "geometry the consumer didn't compute" with primitives plus
  transforms instead, which costs zero dependencies and no wasm asset-fetching
  story. Revisit only when a demo needs authored art that primitives genuinely
  cannot compose.
- **Skeletal animation (joints, vertex skinning, clip playback).** Recognized and
  deferred whole. It is the single largest item any renderer consumer will ask
  for, and it is close to pointless without the asset pipeline above — you cannot
  author a rig with no importer. Slices 8–12 deliberately stop at rigid objects a
  consumer poses itself each frame; that is animation *by the consumer*, and it is
  as far as we go until a demo proves it insufficient.
- ~~**Water that looks wet**~~ and ~~**a render graph**~~ — **both landed**, over
  Slices 14–16, and the sequence is worth keeping because this entry predicted it
  almost exactly. It said waves and Fresnel wanted "a time uniform and somewhere to
  perturb the normal" (Slices 14–15) and that a reflection wanted "an offscreen
  target, which is the next entry" (Slice 16). It also warned that CPU-animating
  the water mesh "should stay an experiment rather than a slice" — Slice 14 did it
  anyway and paid 10 ms a frame, which Slice 15 then reclaimed. The entry was right
  and was read too late.
- ~~**An offscreen render target composited into a UI rect**~~ — **landed as
  Slice 19.** This entry is worth keeping for how wrong it was about the size of
  the job. It said "what remains is letting that composite target an arbitrary
  rect", which is true and is about a fifth of it. It did not see that the
  offscreen targets would have to shrink to the rect (the water's screen-space
  math assumes the texture's extent *is* the camera's frame), that shrinking them
  would evict the blended pass from the swapchain and cost a sixth pass, or that
  `pointer_ray` had been unprojecting through the wrong rectangle since Slice 17.
  The lesson is the one Slice 17 already recorded: an entry on this list is a
  recognition, not an estimate, and the demo is what finds the actual work.

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

UI Slice 6 (animation) cost one field on the same terms: `UiInput` gained `dt`,
filled from `Renderer::dt`. Nothing else — a fading color is still a color and a
collapsing section is still a clip rect, so hover fades, a growing slider knob,
smooth scrolling and animated accordions all landed without the overlay learning
that anything moves. It is worth noting what the engine did *not* have to
provide: no animation system, no easing curves, no timeline. It handed over a
number of seconds.

UI Slice 5 (typography) is the exception, and interesting for being one: it is the
only slice so far to make the seam **narrower**. `text_size` left the `Painter`
trait entirely and `src/renderer/font.rs` was deleted, because two independent
implementations of "how wide is this string" agreed only by the accident of a
monospace font and would have silently disagreed the moment the advances became
proportional. The engine no longer owns a font; it uploads the toolkit's atlas and
draws the quads it is given. A seam that can be *cut back* when a capability moves
above it is as good a sign as one that absorbs a change without moving.
