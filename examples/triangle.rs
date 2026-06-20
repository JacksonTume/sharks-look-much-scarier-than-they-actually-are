//! The smallest possible consumer of the engine: a single colored triangle.
//!
//! This lives in `examples/`, which Cargo compiles as a separate crate that can
//! only see `slmsttaa`'s public API. If the triangle can't be drawn from public
//! items alone, the engine boundary has leaked and this example fails to build —
//! that's the point.
//!
//! Run it:
//!   native — `cargo run --example triangle`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`).

use slmsttaa::{run, Application, Mesh, Renderer, Vertex};

/// A consumer that hands the engine one triangle and otherwise does nothing.
struct TriangleDemo;

impl Application for TriangleDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        // Counter-clockwise winding (so back-face culling keeps it), vertex-colored.
        let mesh = Mesh::new(
            vec![
                Vertex {
                    position: [0.0, 0.5, 0.0],
                    color: [1.0, 0.2, 0.3],
                },
                Vertex {
                    position: [-0.5, -0.5, 0.0],
                    color: [0.2, 1.0, 0.4],
                },
                Vertex {
                    position: [0.5, -0.5, 0.0],
                    color: [0.3, 0.4, 1.0],
                },
            ],
            vec![0, 1, 2],
        );
        renderer.set_meshes(&[mesh]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(TriangleDemo) {
        eprintln!("triangle example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Errors here can't be propagated to JS meaningfully; `run` logs to the
    // browser console on its own.
    let _ = run(TriangleDemo);
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
