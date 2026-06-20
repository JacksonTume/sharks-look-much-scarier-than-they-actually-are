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
smallest reference consumer; `examples/gallery.rs` is the source of the web build.

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
│                     payload, a single mat4x4 view-projection).
└── renderer/
    ├── mod.rs        Renderer: wgpu instance/adapter/device/queue/surface,
    │                 the render pipeline, the depth buffer, the consumer's mesh
    │                 draw-list, and per-frame update()/render().
    ├── mesh.rs       Mesh (vertices + indices): the CPU-side geometry a consumer
    │                 builds and hands over via Renderer::set_meshes.
    ├── vertex.rs     Vertex (position + color) and its buffer layout.
    └── shader.wgsl   Vertex/fragment shaders (WGSL).

examples/
├── triangle.rs       Reference consumer: implements Application and uploads one
│                     triangle. Native fn main + a #[wasm_bindgen(start)] hook.
├── cube.rs           Spinning solid cube: proves indexed meshes, depth testing,
│                     and back-face culling. Rotates its corners on the CPU and
│                     re-uploads the mesh each frame.
└── gallery.rs        Multi-scene switcher and the default web build. Owns several
                      scenes and swaps the draw-list between them; on the web it
                      builds DOM buttons (web-sys) that drive the selection, on
                      native it auto-cycles. Source of the web demo.

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
4. Each `RedrawRequested`: `Application::update` advances the consumer's state,
   then `Renderer::update()` re-uploads the camera uniform, then
   `Renderer::render()` records one command encoder + render pass (clear color +
   depth → draw each mesh in the consumer's draw-list with `draw_indexed`, if any)
   and presents. Depth testing and back-face culling are on, so overlapping 3D
   geometry occludes correctly.
5. `about_to_wait` requests another redraw, so we render continuously
   (`ControlFlow::Poll`).

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

- One command encoder + one render pass per frame.
- Camera data updated with `Queue::write_buffer` — no per-frame buffer
  allocation.
- `PowerPreference::HighPerformance` + `MemoryHints::Performance`.
- `AutoVsync` by default; flip to `AutoNoVsync` in `renderer/mod.rs` to measure
  uncapped frame rates.
- Release profile: thin LTO + a single codegen unit; wasm built size-optimized.

## Natural next steps

The scaffold leaves obvious seams:

- **MSAA** (`multisample` is currently the 1-sample default).
- A **material** abstraction beyond the single shared pipeline, and **per-mesh
  transforms** (meshes are uploaded in world space today; the demo rotates on the
  CPU and re-uploads).
- **Camera controls** (orbit / fly) driven from `WindowEvent` input — see
  `ROADMAP.md` Slice 3.
- A small **render-graph** once there's more than one pass.

Already in place (earlier seams now filled): an indexed `Mesh` + draw-list
(Slice 1) and a **depth buffer + back-face culling** (Slice 2).
