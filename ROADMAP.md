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
`move`/`click`/`press`/`release`/`wheel`/`key` steps keyed to those same frames —
`press` and `release` being separate is what lets one *drag*, since a button held
across two checkpoints is a button held across real frames.

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

- **Committed golden images, and `shoot --check`** — **scheduled as [Slice
  24](#slice-24--the-first-golden-images)**, and the driver is worth naming
  because it is not a demo: Slice 23 and Slice 25 both make their central claim by
  photographing every demo before and after and asserting nothing moved, and doing
  that by hand twice is what promoted this. Everything needed was already here —
  captures are exact, and CI (`.github/workflows/ci.yml`) supplies the "something
  runs it unprompted" half that used to be the blocker. What argued for waiting was
  the *review* cost rather than the running cost, plus one measured limit: a shot
  taken after an input step is stable within a build and not across one (see the
  synthetic-input entry below), so a golden of a scripted click would fail on an
  unrelated commit. That limit does not go away — it scopes the slice to the two
  input-free shots, which are already exact. The `.gitignore` keeps
  `capture/*.script` and drops the PNGs, so the shape is ready.
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
- ~~**A press and a release, so a script can drag**~~ — **done.** Found writing
  `capture/terrain-plot.script` for UI Slice 10, and it was the wheel's story
  again: `Action::Click` is a press *and* release with no update in between, so
  `Response::held` was never true for a single frame — and **every drag widget in
  the toolkit acts on `held`**. A button could be clicked; a slider could not be
  moved at all, which put the transport's rate, both erosion `log_slider`s and
  every numeric knob in every demo out of the harness's reach. That is the one
  entry on this list that was blocking verification of code *already shipped*
  rather than enabling something new, which is why it went first.

  The grammar gained `press` and `release` as separate steps, and `click` became
  the pair — defined once in `Harness`, not re-implemented per platform, so a
  scripted click and a scripted drag cannot drift apart. `mouse_move` on Windows
  now carries `MK_LBUTTON` while the button is down, because the button flags
  ride every mouse message and a move that does not set them is a jump rather
  than a drag.

  *Proof:* `capture/terrain-drag.script` grabs the `passes/sec` knob and pulls it
  across the track — 8, then **18 photographed mid-drag with the button still
  down**, then 27. The control matters as much as the result: the identical
  script with `click` in place of `press`/`move`/`release` leaves the readout at
  8 and the knob where it started. Two runs are still byte-identical, including
  the shots taken during the drag, so the reproducibility this section advertises
  survived a new input verb.
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
existence by the next thing a demo cannot do.

**Finished slices live in [`ROADMAP-DONE.md`](ROADMAP-DONE.md)** — each one's
roadblock, what landed, what it cost, and what it deliberately left out. They are
the precedent this file argues from, so they are linked rather than deleted. This
file keeps the method, the harness, the slices not yet built, and the plan.

- [Slice 0 — Invert control (bootstrapping)](ROADMAP-DONE.md#slice-0--invert-control-bootstrapping--done)
- [Slice 1 — Mesh + indexed drawing](ROADMAP-DONE.md#slice-1--mesh--indexed-drawing--done)
- [Slice 2 — Depth buffer + culling](ROADMAP-DONE.md#slice-2--depth-buffer--culling--done)
- [Slice 3 — Camera the consumer can drive](ROADMAP-DONE.md#slice-3--camera-the-consumer-can-drive--done)
- [Slice 4 — The terrain vertical (the thesis)](ROADMAP-DONE.md#slice-4--the-terrain-vertical-the-thesis--done)
- [Slice 5 — On-screen UI: a debug/HUD text overlay](ROADMAP-DONE.md#slice-5--on-screen-ui-a-debughud-text-overlay--done)
- [Slice 6 — Layered terrain rebuild + wireframe render mode](ROADMAP-DONE.md#slice-6--layered-terrain-rebuild--wireframe-render-mode--done)
- [Slice 7 — A water surface: lakes and rivers](ROADMAP-DONE.md#slice-7--a-water-surface-lakes-and-rivers--done)
- [Slice 8 — Per-object transforms + an instance draw-list](ROADMAP-DONE.md#slice-8--per-object-transforms--an-instance-draw-list--done)
- [Slices 9 & 10 — Lighting and material (taken together)](ROADMAP-DONE.md#slices-9--10--lighting-and-material-taken-together--done)
- [Slice 11 — Primitive mesh builders](ROADMAP-DONE.md#slice-11--primitive-mesh-builders--done)
- [Slice 12 — Fixed-timestep clock + time control](ROADMAP-DONE.md#slice-12--fixed-timestep-clock--time-control--done)
- [Slice 13 — Erosion as a scrubbable time axis](ROADMAP-DONE.md#slice-13--erosion-as-a-scrubbable-time-axis--done)
- [Slice 14 — Water that looks like water](ROADMAP-DONE.md#slice-14--water-that-looks-like-water--done)
- [Slice 15 — Ripples on the GPU (and the performance that bought)](ROADMAP-DONE.md#slice-15--ripples-on-the-gpu-and-the-performance-that-bought--done)
- [Slice 16 — An offscreen target, and the render graph that needed](ROADMAP-DONE.md#slice-16--an-offscreen-target-and-the-render-graph-that-needed--done)
- [Slice 17 — Picking: letting the pointer reach the world](ROADMAP-DONE.md#slice-17--picking-letting-the-pointer-reach-the-world--done)
- [Slice 18 — A keyboard that reaches the consumer](ROADMAP-DONE.md#slice-18--a-keyboard-that-reaches-the-consumer--done)
- [Slice 19 — The scene as a panel among panels](ROADMAP-DONE.md#slice-19--the-scene-as-a-panel-among-panels--done)
- [Slice 20 — a consumer's own window](ROADMAP-DONE.md#slice-20--a-consumers-own-window--done)
- [Slice 21 — pixels a consumer supplies](ROADMAP-DONE.md#slice-21--pixels-a-consumer-supplies--done)
- [Slice 22 — a continent, and the sea its rivers reach](ROADMAP-DONE.md#slice-22--a-continent-and-the-sea-its-rivers-reach--done)
- [Slice 23 — the viewpoint six demos agreed on](ROADMAP-DONE.md#slice-23--the-viewpoint-six-demos-agreed-on--done)
- [Parity, checked rather than reasoned about](ROADMAP-DONE.md#parity-checked-rather-than-reasoned-about)

The sections below are the slices that are **written up but not built**, then the
forward plan.

## Slice 24 — the first golden images

*Roadblock:* [Slice 25](#slice-25--the-fallback-draws-water-too)'s central claim
is "eight demos, before and after, byte-identical on the backend that already
worked" — and there is no way to state that except by hand. Slice 23 made the same
claim for six demos by hand. Twice is the point at which *The harness*' own
**golden images / `shoot --check`** entry stops being recognized-but-undriven, and
it is a weaker pedigree than a demo roadblock, which is why it is written down here
rather than assumed.

**This slice was first written as Slice 25, after the degrade, and moved ahead of
it.** The order contradicted *Block 0*'s own rule — goldens that land after the
work they are meant to watch certify none of it — so they go first, and the
degrade is the first change CI watches rather than the last one checked by hand.

Everything the entry was waiting on has arrived: captures are exact rather than
merely close (0.6% RMSE by hand, zero through the harness), and CI exists, so
"something runs it unprompted" is no longer the blocker.

**Except that "exact" turned out to be "usually exact", and that is this slice's
real first task.** During the October dependency upgrade, `terrain` — input-free,
frame 120, identical binary — came back in **one of two states**: byte-identical
to the reference in most runs, and in roughly one run in four exactly 88,096
pixels off, every one of them on a water surface (sea, lakes, river fills) and
none on land or UI. Two discrete outcomes rather than noise points at a race
that settles one way or the other — something the water reads (ripple time, or
which bake pass the lake surface reflects) is not fully on the pinned clock.
The other seven demos were stable across five runs each. A golden of `terrain` would
fail CI a quarter of the time on no change at all, and **a check that fails
without cause gets ignored**, which is worse than no check. Find the race before
committing the first golden, or leave `terrain` out of the first set and say so.

- **`cargo xtask shoot <example> --check`** — capture, compare against a committed
  PNG, exit nonzero with the differing pixel count on a mismatch.
- **A `capture/golden/` directory that is *not* gitignored**, unlike the rest of
  `capture/`, holding the input-free shots only.
- **A CI job** running the check for those, beside the four Definition-of-Done
  commands already there.

*Related, and worth deciding while depth is open:* `src/camera.rs` multiplies
glam's projection by `OPENGL_TO_WGPU_MATRIX`, whose comment says glam targets
OpenGL's `[-1, 1]` depth. It does not — the `rh` projection it calls is the
`directx` one, already `[0, 1]` — so depth is remapped twice and lands in
`[0.5, 1]`, spending half the depth range and feeding the water's depth
reconstruction a narrower band than it thinks. Removing the matrix is a one-line
fix that **moves every pixel the water touches**, which is exactly why it belongs
just after the goldens land rather than before: the diff is then a deliberate,
reviewed re-baseline instead of noise inside an unrelated change.

**Open, and the slice's first decision: whose pixels are golden.** "Exact" has
only ever been measured on one machine against itself. CI is `ubuntu-latest` with
no GPU, so it would render through lavapipe, and a lavapipe frame is not going to
be byte-identical to one from a desktop GPU — rasterization rules, blending
precision and texture filtering all leave the spec room. Either the goldens are
lavapipe's (committed from a CI artifact, and a local `--check` on a real GPU is
expected to fail), or they are per-platform, or the check tolerates a threshold —
and a threshold reopens the question this slice exists to close.

**Input-free shots only, and the limit is measured rather than cautious.** UI
Slice 10 established that two runs of a scripted click are byte-identical *within*
a build and not across one, so a golden taken after an input step would fail on an
unrelated commit. The two input-free shots are already exact and are what a first
golden should be. Extending to scripted shots waits on the synthetic input queue
that same entry describes — this slice should not pretend to have solved it.

*Proof (the target):* a deliberately broken commit — nudge a light constant — turns
CI red, and the failure names the demo and the pixel count. A green run on an
unrelated change is not proof of anything on its own, which is the trap this
whole idea has to clear: **a check that cannot fail certifies nothing**, exactly
as Slice 20's window title did.

*What it does not do, and this is the entry's oldest caveat:* it proves pixels did
not move, which is a different claim from "this is right". Every bug in this
file's list was found by a person noticing something was **wrong** rather than
merely **different**. A golden makes the second cheap and does nothing for the
first.

## Slice 25 — the fallback draws water too

*Roadblock:* **the first one in this file that is broken rather than absent.**
Every finished slice was pulled by something a demo could not do yet. This one is
pulled by something the engine used to claim it could do and cannot: [*Parity,
checked rather than reasoned about*](ROADMAP-DONE.md#parity-checked-rather-than-reasoned-about)
forced a GL adapter for the first time, found two walls stacked on
each other, fixed the first, and left the second as a design question. This slice
answers it.

The GL backend does not advertise `DownlevelFlags::READ_ONLY_DEPTH_STENCIL`, so
the read-only depth attachment the blended pass rests on does not exist there, and
wgpu refuses the pass rather than permit the aliasing. `src/renderer/mod.rs:649`
warns about this and then walks straight into it.

**And it is not terrain's problem, it is every demo's.** The blended pass is
declared in the graph unconditionally (`src/renderer/mod.rs:896`) — `record_draws`
skips its *contents* when nothing is transparent, but the pass is still begun, and
beginning it is what fails. So a demo with no water at all goes down with the
ones that have it. This was first written as reasoning from the graph, and has
since been **measured**: under `SLMSTTAA_BACKEND=gl` on Windows, `triangle` — no
water, no transparency — panics on the first frame exactly as `scene` and
`terrain` do, with wgpu reporting `scene depth` used as `RESOURCE` and
`DEPTH_STENCIL_WRITE` in one usage scope. It is the whole fallback, not a water
feature.

Two documents are wrong until it is fixed, which is what makes this urgent rather
than merely open: `.claude/CLAUDE.md` instructs `SLMSTTAA_BACKEND=gl` when touching
shaders, bind groups, or the instance buffer — a command that cannot currently
succeed — and `ARCHITECTURE.md` describes a WebGL2 fallback that does not start.

**The ruling is to degrade**, over the second colour target and the depth copy.
The copy may not be expressible either (`DEPTH_TEXTURE_AND_BUFFER_COPIES` is
another flag WebGL2 may lack, so it risks buying a second investigation and no
fix). The second colour target is the one that keeps the water identical
everywhere, and it is declined for a specific reason rather than for cost: it makes
**every** backend pay an attachment and the opaque pipeline carry an extra output,
so the path that works subsidises the path that does not. Degrading spends nothing
anywhere except where the capability is genuinely missing.

There is already a template for it one screen up in the same function.
`can_review_surface` (`src/renderer/mod.rs:697`) reads `SURFACE_VIEW_FORMATS` and
falls back to a linear surface where it is absent — documented, visibly wrong on
that one target, and fatal on none. This is the same shape, and the fallback being
*visibly* worse rather than silently different is the property to preserve.

### What lands, concretely

The frame stays six passes. `scene_color` is not the problem and never was — the
composite pass already copies it precisely so the blended pass can read one texture
and write another. **Only `scene_depth` is both attached and sampled**, so only
`scene_depth` moves:

- **The blended pass stops binding depth as a resource** where the flag is absent:
  it declares `.reads(&[scene_color])` alone, and attaches depth *writably* with
  the pipeline's existing `depth_write_enabled: false` doing the work instead. The
  distinction is worth stating because the two look identical and only one needs
  the flag — `Pass::depth(.., false)` means **"attach read-only"**, which is a
  claim about the attachment; `depth_write_enabled: false` is a claim about the
  pipeline, and every backend has always allowed it. An ordinary alpha-blended
  pass is legal on WebGL2; ours is not, and this is the only reason why.
- **A 1×1 dummy depth texture** fills the layout's third entry for that pass, so
  the bind group *layout* is unchanged and there is one blend pipeline rather than
  two. This is Slice 21's ruling reused verbatim — "the dummy texture is
  load-bearing, not tidiness" — and it is why this slice adds no shader
  permutation.
- **Two of the three terms need no shader change at all.** `fs_water` already
  gates both on strength (`if (refraction > 0.0)` at `shader.wgsl:447`,
  `if (reflection > 0.0)` at `:514`), and `reflected` is *already* initialised to
  `sky_color(dir)` on the line before the second one. So the engine zeroes
  `Material::absorption` and `Material::reflection` as it packs the instance
  buffer, and the shader takes paths it already has: `density = 0` makes
  `absorbed` zero so alpha falls back to the authored coverage, and
  `reflection = 0` skips the march and keeps the Fresnel edge reflecting sky.
- **One shader branch, for refraction's occlusion guard** (`shader.wgsl:471`),
  which is the single remaining `depth_at` call on the surviving path. It costs a
  per-instance signal, and `location(13)`'s fourth channel is documented as "the
  only spare per-instance float left anywhere in this buffer" — so the attribute
  count stays at fourteen of WebGL2's sixteen and the budget
  `ARCHITECTURE.md` tracks does not move.

**Refraction survives, and that is the find.** Both this roadmap and
`ARCHITECTURE.md` describe the degrade option as dropping "refraction, absorption
and reflection" — three terms — and that was written from the frame's shape rather
than from the shader. Reading the shader says otherwise: refraction needs
`scene_color` for its sample and depth only for the guard that stops a displaced
sample dragging a rock standing *in* the lake across the surface in front of it.
Losing the guard costs a thin smeared band around such objects. It does not cost
the term — and `shader.wgsl:445` records that at terrain's default view the surface
is only ~3% reflective, which makes refraction "the term doing most of the work of
looking wet". **The fallback keeps the term that matters most, and the estimate in
both files was pessimistic by the largest one.**

*What the fallback actually looks like, and it is not hypothetical:* water with
coverage-driven alpha, ripples, a specular glint, a Fresnel edge reflecting sky,
and refraction without its guard. **That is very close to the water Slice 14
shipped** — before Slice 16 added the scene-reading terms — which ran for two
slices, was photographed, and looked fine. The degrade target is a picture this
project has already published rather than a guess about how bad it gets.

*Proof (the target):* `SLMSTTAA_BACKEND=gl cargo run --example terrain` starts and
draws water. All eight demos start under a forced GL adapter, which is the claim
the parity section could only make for `SLMSTTAA_LIMITS=webgl2`. And the load-
bearing half: `cargo xtask shoot --check` ([Slice
24](#slice-24--the-first-golden-images)) on the Vulkan path is **byte-identical
across the change for all eight**, because an adapter with the flag must take
exactly the path it took before — the same shape of proof Slice 23 used, and the same reason
it is the right one for a change whose entire purpose is to not be visible where
things already worked.

*What this deliberately does not do:* no runtime toggle, no per-material opt-out,
and no way for a consumer to ask which terms it got. The degrade is keyed on an
adapter capability and nothing else, for the reason `capture.rs` and
`src/backend.rs` already give — a knob only a parity check turns would widen the
surface `examples/triangle.rs` exists to measure. A demo that wants to *see* the
fallback forces the backend, which is what that variable is for.

*The honest limit:* this makes the fallback run. It does not make the fallback
checkable in a browser — headless Chrome still photographs a blank canvas from a
WebGPU surface, and a GL browser is not available in a cloud session either. The
`SLMSTTAA_BACKEND=gl` path on a desktop GL driver is the instrument, and it is a
proxy for WebGL2 rather than WebGL2 itself.

## What comes next

Everything above this line was written *after* the work, or immediately before it
by a demo that had already hit the wall. **Everything below it is written ahead of
all of it**, which makes it a different kind of document, and the difference is
worth stating before the content rather than discovering in it.

### What this section is allowed to be, and what it isn't

Two of this file's own rules are in play. One is kept and one is bent, and keeping
them straight is what stops a plan turning into the wishlist this project was
built to avoid.

**"Sequenced capability, not dates" — kept.** There are no weeks here, no dates,
and no estimates of duration. The list is **strictly ordered** and nothing more:
each block is expected to start when the one above it finishes, whenever that is.
That is not a formality — it is the only honest way to plan work that happens in
bursts, and a plan with week numbers on it would be wrong by the second burst and
would then get ignored wholesale rather than followed partially.

**Principle 2 — bent, and the shape of the bend is the whole design.** The unit
below is a **demo**, never a feature. Each block names the demo first, and the
capability under it is what that demo is *predicted to demand*. That preserves
the thing principle 2 actually protects: the demo still decides, and a prediction
that the demo does not hit gets deleted rather than built. What is genuinely new
is that the predictions are written down before the demo exists instead of
after — so this section can be **wrong**, which nothing above it can be.

Three consequences follow, and the file already has evidence for all three:

- **These are recognitions, not estimates.** Slice 17 said it first ("it was found
  by choosing a demo rather than by reading this file"), and *Beyond*'s
  offscreen-target entry proved it hardest — it described about a fifth of the job
  Slice 19 turned out to be, and did not see three of the four things that made it
  expensive. Assume the same error bar here.
- **The tail is expected to slip, and is ordered so it can.** The last block is
  the one to lose. Nothing in it blocks anything above it.
- **A block that ends with "and the demo demanded nothing" is a success**, not a
  wasted one. Slice 7 is the precedent: it bought the engine no capability at all,
  said so outright, and that was the slice's real output.

**On the wishlists.** Three files hold recognized-but-unbuilt items — this one's
*Beyond* and *The harness*, [`slmsttaa-ui/ROADMAP.md`](slmsttaa-ui/ROADMAP.md)'s
*Waiting on a roadblock*, and
[`slmsttaa-ui/WISHLIST.md`](slmsttaa-ui/WISHLIST.md). Items are **not** scheduled
below because they are on those lists; they appear only where a demo below is
expected to pull them, and the ones no demo pulls are accounted for at the end
rather than quietly dropped.

**On the UI half.** UI slices are planned in
[`slmsttaa-ui/ROADMAP.md`](slmsttaa-ui/ROADMAP.md), as always — this section names
them in one line each where a block depends on them and does not restate them.

---

## Block 0 — the ground to stand on

*No demo. This is the block that makes every block after it checkable, and it
goes first for one reason: **the goldens have to exist before the work they are
supposed to watch.*** Arriving after the three verticals, they would certify
nothing that had already happened, which is most of the value.

**[Slice 24](#slice-24--the-first-golden-images)** and **[Slice
25](#slice-25--the-fallback-draws-water-too)** are written up above. In order: the
goldens first, by the rule just stated, and then the WebGL2 degrade — the only
thing in this document that is *broken* rather than absent, and the first change
the goldens get to watch, which is how it proves it changed nothing on the backend
that already worked. (The draft had these the other way round, which broke that
same rule on its first application.)

### Slice 26 — a photograph of a window that is already running

*Roadblock:* three separate entries in
[`WISHLIST.md`](slmsttaa-ui/WISHLIST.md#engine-side-not-this-crate), all filed by
the second consumer, and they are one roadblock wearing three coats: **this
project cannot see what that consumer sees.**

- **`shoot` cannot photograph a binary it did not build.** It takes an example
  name and looks under `target/…/examples/`. The capture *protocol* has no such
  limit — it is entirely environment-driven and lives in `src/capture.rs`, and has
  been demonstrated printing `slmsttaa: capture 60` from a repository this one has
  never heard of. Only the naming is missing: an `--exe <path>` that skips the
  build.
- **`shoot` cannot photograph a build that is already running**, which is the more
  useful half. A script photographs what its author predicted; the entire value of
  looking at a window is the thing nobody predicted. **Six bugs on file here were
  caught by a human looking, and not one by a script that knew where to click.**
  This needs none of `capture.rs` — no pinned clock, no frame numbers, no stdin
  handshake — just the `PrintWindow` path `xtask/src/harness/win32.rs` already
  has, aimed at a live process.
- **A per-frame failure logs one line per frame.** That consumer's first run
  produced **170,897 lines and 512 KiB in eighteen minutes** and never presented a
  frame. The volume is the trivial cost; the real one is that half a megabyte of
  one repeated line reads as noise to skim past. Edge-trigger it with a count, and
  give `Renderer` a surface-health signal so a consumer can assert the state.

*Also here, and it is a one-line coupling worth removing while the harness is
open:* `shoot` finds the window by the exact name `SLMSTTAA`, so any demo that
uses [Slice 20](ROADMAP-DONE.md#slice-20--a-consumers-own-window--done)'s `Config::title` breaks
the harness. Resolve the window by process instead.

*Why this belongs in Block 0 rather than in the dossier vertical it serves:*
[UI Slice 8](slmsttaa-ui/ROADMAP.md#slice-8--a-header-that-lines-up-with-its-body--done)
recorded that a fix pulled by a wishlist gets a weaker demo, *"written afterward
to check the fix rather than to discover the need"*. A harness that can shoot the
second consumer's screens is the one thing that would let a wishlist entry arrive
with a picture of the wall attached — which is worth more before three blocks of
work than after them.

---

## The fifth vertical — light that moves

**The demo:** the terrain island, or `scene.rs`'s stage, under a sun that travels.
Not a time-of-day *system* — a light with a direction the consumer sets, and the
shadows that immediately become the obvious missing thing once it moves.

*Why this one first among the three:* it is the largest visible return per slice
of anything on any list, it improves **every demo already in the repo** rather
than only its own, and unlike the other two verticals its first roadblock is
already written down as a deliberate omission rather than predicted. It is also
the block most likely to be finished inside a short burst.

### Slice 27 — a sun a consumer can move

*Roadblock, and it is the rarest kind here — a demander arriving for a decision
that was already made and recorded.*
[Slices 9 & 10](ROADMAP-DONE.md#slices-9--10--lighting-and-material-taken-together--done) shipped
the lighting model with the light as a **shader constant**, and said exactly why:
*"No demo asked to move the sun, and a setter with no caller is the speculative
build principle 2 forbids."* A demo about a moving sun is that caller. The
constants are still the ones terrain used to bake by hand, which is what will make
the before/after meaningful.

Expected to be small: a direction, a colour, an ambient level, reaching the
existing camera uniform or one beside it.

### Slice 28 — a shadow map

*Roadblock:* a moving sun with no shadows reads as a light level changing rather
than as a time of day, and terrain is the worst case for it — a landscape's
relief is *legible* almost entirely through what the sun cannot reach.

**Predicted, and flagged as prediction:** a depth-only pass rendering the scene
from the light, into an offscreen depth target, sampled by the scene shader.

Three things are already known about it rather than guessed, which is why this is
the block with the most confidence behind it:

- **The render graph is about to be used for what it was built for.** `graph.rs`
  resolves pass order from declared reads and writes, and has carried six passes
  since Slice 16 without a new one arriving. A shadow pass writes a depth
  resource that the opaque pass reads — a genuinely new edge, and the first real
  test of whether "declared, not sequenced" holds.
- **`Camera` cannot express the light's view, and this is verifiable today.**
  `Camera::view_projection` is glam's `rh::proj::directx::perspective`
  (`src/camera.rs:87`), and a directional sun wants an **orthographic** projection
  (`directx::orthographic` sits beside it in the same glam module). `pointer_ray`'s own
  rustdoc already records the asymmetry — the eye position is not a valid ray
  origin under an orthographic projection — so the type has thought about this
  case once and declined it. Expect that to be the slice's actual shape.
- **The depth binding work is done.** Slice 25 is about to establish depth bound
  as `unfilterable-float` and readable as a plain `sampler2D` on both backends,
  which is exactly what sampling a shadow map needs. That is a dependency, and it
  is why this block sits after Block 0 rather than beside it.

*What it is expected to expose:* shadow acne and edge aliasing, which is the first
thing that has ever argued for **MSAA** — carried in *Beyond* since that section
was written, with no demander. Recorded as a prediction so that if the demo does
*not* demand it, the entry stays where it is.

---

## The sixth vertical — the continent from the air

**The demo:** fly over the 2048² island rather than orbiting it. A slow pass from
the mountain core out to the coast, which is the one view that shows what Slice 22
actually built and the one the orbit camera cannot give.

*Why second:* it is a direct continuation of a slice that already hit two walls
(a 780 MB vertex buffer refused outright, and an orbit camera whose outer clamp
sat inside the mountains — *"the first screenshot of the big continent is a
photograph of that bug"*). It also benefits from shadows existing, since relief
seen from the air is the case that needs them most.

### Slice 29 — a camera that flies

*Roadblock:* [Slice 23](ROADMAP-DONE.md#slice-23--the-viewpoint-six-demos-agreed-on--done) pushed
`Orbit` down after six demos wrote it out separately. There is no second
controller, and orbiting is exactly the wrong verb for a continent — you cannot
get inside a valley, and distance-from-a-pivot is not how anyone looks at a
landscape.

`Fly` beside `Orbit`, on the same terms: public fields, an `Input`-shaped gate
struct, `drive` written against the same public `Input` a consumer reads, and the
frame-rate tests Slice 23 established (one 1-second step and a hundred 10 ms steps
landing in the same place). Slice 23's *"what has every demo written out?"*
question does not apply — this is the first consumer, not the seventh — so the
honest framing is that `Orbit` set the pattern and `Fly` is the second instance
of it.

### Slice 30 — where the frame actually goes

*Roadblock:* **[The harness](#the-harness)' own perf-capture entry, with its
driver finally attached.** It has sat there since Slice 14 spent 10 ms a frame
CPU-animating water — *"noticed as a feeling before it was measured"* — and the
whole of Slice 31 below is a guess about where time goes that nobody can currently
check.

Frame timings emitted alongside the frames a capture already takes, so a
before/after is a number rather than an impression. Slice 22 measured pass costs
(3.6 ms at 128², 1.6 s at 2048²) **by hand, once**; this is that, repeatable and
attached to the picture.

*Ordered before culling, and that is a recorded swap.* The draft put this after
it — the argument being that culling is where the demand for the instrument
becomes undeniable, and building it sooner is the speculative build — and said
that if the order were reversed, the reversal should be written down rather than
done quietly. It was reversed on UI Slice 9's lesson: that slice expected layout
to be the bottleneck and found eight of eleven milliseconds somewhere nobody had
looked. Culling is the optimisation most likely to be aimed at the wrong cost, so
the instrument comes first.

### Slice 31 — draw only what is in view

*Roadblock:* predicted, and it is the one most likely to arrive early and hard.
The terrain is **two meshes** covering the whole map, and `record_draws`
(`src/renderer/mod.rs:1774`) iterates the entire draw list every frame with no
per-object rejection anywhere in the engine — `Face::Back` in the pipelines is all
the culling there is. From above that is fine. From inside it means the whole
continent is submitted to draw a valley.

Expected to want: per-instance bounds and a frustum test, which is engine-shaped
plumbing every consumer wants; and **chunking the terrain mesh**, which is the
demo's own arithmetic and stays there (principle 3 — the same ruling that keeps
the erosion solver out of the engine).

*The honest uncertainty:* it may turn out that 2048² submits fine and the real
cost is elsewhere entirely. That has happened before, and recently — [UI Slice
9](slmsttaa-ui/ROADMAP.md#slice-9--only-lay-out-the-rows-you-can-see-done)
expected layout to be the bottleneck, measured it, and found **eight of eleven
milliseconds were two quadratic scans nobody suspected**. Which is why the slice
before this one is the instrument, and this slice starts by reading it.

---

## The seventh vertical — a dossier beside a scene

**The demo:** an in-repo screen shaped like the second consumer's — a sortable
roster, a dossier pane, a chart, and the 3D scene as one panel among them, with
panes the user can resize. `examples/workspace.rs` is the seed; this is that demo
grown until it is genuinely dense.

**This is the block that exists to be lost**, and it is last for that reason. It
is also the one carrying the most UI work, which means most of it is planned in
[`slmsttaa-ui/ROADMAP.md`](slmsttaa-ui/ROADMAP.md) rather than here.

*Why build a proxy for a consumer we cannot run, instead of just answering its
filed requests?* Because [UI Slice
8](slmsttaa-ui/ROADMAP.md#slice-8--a-header-that-lines-up-with-its-body--done)
already ran that experiment and wrote down the result: a fix pulled by a wishlist
entry gets a demo written *afterward*, to check the fix rather than to discover
the need, and *"a demo written that way supplies no surprise"*. A dense screen in
this repo, that `cargo xtask shoot` can photograph, is the only way to get the
surprise back — and **[UI Slice
9](slmsttaa-ui/ROADMAP.md#slice-9--only-lay-out-the-rows-you-can-see-done) is the
proof it works**: the demo was grown to 5,000 rows first, and the wall turned out
to be somewhere nobody had looked.

**What it is expected to pull, UI side** — each already recognized, none
currently scheduled, all planned over in the UI roadmap:

- **Splitters / resizable panes.** WISHLIST calls this *"the sharpest conflict on
  the list"* — a multi-pane workspace is the native idiom of the genre, and the
  toolkit's answer is currently "no". `set_scene_rect` takes a new rectangle every
  frame for free, so the engine half already exists; the toolkit half does not.
  This is the item this vertical exists to force a decision on.
- **`fit_text` / ellipsis** — asked for twice, declined twice, and *fully
  unblocked* since Slice 5 (`…` is in the atlas, `font::text_width` is exact). The
  missing piece was never the code, it was a caller whose strings are not its own
  to shorten. A roster is that caller.
- **A tab ring a virtualized container can extend** — the one item on any list
  found by a demo rather than recognized in advance, opened by UI Slice 9 and
  confirmed at 1,400 rows in the real consumer.
- **Sort arrows** (`▲`/`▼` are not in the atlas; `▶` is), and whichever of
  **dropdown / tooltip / tabs** the screen genuinely needs — with the roster
  itself being the thing this project is most at risk from, per the UI roadmap's
  own stopping rule.

### Slice 32 — a frame the engine can decline to draw

*Roadblock:* predicted, and the interesting part is **which side of the seam it
lands on**. WISHLIST files reactive repaint as a toolkit concern
([*Runtime behavior*](slmsttaa-ui/WISHLIST.md)), and it cannot be one: **the
engine owns the event loop**, so "do not redraw when nothing changed" is the
engine's to decide and the toolkit's only to inform.

The precedent is exact and recent. Keyboard input was also filed as a toolkit
demand, and [Slice 18](ROADMAP-DONE.md#slice-18--a-keyboard-that-reaches-the-consumer--done) plus
UI Slice 7 landed together with **the engine's half being the larger one** —
which WISHLIST itself flags as the thing *"nothing in this file predicted"*.
Expect the same here.

*The honest status of the demand:* weaker than it looks, and the UI roadmap says
so with a number. Virtualization took a static 1,400-row screen to **0.3 ms a
frame and a constant 196 draw commands**, which retired most of the urgency the
entry was filed with. What is left is real but smaller: 0.3 ms of CPU and a full
GPU wake, sixty times a second, to show nothing new — a battery argument about the
*frame*, not about the rows in it. If this vertical slips, this slice slips with
it and nothing suffers.

---

## What this plan does not pull, and why

The three verticals above do not touch most of what is recognized, and listing the
remainder is the difference between a plan and a claim of completeness.

**Still waiting, and correctly so — no demo above hits them:**

- **An asset pipeline and skeletal animation** (*Beyond*). Together they are larger
  than this entire plan, and the deferral reason is unchanged: you cannot author a
  rig with no importer, so they are one item pretending to be two. `scene.rs`'s
  figures keep walking on the spot.
- **Textures sampled by the *scene* shader** (*Beyond*). Slice 21 answered the UI
  half; the 3D half still has no demander, and none of the three demos above is one.
- **Engine-side readback** and **a synthetic input queue** (*The harness*). Both
  get *more* attractive once goldens exist — readback would make a capture callable
  from a `#[test]`, and the input queue is what would let a golden survive a
  scripted click across a rebuild, which Slice 24 explicitly cannot do. Neither is
  scheduled, because Slice 24 is deliberately scoped to input-free shots and
  nothing above needs more.
- **Web capture.** Still environmental — headless Chrome photographs a blank canvas
  from a WebGPU surface. Until that moves, the web half of the Definition of Done
  stays a build-and-boot check, and slices should keep saying so.
- **Numeric entry, kerning, a third weight, mark-to-base positioning, a transport
  widget, `Ui::remaining()`, golden-file *layout* snapshots** (UI roadmap and
  WISHLIST). Each has a recorded reason for waiting that none of the demos above
  changes.

**One loose end that is a decision, not a slice:** a consumer cannot quit on the
web, it can only crash — `event_loop.exit()` unwinds by throwing winit's sentinel
exception from inside an animation-frame callback, which `web/index.html`'s catch
does not cover. It predates [Slice 20](ROADMAP-DONE.md#slice-20--a-consumers-own-window--done)
and every demo with the default `quit_on_escape` has behaved this way since exit
existed. The question underneath is what "quit" should *mean* in a tab, where
nothing can close the page — most likely a logged no-op. Worth settling in
whichever block is open when it next annoys someone.

## Beyond (seams, not commitments)

Listed only so we recognize them when a future demo demands them — **not** to be
built ahead of need: MSAA — whose first plausible demander is now named, in
[Slice 28](#slice-28--a-shadow-map): shadow-map edges are the first thing in this
project that would argue for it, and the entry stays here until that demo actually
does. (Transforms, a lighting model, and a minimal material
moved out of this list and into Slices 8–12 above, because `scene.rs` demands
them; the render graph and "water that looks wet" left it in Slice 16, because
terrain did.) Each of the rest waits for a consumer to ask:

- ~~**Picking / hit-testing**~~ — **landed as Slice 17.** Never listed here, which
  is worth noting: it was found by choosing a demo rather than by reading this
  file, and the seam it needed (`Renderer::pointer_ray`) took nine lines. The
  items below have been sitting here longer and are still waiting for the same
  thing — a consumer that is actually blocked.
- ~~**Consumer-supplied textures**~~ — **landed as [Slice
  21](ROADMAP-DONE.md#slice-21--pixels-a-consumer-supplies--done).** This entry predicted two
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
