# SLMSTTAA

**Sharks Look Much Scarier Than They Actually Are** — a small, performant 3D
rendering engine in Rust, built on [WebGPU](https://www.w3.org/TR/webgpu/) via
[`wgpu`](https://wgpu.rs/). It runs **standalone** on the desktop
(Vulkan / Metal / DX12) and **in the browser** (WebGPU, with a WebGL2 fallback)
from a single codebase.

The name is a reminder: graphics programming looks terrifying from the outside,
but up close it's mostly friendly little triangles.

## Status

Early scaffold, but live on both targets: a consumer implements the
`Application` trait and calls `run(app)`; the engine owns the window, GPU, and
event loop and calls into it. The `triangle` example renders a
camera-transformed, vertex-colored triangle natively **and** in the browser. The
structure is laid out so the scary parts (meshes, materials, a render graph) have
obvious homes to grow into.

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for how the pieces fit together and the
cross-platform gotchas that shaped them.

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
| `src/renderer/vertex.rs` | Vertex format + buffer layout.                           |
| `src/renderer/shader.wgsl` | WGSL vertex/fragment shaders.                          |
| `src/camera.rs`          | Perspective camera + GPU uniform.                        |
| `examples/triangle.rs`   | Reference consumer: draws one triangle (native + web).   |
| `web/index.html`         | Browser harness for the wasm build.                      |

## Run it (standalone)

```sh
cargo run --example triangle            # debug
cargo run --example triangle --release  # optimized
```

Press <kbd>Esc</kbd> or close the window to quit. Set `RUST_LOG=slmsttaa=debug`
for more output.

## Run it (browser / WebGPU)

The web build compiles the example to wasm, runs `wasm-bindgen` to emit
`web/pkg/`, and serves the `web/` directory:

```sh
# one-time — install the CLI at a version matching the wasm-bindgen dependency
cargo install wasm-bindgen-cli

# build the example for wasm, then generate JS/wasm bindings into web/pkg/
cargo build --example triangle --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/debug/examples/triangle.wasm \
  --out-dir web/pkg --target web

# serve (any static server works)
python -m http.server -d web 8080
```

Then open <http://localhost:8080>. A WebGPU-capable browser (recent
Chrome/Edge) is recommended; the build also includes a WebGL2 fallback.

## Write your own

Implement `Application` and hand it to `run`:

```rust
use slmsttaa::{run, Application, Renderer, Vertex};

struct MyApp;
impl Application for MyApp {
    fn init(&mut self, renderer: &mut Renderer) {
        renderer.set_vertices(&[ /* your vertices */ ]);
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
