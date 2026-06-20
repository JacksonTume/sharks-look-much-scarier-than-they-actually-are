# Architecture

SLMSTTAA is a thin layer over [`wgpu`](https://wgpu.rs/) (the Rust WebGPU
implementation) and [`winit`](https://github.com/rust-windowing/winit) (cross-
platform windowing). The same source builds two ways:

- **Native** — desktop window via winit, GPU via Vulkan / Metal / DX12.
- **Web** — a `<canvas>` via winit's web backend, GPU via WebGPU (with a WebGL2
  fallback), shipped as a `wasm-bindgen` module.

The crate is both a library (`slmsttaa`) and a demo binary (`slmsttaa-demo`).

## Module map

```
src/
├── lib.rs            Crate root. Logging, the run() entry point, and the
│                     #[wasm_bindgen(start)] hook for the web.
├── main.rs           Native binary; just calls slmsttaa::run().
├── app.rs            App: winit ApplicationHandler. Owns the window and the
│                     Renderer; routes events; drives the redraw loop.
├── camera.rs         Camera (perspective look-at) + CameraUniform (the GPU
│                     payload, a single mat4x4 view-projection).
└── renderer/
    ├── mod.rs        Renderer: wgpu instance/adapter/device/queue/surface,
    │                 the render pipeline, and per-frame update()/render().
    ├── vertex.rs     Vertex (position + color) and its buffer layout.
    └── shader.wgsl   Demo vertex/fragment shaders (WGSL).
```

## Frame lifecycle

1. `run()` builds a `winit` event loop **parameterized over `Renderer`** as its
   user-event type, then hands control to the platform-appropriate runner.
2. On `resumed`, `App` creates the window. On the web it also mounts the canvas
   and sizes it (see gotchas).
3. The `Renderer` is built asynchronously (GPU init is async) and delivered back
   into the loop. Native blocks on it; the web spawns it and reports completion
   via an `EventLoopProxy<Renderer>` user event.
4. Each `RedrawRequested`: `Renderer::update()` re-uploads the camera uniform,
   then `Renderer::render()` records one command encoder + render pass (clear →
   draw) and presents.
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

- A **depth buffer** (`depth_stencil` is currently `None`) and back-face culling
  once real geometry exists.
- **MSAA** (`multisample` is currently the 1-sample default).
- A **mesh/material** abstraction beyond the single hard-coded vertex buffer.
- **Camera controls** (orbit / fly) driven from `WindowEvent` input.
- A small **render-graph** once there's more than one pass.
