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
│                     payload, a single mat4x4 view-projection). look_from_to
│                     lets a consumer aim it with plain [f32; 3] arrays.
├── input.rs          Input: per-frame keyboard/mouse state, decoupled from
│                     winit. Exposes engine Key/MouseButton enums (never winit's);
│                     the event loop feeds it, the consumer reads it via
│                     Renderer::input(). Also absolute cursor position + mouse
│                     press-edges, for screen-space UI hit-testing.
├── time.rs           Clock: a cross-platform frame clock (delta-time). Native
│                     Instant; web performance.now() (Instant panics on wasm).
│                     Surfaced as Renderer::dt().
├── ui.rs             A modular, decoupled immediate-mode UI framework. The
│                     Painter trait decouples it from the renderer (it never sees
│                     wgpu); widgets edit a consumer's own &mut values, decoupling
│                     it from the consumer. Renderer::ui() hands one out per frame.
└── renderer/
    ├── mod.rs        Renderer: wgpu instance/adapter/device/queue/surface, the
    │                 solid + wireframe render pipelines (RenderMode), the depth
    │                 buffer, the consumer's mesh draw-list, the overlay, the UI
    │                 state + clock, and per-frame begin_frame()/update()/render().
    ├── mesh.rs       Mesh (vertices + indices): the CPU-side geometry a consumer
    │                 builds and hands over via Renderer::set_meshes.
    ├── vertex.rs     Vertex (position + color) and its buffer layout.
    ├── shader.wgsl   3D vertex/fragment shaders (WGSL).
    ├── overlay.rs    Overlay: the screen-space 2D pass (UI/HUD). Owns its own
    │                 pipeline, a glyph atlas, and dynamic 2D buffers; implements
    │                 ui::Painter. Drawn after the 3D pass (see Frame lifecycle).
    ├── overlay.wgsl  2D vertex/fragment shaders for the overlay.
    └── font.rs       Embedded public-domain 8x8 bitmap font (font8x8), baked into
                      the overlay's glyph atlas — no font file/rasterizer at runtime.

examples/
├── triangle.rs       Reference consumer: implements Application and uploads one
│                     triangle. Native fn main + a #[wasm_bindgen(start)] hook.
├── cube.rs           Spinning solid cube: proves indexed meshes, depth testing,
│                     and back-face culling. Rotates its corners on the CPU and
│                     re-uploads the mesh each frame.
├── gallery.rs        Multi-scene switcher. Owns several scenes and swaps the
│                     draw-list between them; on the web it builds DOM buttons
│                     (web-sys) that drive the selection, on native it auto-cycles.
├── grid.rs           Orbitable height-mapped terrain grid: proves the input +
│                     camera seam (Slice 3). Keeps its own orbit state and aims
│                     the camera from Renderer::input() each frame.
└── terrain.rs        The capstone and default web build: layered, iterative
    terrain/          terrain. A Perlin-noise base heightmap (heightmap.rs) is
    ├── heightmap.rs  carved by a stream-power landscape-evolution model
    └── erosion.rs    (erosion.rs: priority-flood flow routing + drainage-area
                      incision + thermal relaxation) into dendritic valley
                      networks, with a live engine-drawn UI panel to tune every
                      layer and a wireframe toggle. Both layers live entirely in
                      the demo (pulled in via #[path]); the engine only uploads the
                      mesh, picks solid/wireframe, draws the UI, runs the camera.

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
5. Each `RedrawRequested`: `Renderer::begin_frame()` ticks the frame clock (so
   `Renderer::dt()` is fresh) and clears last frame's overlay geometry. Then
   `Application::update` advances the consumer's state (reading `Renderer::input()`,
   driving the camera via `Renderer::camera_mut`, and building its UI via
   `Renderer::ui()`). Then `Renderer::update()` re-uploads the camera uniform, and
   `Renderer::render()` records **two** passes into one command encoder: the 3D
   pass (clear color + depth → `draw_indexed` each mesh in the draw-list, using the
   solid or wireframe pipeline per the current `RenderMode`) and then the
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

- **Text without a font dependency.** Glyphs come from an embedded 8x8 bitmap font
  (`font.rs`, public-domain `font8x8`) baked once into an `R8Unorm` coverage atlas
  at startup. One cell is overwritten fully white so solid rectangles and text
  share a single pipeline (rects point at the white texel). Nearest filtering
  keeps it crisp; there is no font file or rasterizer at runtime, which keeps it
  identical on native and web (KISS).

- **The UI is decoupled from both sides.** `ui.rs` is an immediate-mode toolkit
  that talks to the overlay *only* through the `Painter` trait (`rect`/`text`/
  `text_size`) and reads `Input`. It never sees `wgpu`; the overlay is just one
  `Painter` implementation. Upward, widgets borrow the consumer's own `&mut f32`/
  `&mut bool`, so parameters live in the demo — the UI knows nothing about what it
  controls. The only retained state is a tiny `UiState` (which slider is being
  dragged, last frame's panel height for the background). This mirrors the
  engine/consumer inversion of control one layer down, and is deliberately *not* a
  retained-mode widget tree (roadmap principle 2).

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

- **Match the WebGPU spec; keep wgpu current.** Browsers track the live WebGPU
  spec and reject limits/fields they no longer recognize (e.g. a stale
  `maxInterStageShaderComponents` caused `requestDevice` to fail). Prefer a
  recent `wgpu`; we're on 29.

## Performance posture

- One command encoder per frame, now with two render passes (3D scene + overlay).
- Camera data updated with `Queue::write_buffer` — no per-frame buffer
  allocation. The overlay's 2D buffers are written once per frame and only
  reallocated (to the next power of two) when the UI geometry outgrows them.
- `PowerPreference::HighPerformance` + `MemoryHints::Performance`.
- `AutoVsync` by default; flip to `AutoNoVsync` in `renderer/mod.rs` to measure
  uncapped frame rates.
- Release profile: thin LTO + a single codegen unit; wasm built size-optimized.

## Natural next steps

The scaffold leaves obvious seams:

- **MSAA** (`multisample` is currently the 1-sample default).
- A **material** abstraction beyond the single shared pipeline, **per-mesh
  transforms** (meshes are uploaded in world space today; the demo rotates on the
  CPU and re-uploads), and **vertex normals + a lighting model** (the terrain demo
  bakes diffuse shading into vertex color CPU-side, which is fine for one consumer
  but a second lit demo would justify pushing normals into the pipeline).
- A small **render-graph**: there are now two passes (3D + overlay) wired by hand
  in `render()`. A second consumer wanting its own pass is the roadblock that turns
  this into a real graph.
- Interactive UI **beyond the current widgets** (text input, scroll regions,
  draggable panels) — pulled in only when a demo asks.

Already in place (earlier seams now filled): an indexed `Mesh` + draw-list
(Slice 1), a **depth buffer + back-face culling** (Slice 2), a **consumer-driven
camera** fed by a winit-free `Input` (Slice 3), the **terrain vertical** (Slice 4,
rebuilt in Slice 6 as a layered Perlin + hydro-thermal pipeline), a **screen-space
overlay pass + decoupled immediate-mode UI** with an embedded bitmap font and a
wasm-safe frame clock (Slice 5), and a **portable wireframe render mode**
(`RenderMode`, line topology — Slice 6).
