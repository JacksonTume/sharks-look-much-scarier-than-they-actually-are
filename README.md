# SLMSTTAA

**Sharks Look Much Scarier Than They Actually Are** — a small, performant 3D
rendering engine in Rust, built on [WebGPU](https://www.w3.org/TR/webgpu/) via
[`wgpu`](https://wgpu.rs/). It runs **standalone** on the desktop
(Vulkan / Metal / DX12) and **in the browser** (WebGPU, with a WebGL2 fallback)
from a single codebase.

The name is a reminder: graphics programming looks terrifying from the outside,
but up close it's mostly friendly little triangles.

## Status

Live on both targets, and past the point of proving itself. A consumer implements
the `Application` trait and calls `run(app)`; the engine owns the window, GPU, and
event loop and calls into it. It draws indexed meshes with a depth buffer and
back-face culling, in solid or **wireframe** mode; the camera is driven by the
consumer through a `winit`-free input layer; and a second, screen-space pass
provides text and an immediate-mode **UI** (sliders, buttons, checkboxes) that
renders identically on native and web.

The capstone is `examples/terrain`: a fractal Perlin heightmap carved by live
hydro-thermal **stream-power erosion**, every layer tunable from an on-screen
panel. The whole algorithm lives in the demo — `wgpu` and `winit` appear nowhere
in it, which is the point.

The UI lives in its own zero-dependency crate,
[`slmsttaa-ui`](slmsttaa-ui/README.md), so a widget toolkit doesn't grow inside a
rendering engine. The engine keeps the screen-space pass and the glyph atlas; the
toolkit keeps everything above the `Painter` trait — and since it depends on
nothing, "the UI never touches `wgpu`" is enforced by the crate graph rather than
by good intentions. It is re-exported as `slmsttaa::ui`, so consumers still see
one dependency, and it plans its own work in
[`slmsttaa-ui/ROADMAP.md`](slmsttaa-ui/ROADMAP.md).

Still to grow into: materials, multiple meshes with transforms, a real lighting
model, and a render graph — each waiting for a demo that actually demands it.

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for how the pieces fit together and the
cross-platform gotchas that shaped them, and [`ROADMAP.md`](ROADMAP.md) for where
it's going and why.

## Requirements

- Rust 1.80+ (a `rust-toolchain.toml` pins `stable` with the `wasm32-unknown-unknown`
  target, `clippy`, and `rustfmt`).
- For the browser build:
  [`wasm-bindgen-cli`](https://crates.io/crates/wasm-bindgen-cli) (at a version
  matching the `wasm-bindgen` dependency) and a WebGPU-capable browser (recent
  Chrome/Edge); a WebGL2 fallback covers others.
- GPU stack: [`wgpu`](https://wgpu.rs/) 29 (WebGPU), [`winit`](https://github.com/rust-windowing/winit) 0.30.

## Layout

| Path                     | Purpose                                                  |
| ------------------------ | -------------------------------------------------------- |
| `src/lib.rs`             | Crate root, logging setup, the `run(app)` entry point.   |
| `src/application.rs`     | The `Application` trait a consumer implements (IoC seam).|
| `src/app.rs`             | winit `ApplicationHandler`: window + event loop.         |
| `src/renderer/mod.rs`    | wgpu device/surface/pipeline; per-frame render.          |
| `src/renderer/mesh.rs`   | `Mesh` (vertices + indices) the consumer hands over.     |
| `src/renderer/vertex.rs` | Vertex format + buffer layout.                           |
| `src/renderer/shader.wgsl` | WGSL vertex/fragment shaders.                          |
| `src/camera.rs`          | Perspective camera + GPU uniform.                        |
| `src/renderer/overlay.rs` | Screen-space 2D pass + glyph atlas (`Painter` impl).   |
| `src/time.rs`            | Cross-platform frame clock (`Renderer::dt`).             |
| `examples/triangle.rs`   | Reference consumer: draws one triangle (native + web).   |
| `examples/cube.rs`       | Spinning solid cube: indexed mesh + depth + culling.     |
| `examples/gallery.rs`    | Scene switcher: web buttons swap demos; native cycles.   |
| `examples/grid.rs`       | Orbitable terrain grid: the input + camera seam.         |
| `examples/terrain.rs`    | **Capstone**: Perlin + stream-power erosion, live panel.  |
| `slmsttaa-ui/src/`       | The UI toolkit crate (zero deps): `Painter`, widgets.    |
| `slmsttaa-ui/tests/`     | Layout + hit-testing tests (headless recording painter). |
| `web/index.html`         | Browser harness for the wasm build (loads `pkg/app.js`). |
| `xtask/`                 | `cargo xtask serve`: build native + web and host it.     |

## Run it (standalone)

```sh
cargo run --example terrain             # the capstone: layered Perlin + stream-power erosion
cargo run --example triangle            # the smallest consumer
cargo run --example cube                # spinning solid cube (depth + culling)
cargo run --example gallery             # switch between scenes (auto-cycles on native)
```

In the `terrain` demo, drag the left mouse button over the 3D view to orbit and
scroll to zoom; the panel on the left edits the Perlin base shape and the
stream-power erosion live (release a slider to rebuild), with a **wireframe**
toggle to inspect the grid. Press
<kbd>Esc</kbd> or close the window to quit. Set `RUST_LOG=slmsttaa=debug` for more
output.

## Run it (browser / WebGPU)

One command builds the example for the web and serves it — no Python, no manual
`wasm-bindgen` step:

```sh
# one-time — install the wasm-bindgen CLI (matched to the wasm-bindgen dependency)
cargo install wasm-bindgen-cli

cargo xtask serve              # builds + serves `terrain` at http://localhost:8080
cargo xtask serve cube        # a different example
cargo xtask serve --release   # optimized build
cargo xtask serve --port 9000 # a different port
```

Then open <http://localhost:8080> and **hard-refresh** if you rebuilt. A
WebGPU-capable browser (recent Chrome/Edge) is recommended; the build also
includes a WebGL2 fallback.

Under the hood `cargo xtask serve` builds the example natively *and* for wasm,
runs `wasm-bindgen` into `web/pkg/` (as `app.js`, so `web/index.html` is stable
across examples), and hosts `web/` from a tiny built-in static server. See
`xtask/src/main.rs`.

## Write your own

Implement `Application` and hand it to `run`:

```rust
use slmsttaa::{run, Application, Mesh, Renderer, Vertex};

struct MyApp;
impl Application for MyApp {
    fn init(&mut self, renderer: &mut Renderer) {
        let mesh = Mesh::new(vec![/* your vertices */], vec![/* your indices */]);
        renderer.set_meshes(&[mesh]);
    }
    // `update(&mut self, renderer)` runs every frame (optional).
}

fn main() {
    run(MyApp).unwrap();
}
```

## Performance notes

- Single command encoder + render pass per frame; camera data uploaded via
  `Queue::write_buffer` (no per-frame buffer churn).
- `PowerPreference::HighPerformance` adapter selection and
  `MemoryHints::Performance`.
- `AutoVsync` by default — switch to `AutoNoVsync` in `renderer/mod.rs` to
  benchmark uncapped frame rates.
- Release profile uses thin LTO + a single codegen unit; wasm packages are size-
  optimized.

## License

Dual-licensed under MIT or Apache-2.0, at your option.
