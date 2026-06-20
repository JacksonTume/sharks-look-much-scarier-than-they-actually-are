# SLMSTTAA

**Sharks Look Much Scarier Than They Actually Are** — a small, performant 3D
rendering engine in Rust, built on [WebGPU](https://www.w3.org/TR/webgpu/) via
[`wgpu`](https://wgpu.rs/). It runs **standalone** on the desktop
(Vulkan / Metal / DX12) and **in the browser** (WebGPU, with a WebGL2 fallback)
from a single codebase.

The name is a reminder: graphics programming looks terrifying from the outside,
but up close it's mostly friendly little triangles.

## Status

Early scaffold, but live on both targets: it opens a window and renders a
camera-transformed, vertex-colored triangle natively **and** in the browser. The
structure is laid out so the scary parts (meshes, materials, a render graph) have
obvious homes to grow into.

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for how the pieces fit together and the
cross-platform gotchas that shaped them.

## Requirements

- Rust 1.80+ (a `rust-toolchain.toml` pins `stable` with the `wasm32-unknown-unknown`
  target, `clippy`, and `rustfmt`).
- For the browser build: [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and
  a WebGPU-capable browser (recent Chrome/Edge); a WebGL2 fallback covers others.
- GPU stack: [`wgpu`](https://wgpu.rs/) 29 (WebGPU), [`winit`](https://github.com/rust-windowing/winit) 0.30.

## Layout

| Path                     | Purpose                                                  |
| ------------------------ | -------------------------------------------------------- |
| `src/lib.rs`             | Crate root, logging setup, `run()` / wasm entry point.   |
| `src/app.rs`             | winit `ApplicationHandler`: window + event loop.         |
| `src/renderer/mod.rs`    | wgpu device/surface/pipeline; per-frame render.          |
| `src/renderer/vertex.rs` | Vertex format + buffer layout.                           |
| `src/renderer/shader.wgsl` | Demo WGSL vertex/fragment shaders.                     |
| `src/camera.rs`          | Perspective camera + GPU uniform.                        |
| `web/index.html`         | Browser harness for the wasm build.                      |

## Run it (standalone)

```sh
cargo run --bin slmsttaa-demo            # debug
cargo run --bin slmsttaa-demo --release  # optimized
```

(`cargo run` works too — there's only one binary.) Press <kbd>Esc</kbd> or close
the window to quit. Set `RUST_LOG=slmsttaa=debug` for more output.

## Run it (browser / WebGPU)

Build the wasm package and serve the `web/` directory:

```sh
# one-time
cargo install wasm-pack

# build → emits web/pkg/
wasm-pack build --target web --out-dir web/pkg

# serve (any static server works)
python -m http.server -d web 8080
```

Then open <http://localhost:8080>. A WebGPU-capable browser (recent
Chrome/Edge) is recommended; the build also includes a WebGL2 fallback.

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
