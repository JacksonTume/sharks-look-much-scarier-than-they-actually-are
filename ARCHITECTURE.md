# Architecture

SLMSTTAA is a thin layer over [`wgpu`](https://wgpu.rs/) (the Rust WebGPU
implementation) and [`winit`](https://github.com/rust-windowing/winit) (cross-
platform windowing). The same source builds two ways:

- **Native** — desktop window via winit, GPU via Vulkan / Metal / DX12.
- **Web** — a `<canvas>` via winit's web backend, GPU via WebGPU (with a WebGL2
  fallback), shipped as a `wasm-bindgen` module.

`slmsttaa` is a **library** (the engine). Consumers are separate programs that
implement the `Application` trait and call `run(app)`; they live in `examples/`
(Cargo compiles each as its own crate that can only see the public API, so the
engine/consumer boundary is enforced by the build). `examples/triangle.rs` is the
smallest reference consumer; `examples/terrain.rs` is the default web build, and
any example can be served with `cargo xtask serve <name>`.

## Module map

```
src/
├── lib.rs            Crate root. Logging and the run(app) entry point.
├── application.rs    The Application trait (init/update) — the IoC seam a
│                     consumer implements. The engine only sees dyn Application.
├── app.rs            App: winit ApplicationHandler. Owns the window, the
│                     Renderer, and the boxed Application; routes events; drives
│                     the redraw loop and calls the consumer's hooks.
├── camera.rs         Camera (perspective look-at) + CameraUniform (the GPU
│                     payload: the mat4x4 view-projection, the world-space eye,
│                     and wall-clock seconds since start). look_from_to lets a
│                     consumer aim it with plain [f32; 3] arrays. The last two
│                     fields are there because the *fragment* stage needs them —
│                     see Shading the 3D pass below.
├── input.rs          Input: per-frame keyboard/mouse state, decoupled from
│                     winit. Exposes engine Key/MouseButton enums (never winit's);
│                     the event loop feeds it, the consumer reads it via
│                     Renderer::input(). Also absolute cursor position + mouse
│                     press-edges, for screen-space UI hit-testing.
├── time.rs           Two clocks. Clock: a cross-platform frame clock
│                     (delta-time). Native Instant; web performance.now()
│                     (Instant panics on wasm). Surfaced as Renderer::dt().
│                     Timeline: the fixed-timestep simulation clock built on it
│                     — accumulates the wall delta and pays it out in identical
│                     steps, with pause / scale / single-step / seek and an
│                     interpolation alpha. Surfaced as Renderer::time(). It
│                     touches no platform API, so it needed no #[cfg] and is
│                     unit-tested without a GPU.
└── renderer/
    ├── mod.rs        Renderer: wgpu instance/adapter/device/queue/surface, the
    │                 solid + blended + wireframe render pipelines (RenderMode
    │                 and per-instance alpha select between them), the depth
    │                 buffer, the mesh registry + instance draw-list, the overlay,
    │                 the UI state + both clocks, and per-frame
    │                 begin_frame()/update()/render().
    │                 Renderer::ui() also translates Input + the surface size ->
    │                 ui::UiInput, which is what keeps the UI crate free of any
    │                 dependency on the engine.
    ├── mesh.rs       Mesh (vertices + indices): the CPU-side geometry a consumer
    │                 builds and hands over via Renderer::upload_mesh.
    ├── primitives.rs Mesh::plane/cuboid/sphere/capsule — content-free geometry
    │                 with correct normals, the deliberate alternative to an
    │                 asset pipeline. The engine's only unit tests live here,
    │                 because this is its only GPU-free code.
    ├── instance.rs   Transform (position/euler rotation/scale), MeshHandle,
    │                 Material (RGBA tint + the view-dependent shading terms:
    │                 specular, shininess, Fresnel + its tint, ripple strength
    │                 and scale, and a blended override), and Instance: where an
    │                 uploaded mesh is drawn and how it looks. Plus the private
    │                 InstanceRaw + its instance-step buffer layout, including
    │                 the 3x3 inverse-transpose normal matrix. Every shading term
    │                 defaults to zero, so a material that asks for none of them
    │                 shades exactly as it did before they existed.
    ├── vertex.rs     Vertex (position + normal + RGBA color) and its buffer
    │                 layout. The alpha is the *shape* of a surface's
    │                 transparency; Material::tint's alpha is its strength.
    ├── shader.wgsl   3D vertex/fragment shaders (WGSL): one directional light
    │                 with Lambert diffuse, a Blinn-Phong specular term, a
    │                 Schlick Fresnel edge, and the animated ripple field that
    │                 perturbs the normal per pixel.
    ├── overlay.rs    Overlay: the screen-space 2D pass (UI/HUD). Owns its own
    │                 pipeline and dynamic 2D buffers, and uploads the *toolkit's*
    │                 glyph atlas; implements ui::Painter. Drawn after the 3D pass
    │                 (see Frame lifecycle).
    └── overlay.wgsl  2D vertex/fragment shaders for the overlay.

                      (font.rs is gone as of UI Slice 5 — the font moved to
                      slmsttaa-ui/src/font/, for reasons under Text below.)

examples/
├── triangle.rs       Reference consumer: implements Application and uploads one
│                     triangle. Native fn main + a #[wasm_bindgen(start)] hook.
├── cube.rs           Spinning solid cube (Mesh::cuboid): proves indexed meshes,
│                     depth testing, and back-face culling. Uploaded once; the
│                     spin is one Transform per frame.
├── scene.rs          The second vertical (Slices 8-11): articulated figures
│                     walking on the spot, each a different Material tint, built
│                     from engine primitives with no vertex array in the file.
│                     Limbs are composed parent-into-child with Transform::then,
│                     which is why they stay attached while a figure turns. Four
│                     meshes, four draw calls, zero vertex uploads per frame —
│                     the HUD reports all of it.
├── gallery.rs        Multi-scene switcher. Uploads every scene up front and
│                     points the draw-list at one of them; on the web it builds DOM
│                     buttons (web-sys) that drive the selection, on native it
│                     auto-cycles.
├── grid.rs           Orbitable height-mapped terrain grid: proves the input +
│                     camera seam (Slice 3). Keeps its own orbit state and aims
│                     the camera from Renderer::input() each frame.
└── terrain.rs        The capstone and default web build: layered, iterative
    terrain/          terrain. A Perlin-noise base heightmap (heightmap.rs) is
    ├── heightmap.rs  carved by a stream-power landscape-evolution model
    └── erosion.rs    (erosion.rs: priority-flood flow routing + drainage-area
                      incision + thermal relaxation) into dendritic valley
                      networks, with a live engine-drawn UI panel to tune every
                      layer and a wireframe toggle. Erosion is a *time axis*
                      rather than a parameter: the demo steps it on
                      fixed_update, keeps every pass in a history array so a
                      rewind is an array index, and blends consecutive passes
                      with Timeline::alpha. Its water is a separate contoured
                      surface — both lakes and rivers write a continuous wetness
                      field that is marching-squared over the terrain's own
                      triangles, so shorelines curve and fade instead of
                      staircasing. All of it lives in the demo (pulled in via
                      #[path]); the engine only uploads the mesh, picks
                      solid/wireframe, shades the water, draws the UI, runs the
                      camera and the clock.

slmsttaa-ui/          The UI toolkit, as its own zero-dependency workspace member
├── README.md         (see its README for why, and for the dependency-direction
├── ROADMAP.md        rule). The engine depends on it and re-exports it as
├── WISHLIST.md       `slmsttaa::ui`; it never depends on the engine, so it sees
├── src/              no wgpu and no winit. UI planning lives in its ROADMAP,
│   ├── lib.rs        not the engine's. Ui: the id stack, the public
│   │                 allocate/interact/painter seam, the region stack, and the
│   │                 panel / row / column containers.
│   ├── painter.rs    Painter (the drawing seam), Layer (the four ordered draw
│   │                 buckets), + RecordingPainter, the headless test double that
│   │                 makes layout assertable without a GPU.
│   ├── interact.rs   UiInput (this frame's pointer, viewport size, and dt,
│   │                 filled in by the host), UiState (hot/active/focused,
│   │                 collapsed sections, scroll offsets, panel rects, and the
│   │                 eased per-widget floats), and the Response every widget
│   │                 returns.
│   ├── layout.rs     Rect + the stack of layout regions widgets are placed in.
│   ├── theme.rs      Theme: semantic color tokens plus the radius/spacing/type/
│   │                 control scales, with Variant and Size. Public, so a widget
│   │                 written by a consumer can match the built-in ones — and no
│   │                 widget anywhere names a literal color.
│   └── widgets/      One file per widget: text.rs, button.rs, slider.rs.
└── tests/            The project's only tests — layout + ids + hit-testing +
                      theming, driven against RecordingPainter. No GPU, no window,
                      no async.

xtask/                Dev tooling (a separate workspace member, no deps). `cargo
└── src/main.rs       xtask serve [example]` builds the example natively and for
                      wasm, runs wasm-bindgen into web/pkg/ as app.js, and serves
                      web/ from a built-in static server. No Python required.
```

## Frame lifecycle

1. `run(app)` boxes the consumer as `dyn Application`, then builds a `winit`
   event loop **parameterized over `Renderer`** as its user-event type and hands
   control to the platform-appropriate runner.
2. On `resumed`, `App` creates the window. On the web it also mounts the canvas
   and sizes it (see gotchas).
3. The `Renderer` is built asynchronously (GPU init is async) and delivered back
   into the loop. Native blocks on it; the web spawns it and reports completion
   via an `EventLoopProxy<Renderer>` user event. Both paths funnel through
   `App::on_renderer_ready`, which resyncs the surface and then calls the
   consumer's one-time `Application::init` (where it uploads geometry).
4. Between redraws, keyboard/mouse `WindowEvent`s are folded into the renderer's
   `Input` snapshot (see *Input flow* below).
5. Each `RedrawRequested`: `Renderer::begin_frame()` ticks both clocks (so
   `Renderer::dt()` is fresh), clears last frame's overlay geometry, and returns
   how many **fixed steps** this frame owes. The loop calls
   `Application::fixed_update(dt)` that many times — zero while paused, several
   after a slow frame — which is where a consumer advances simulation state and
   the reason a run reproduces at any frame rate. Everything after this happens
   exactly once per frame, because it is rendering rather than simulation. Then
   `Application::update` builds the frame (reading `Renderer::input()`,
   driving the camera via `Renderer::camera_mut`, and building its UI via
   `Renderer::ui()`). Then `Renderer::update()` re-uploads the camera uniform, and
   `Renderer::render()` records **two** passes into one command encoder: the 3D
   pass (clear color + depth → one instanced `draw_indexed` per mesh in the
   draw-list, opaque runs first and blended runs after, using the solid, blended,
   or wireframe pipeline per the current `RenderMode` and the run's alpha)
   and then the
   **overlay pass** (load, not clear → draw the accumulated 2D UI), and
   presents. Finally `Input::end_frame` clears the per-frame deltas/press-edges.
   Depth testing and back-face culling are on for the 3D pass; the overlay ignores
   depth and alpha-blends on top.
6. `about_to_wait` requests another redraw, so we render continuously
   (`ControlFlow::Poll`).

## Input flow

The engine owns the event loop, so a consumer never touches `winit` (roadmap
principle 1). Input is funneled instead:

1. `App::window_event` maps each keyboard/mouse `WindowEvent` onto the renderer's
   `Input` via `pub(crate)` methods (`on_keyboard`, `on_mouse_button`,
   `on_cursor_moved`, `on_scroll`). These do the winit→engine translation, so the
   winit types stop at the engine boundary.
2. `Input` keeps two kinds of state: **held** keys/buttons that persist across
   frames, and **per-frame deltas** (mouse motion, scroll) that accumulate within
   a frame. Its public getters speak only in engine `Key`/`MouseButton` enums.
3. The consumer reads it in `update` via `Renderer::input()` and moves the camera
   through `Renderer::camera_mut()`. The *control scheme lives in the consumer*
   (e.g. `grid.rs`'s orbit math); the engine only exposes the input and the camera.
4. After the frame is drawn, `Input::end_frame` zeroes the per-frame deltas and
   press-edges (held state survives), so the next `update` sees only that frame's
   motion.

The **frame clock** that was once deferred here now exists (`time.rs`,
`Renderer::dt()`): the terrain demo's FPS readout needed frame-rate-independent
timing. It is the wasm-safe `Instant` this note
anticipated — native `Instant`, web `performance.now()` (`std::time::Instant`
panics on wasm). Key-driven camera motion still uses a fixed per-frame step;
nothing has demanded converting that yet.

**There are now two clocks, and picking the wrong one is the easy mistake.**
`Renderer::dt()` is wall time and should drive anything that must keep moving
while the simulation is stopped — the FPS readout, and the UI's hover fades and
collapse animations (`Renderer::ui()` fills `UiInput.dt` from it deliberately: a
paused scene must not freeze a fade half-way). `Timeline`, reached through
`Renderer::time()`, is simulation time, and the `dt` handed to
`Application::fixed_update` is the same number on every machine. A consumer that
advances state only in that hook is frame-rate independent; the engine's
guarantee stops exactly there and does not extend to making the consumer itself
deterministic.

The wall clock now has a **third surface, and it is on the GPU**: `Renderer`
sums the clamped frame deltas into `elapsed` and ships it in `CameraUniform` as
`frame.x`, so a shader can animate surface detail without the consumer touching
its mesh. It is deliberately the same wall time as `dt()` rather than simulation
time — pausing terrain's erosion leaves its water rippling, which is the split
above made visible in one frame. A consumer that wants detail to freeze with the
simulation is not served by this and should drive the geometry from
`fixed_update` instead.

One consequence worth knowing before it surprises someone: `Clock` clamps a
frame to 100 ms, so a throttled background browser tab accumulates *simulation*
time far more slowly than real time. That is the intended failure — the
alternative is a tab that returns to the foreground and teleports — but it means
"wall time" measured as a sum of frame deltas is not wall time in a tab that
wasn't drawing.

## Shading the 3D pass

All of it lives in `shader.wgsl` and is driven by `Material`; there are no
pipeline permutations and no shader graph. One directional light plus ambient,
both fixed in the shader because no demo has asked to move the sun, and three
things a material can layer on top:

- **Lambert diffuse** — the entire model from Slice 9 (when lighting moved off
  the vertex colors and into the shader) through Slice 13, and still the entire
  model for any material that asks for nothing else.
- **A Blinn-Phong specular highlight** (`specular`, `shininess`), and **a Schlick
  Fresnel edge** (`fresnel`, `fresnel_tint`). These two are *view-dependent*,
  which is the reason `CameraUniform` carries the eye: a projection matrix places
  a fragment but cannot tell it where the viewer is without being inverted. They
  exist because a surface reads as **wet** almost entirely through them — under
  diffuse alone, a rippling water surface and a flat one are nearly the same
  picture, because diffuse shading does not care where you stand.
- **An animated ripple field** (`ripple_strength`, `ripple_scale`) that perturbs
  the normal per fragment: six octaves of directional waves, each rotated off the
  last, at a non-integer frequency ratio, with longer waves travelling faster.
  All three of those are load-bearing against banding, which is what a naive sum
  of sines gives you.

Two properties of that list matter more than the terms themselves:

- **Everything defaults to zero, so the model is additive in the ledger sense.**
  A material that sets none of it produces the identical picture it did before
  any of this existed — which is what makes it safe to grow the lighting model
  for one demo without re-verifying every other one.
- **Ripples are shading, not geometry, and that distinction is the point.** The
  same waves lived in terrain's *mesh* first and cost 10 ms a frame in
  `sin_cos` over ~50,000 vertices — and they *banded*, because a normal per
  vertex interpolated across triangles far larger than a ripple draws stripes.
  Moving them into the fragment shader made them free and made their detail
  per-pixel rather than per-tessellation. This is why the frame clock is in the
  uniform at all: without a clock in the shader, a consumer whose surface detail
  animates is forced to rebuild and re-upload a mesh every frame to say so.

The honest ceiling: with no offscreen render target there is no reflection and no
refraction, so the Fresnel term tends toward a **flat colour** rather than toward
an image of the scene. It is a stand-in, labelled as one in its own docs, and
closing that gap is the render-graph entry under *Natural next steps*.

## The overlay pass and the UI

The overlay is the engine's first **second render pass** and the seam where a
render graph will eventually grow. The design holds two boundaries at once:

- **A second pass, composited.** `Renderer::render()` records the 3D pass, then
  `Overlay::flush()` records a second pass that *loads* (rather than clears) the
  color target, runs a 2D pipeline (orthographic pixel→NDC mapping, depth off,
  alpha blending), and draws this frame's accumulated 2D geometry. It no-ops if
  the consumer drew no UI, so 3D-only demos are unaffected. The overlay's
  pixel→NDC uniform tracks the surface size, so it resyncs through the same
  `resize()` path as the depth buffer and the web async-renderer resync.

- **Text without a font dependency, and no longer the engine's font.** Glyphs come
  from a **signed distance field** atlas that `slmsttaa-ui` owns
  (`slmsttaa_ui::font::ATLAS`, `include_bytes!`d): the overlay uploads it as
  `R8Unorm` and samples it with **linear** filtering, because a distance field has
  to be interpolated to scale. `0.5` is the glyph outline; the antialiasing width
  is computed on the CPU per run (`font::aa_band`) and passed per-vertex, since
  `overlay.wgsl` uses no derivatives and `fwidth` is therefore unavailable. Solid
  rectangles no longer sample the atlas at all — the old fully-white cell is gone,
  and the shader gives them coverage by mode.

  There is still no font file or rasterizer at runtime. The rasterizing happens
  offline in the `fontbake` workspace member (Inter, OFL-1.1, committed under
  `fontbake/assets/`), which is the one crate here allowed a font dependency; its
  output is committed and reviewed.

  **Why the font lives above the seam.** Through UI Slice 4 the engine owned the
  font and `text_size` was on the `Painter` trait — implemented once by the overlay
  and once by the test recorder, agreeing only because the bitmap font was a
  monospace grid. Proportional advances break that: the two implementations
  diverge, and then the UI tests measure a different font than the screen draws,
  so they stay green while readouts drift out of the panel. Moving the metrics into
  the crate that does the layout makes that unrepresentable. It costs a narrower
  seam — a `Painter` no longer chooses its font — which is recorded in
  `slmsttaa_ui::font` as the deliberate trade it is.

- **`Painter` is the seam, and it is the engine's only obligation to the UI.**
  The toolkit talks to the overlay *only* through that trait (`fill_rect` /
  `stroke_rect` / `text` / `set_layer` / `push_clip` / `pop_clip`)
  and never sees `wgpu`; the overlay is just one implementation, and a headless
  recorder in the UI crate's tests is another. Everything above the trait —
  widgets, layout, theming, interaction — is
  [`slmsttaa-ui`](slmsttaa-ui/README.md)'s business and is documented there.

  Input crosses the same boundary, in the opposite direction and by copy. The
  toolkit cannot `use crate::input::Input` — it doesn't depend on the engine, and
  reaching back would be a dependency cycle — so it declares its own `UiInput`
  snapshot and `Renderer::ui()` fills one in each frame from this frame's `Input`,
  the surface size (so a panel can anchor to a window edge without the toolkit
  ever learning what a window is), and `Renderer::dt` (so its hover fades and
  collapse transitions run on our clock without it owning one). Five field
  assignments buys a UI crate with no dependencies at all.

  What lives on *this* side of the seam is the part that touches the GPU: the
  overlay pipeline, the glyph atlas, the 2D vertex format, and draw ordering. So
  when the UI needs a capability the painter lacks, the work lands here, in
  `overlay.rs` / `overlay.wgsl` / `Vertex2D`, as a deliberate widening of the
  trait. That funnel is the point of the crate split; UI Slice 1 was the first
  time it was used in anger (ordered draw layers), and UI Slice 2 the second
  (rounded corners, borders, and clipping).

  UI Slice 3 is the useful counter-example: a rewrite of the whole layout system
  — regions, rows, columns, edge-anchored panels, right-aligned readouts — that
  widened the `Painter` trait by **nothing at all**. Everything it needed was
  already there, text measurement included. The only engine-side change was one
  more field copied into `UiInput` (the viewport, for anchoring). A seam that
  absorbs a change that size without moving is a seam in roughly the right
  place. UI Slice 6 (animation) then cost one more field on the same terms —
  `dt` — and again nothing on the `Painter` trait: a fading color is still a
  color, and a collapsing section is still a clip rect.

- **Rounded corners and clipping are per-vertex parameters, not extra passes.**
  `Vertex2D` carries the rect it belongs to (center + half-size), a corner
  radius, a border width, and a clip rectangle — 80 bytes a vertex. The fragment
  shader evaluates a rounded-box SDF for the shape, subtracts an inset SDF for a
  stroke, and discards outside the clip rect. Because all of it is per-vertex
  rather than per-draw, a panel with rounded corners, a hairline border, and a
  clipped scroll region inside it is still **one** `draw_indexed`. Clip
  rectangles intersect rather than replace as they nest, so an inner region can
  only ever shrink what is visible.

- **Draw layers cost one index vector, not one draw call.** `Painter::set_layer`
  directs primitives into one of four buckets (base / panel / popup / tooltip).
  Only *index* order decides what covers what, so every layer indexes the same
  vertex vector and `Overlay::flush` simply concatenates the buckets back-to-front
  before uploading — the whole overlay is still a single `draw_indexed`.

  This is what lets the UI declare a panel background *last*, when its height is
  finally known, and still have it painted *behind* the widgets above it.

- **The UI is laid out in logical points, not physical pixels.** Nothing read
  `scale_factor` before UI Slice 1, so on a 2× display the panel drew at half
  size. The conversion happens at both ends of the seam and nowhere else:
  `Renderer::ui()` divides the cursor by the scale factor on the way in, and
  `Overlay` multiplies coordinates by it on the way to vertices. The toolkit
  never learns the scale factor at all, which is what keeps its layout math (and
  its tests) resolution-independent. Text runs are snapped to whole *physical*
  pixels, since rounding in points still lands mid-pixel at 1.5×.

## Why the async/user-event dance

GPU initialization (`request_adapter` / `request_device`) is async. On native we
can just `pollster::block_on` it. On the web you **cannot block the main thread**,
so the renderer is built in a spawned future and sent back into the running event
loop as a user event. Parameterizing the loop over `Renderer` lets the exact same
control flow serve both targets — only the "how do we wait" differs, isolated in
`App::init_renderer`.

## Cross-platform gotchas (learned the hard way)

These are subtle and easy to reintroduce, so they're documented here:

- **Web event loop uses `spawn_app`, not `run_app`.** On wasm, winit unwinds the
  stack by *throwing* a sentinel exception (`"Using exceptions for control
  flow"`) when it hands the loop to the browser's animation frames. Calling
  `run_app` there surfaces as a rejected `init()`. `web/index.html` explicitly
  ignores that one exception.

- **Canvas backing size is not derived from CSS.** winit creates the web surface
  at 1x1 and `.with_inner_size()` is ignored on the web. We must read the
  viewport (`window.inner_width/height`), call `request_inner_size` with a
  `LogicalSize` (winit scales by device-pixel-ratio), and **resync the surface
  size when the async renderer arrives** — the `Resized` event usually fires
  before GPU init finishes, so it'd otherwise be missed. Symptom if wrong: a
  single stretched pixel (a flat color filling the page).

- **Backend selection differs.** `Backends::PRIMARY` excludes GL, so on the web
  it's WebGPU-only. We request `BROWSER_WEBGPU | GL` on wasm so a WebGL2 fallback
  is actually available, and use `downlevel_webgl2_defaults` limits there so a GL
  adapter can satisfy the device request.

- **The depth buffer must track the surface size.** Depth and color attachments
  have to share dimensions, so the depth texture is recreated in `resize()`
  alongside the surface reconfigure — and because the web's async-renderer resync
  funnels through `resize()` too, that path is covered without a special case.
  Forgetting it surfaces as a render-pass validation error after the first resize.
  The depth format is `Depth32Float` (a render-attachment format on every backend,
  including the WebGL2 fallback); both the texture and the pipeline read one
  `DEPTH_FORMAT` constant, so swapping to `Depth24Plus` is a one-line change.

- **WebGL2 has no non-zero `first_instance`, and no storage buffers.** Both
  constrain how per-object data reaches the shader, and both bite in the same
  place. Per-instance data rides an **instance-step vertex buffer**
  (`VertexStepMode::Instance`: the model matrix as four `vec4` columns at
  locations 3–6, the normal matrix as three `vec3` columns at 7–9, the material
  tint at 10, the packed shading terms at 11, and the Fresnel tint at 12) rather
  than the storage buffer most tutorials reach for —
  `downlevel_webgl2_defaults` does not have those at all. With `Vertex` taking
  0–2, that is **thirteen of the sixteen vertex attributes WebGL2 guarantees**.
  Three slots left is one `mat4` short of anything structural, so the next thing
  wanting per-instance data should expect to **pack into the spare `w` channels**
  of the vectors already there — which is exactly how the ripple parameters got
  aboard for free — rather than claim a slot. And when several meshes
  share one instance buffer, each mesh's run is selected by **offsetting the
  buffer binding** (`set_vertex_buffer(1, buf.slice(byte_offset..))`, then always
  drawing `0..count`), *not* by passing a non-zero start to `draw_indexed`'s
  instance range. The obvious version works on native and fails in the browser
  fallback, which is the worst kind of bug this file exists to prevent.

- **A uniform's `visibility` must name every stage that reads it.** The camera
  bind group was `ShaderStages::VERTEX` for as long as only `vs_main` projected
  with it; the moment the fragment shader needed `eye`, it had to become
  `VERTEX_FRAGMENT`. Worth knowing mainly because of *how* it fails: wgpu rejects
  the pipeline at creation with a validation error naming the binding, rather
  than handing the shader zeroes and drawing a subtly wrong picture. That is the
  good kind of failure, and it is the reason this one is a footnote rather than a
  war story.

- **A normal is not transformed by the model matrix.** Using the model matrix's
  upper 3×3 is the tempting shortcut and is wrong under non-uniform scale: it
  stretches normals along with the geometry, so a squashed box's flat top shades
  as though tilted. `InstanceRaw::normal_matrix` ships the 3×3 **inverse-transpose**
  instead. `examples/scene.rs` scales one capsule to four different limb lengths,
  so the shortcut is visibly wrong in the very first frame — this is not a
  subtlety that hides until later. A degenerate transform (a zero scale
  component) has no invertible block and falls back to the plain matrix, so the
  buffer never fills with `NaN`.

- **Transparency is ordering, not just blending.** A material with alpha below
  `1.0` moves its instance into a blended pipeline with **depth writes off**, and
  `set_instances` sorts every opaque run ahead of every transparent one. Both
  halves are required: blending composites correctly only over a finished target,
  and a see-through surface that wrote depth would hide what is behind it — the
  one thing it must not do. Transparent instances are *not* depth-sorted against
  each other; terrain's water is a single non-overlapping surface, and a general
  back-to-front sort waits for a demo that needs one.

  **Per-vertex alpha is invisible to that choice**, and this is the trap. The
  pipeline is picked per *instance*, from the material, before anything looks at
  a vertex — so a mesh whose corners fade out under a fully opaque tint gets the
  opaque pass and its fade is silently ignored. `Material::blended()` forces the
  blended pipeline regardless of tint alpha, and exists precisely because
  dragging terrain's opacity slider to `1.0` would otherwise snap its soft
  shoreline back to a hard line.

- **Match the WebGPU spec; keep wgpu current.** Browsers track the live WebGPU
  spec and reject limits/fields they no longer recognize (e.g. a stale
  `maxInterStageShaderComponents` caused `requestDevice` to fail). Prefer a
  recent `wgpu`; we're on 29.

## Performance posture

- One command encoder per frame, now with two render passes (3D scene + overlay).
- Camera data updated with `Queue::write_buffer` — no per-frame buffer
  allocation. The overlay's 2D buffers are written once per frame and only
  reallocated (to the next power of two) when the UI geometry outgrows them.
- **Moving an object costs a matrix, not a mesh.** `set_instances` writes one
  148-byte `InstanceRaw` per object (the 64-byte model matrix, the normal matrix,
  and the material) into a single instance buffer (grown to the next power of
  two, same rule as the overlay) and touches no vertex or index buffer.
  Geometry is uploaded by `upload_mesh` and only ever re-uploaded by
  `update_mesh`, for a consumer whose geometry genuinely changes shape.
- **Animated surface detail costs neither.** A shader clock (`CameraUniform`'s
  `frame`) plus a per-fragment normal perturbation is how a surface moves without
  re-uploading anything. Terrain's waves lived in its mesh first, at four
  `sin_cos` per vertex over ~50,000 vertices every frame; moving them into the
  fragment shader took the water mesh build from **10.2 ms to 2.3 ms** and the
  demo from 66 fps back to 75–80, while making the detail finer. The general
  rule, learned twice: if what changes per frame is how a surface *looks* rather
  than where its vertices *are*, it does not belong in the mesh.
- **One draw call per mesh, not per object.** The draw-list is sorted by handle so
  every instance of a mesh is contiguous: `examples/scene.rs` draws nine
  ten-part figures — ninety placements — in four calls, one per primitive.
- `PowerPreference::HighPerformance` + `MemoryHints::Performance`.
- `AutoVsync` by default; flip to `AutoNoVsync` in `renderer/mod.rs` to measure
  uncapped frame rates.
- Release profile: thin LTO + a single codegen unit; wasm built size-optimized.

## Natural next steps

The scaffold leaves obvious seams:

- **MSAA** (`multisample` is currently the 1-sample default).
- A small **render-graph**, and this is no longer speculative — it is the one
  entry here with a demo blocked on it. There are two passes (3D + overlay) wired
  by hand in `render()`; what water needs next is an **offscreen render target**:
  draw the opaque pass to a texture, then let the blended pass sample it for
  refraction and a screen-space reflection. That is what turns the Fresnel term
  from a flat stand-in colour into an image of the scene, and no amount of
  tuning the current shader substitutes for it. A third pass with a real
  dependency between passes is what makes the hand-wiring stop scaling.
- **Further overlay capabilities the UI crate may ask for**: soft shadows (looked
  at in UI Slice 2 and deliberately declined — a hairline border reads as
  well for a fraction of the fill cost), and textured quads for icons, which
  wait on consumer-supplied textures. Sequenced in the
  [UI roadmap](slmsttaa-ui/ROADMAP.md), since that's what demands them; widgets
  themselves are no longer an engine concern.

Already in place (earlier seams now filled): an indexed `Mesh` + draw-list
(Slice 1), a **depth buffer + back-face culling** (Slice 2), a **consumer-driven
camera** fed by a winit-free `Input` (Slice 3), the **terrain vertical** (Slice 4,
rebuilt in Slice 6 as a layered Perlin + hydro-thermal pipeline), a **screen-space
overlay pass + decoupled immediate-mode UI** with a wasm-safe frame clock
(Slice 5), a **portable wireframe render mode** (`RenderMode`, line topology —
Slice 6), and **per-object transforms + an instance draw-list** (Slice 8 — meshes
are uploaded once for a handle and placed by `Transform`, so nothing re-uploads
geometry to move it), **in-shader lighting + a per-instance `Material`** with the
inverse-transpose normal matrix and a blended pass for transparency (Slices
9–10), **primitive mesh builders** (Slice 11), a **fixed-timestep clock + time
control** (Slice 12 — `Timeline`, `Application::fixed_update`, and
`Renderer::time_mut` for pause, scale, single-step and seek), and a
**view-dependent shading model** — the eye and a wall clock in `CameraUniform`,
Blinn-Phong specular, a Schlick Fresnel edge, RGBA vertex colors, and per-fragment
animated ripples (Slices 14–15). On the overlay side specifically: **ordered draw layers** in
`Overlay::flush` and a **`scale_factor`-aware surface** (UI Slice 1 — the toolkit
speaks logical points and the overlay scales on the way to vertices),
**rounded-rect, border, and clip support** in `overlay.wgsl` (UI Slice 2), and a
**distance-field text mode** with a linear atlas sampler and a CPU-computed
antialiasing band (UI Slice 5 — which also deleted `font.rs`, moving the font
above the seam).
