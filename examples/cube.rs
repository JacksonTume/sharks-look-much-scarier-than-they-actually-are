//! A spinning, solid cube — the proof for indexed meshes + depth + culling.
//!
//! Like `triangle`, this lives in `examples/` and is compiled as a separate crate
//! that can only see `slmsttaa`'s public API. It exercises three things at once:
//!
//! - **Indexed drawing:** the cube is 8 shared corners + 36 indices (12 triangles),
//!   not 36 duplicated vertices.
//! - **Depth testing:** as it tumbles, near faces correctly occlude far ones.
//! - **Back-face culling:** every face is wound counter-clockwise *from outside*,
//!   so the inward-facing back triangles are dropped and the solid stays solid.
//!
//! It spins without any engine-side camera or transform support (neither exists
//! yet): each frame it rotates the 8 corners on the CPU and re-uploads the mesh —
//! the same "mutate geometry, hand it back" pattern the terrain demo will use.
//!
//! Run it:
//!   native — `cargo run --example cube`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `cube` for `triangle`.

use slmsttaa::{run, Application, Mesh, Renderer, Vertex};

/// The 8 corners of a unit cube centered on the origin, colored by position so
/// every face reads differently as the cube turns. Index order matters: the
/// triangle list in [`CUBE_INDICES`] refers to these by position.
const CUBE_CORNERS: [([f32; 3], [f32; 3]); 8] = [
    ([-0.5, -0.5, -0.5], [0.0, 0.0, 0.0]), // 0
    ([0.5, -0.5, -0.5], [1.0, 0.0, 0.0]),  // 1
    ([0.5, 0.5, -0.5], [1.0, 1.0, 0.0]),   // 2
    ([-0.5, 0.5, -0.5], [0.0, 1.0, 0.0]),  // 3
    ([-0.5, -0.5, 0.5], [0.0, 0.0, 1.0]),  // 4
    ([0.5, -0.5, 0.5], [1.0, 0.0, 1.0]),   // 5
    ([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),    // 6
    ([-0.5, 0.5, 0.5], [0.0, 1.0, 1.0]),   // 7
];

/// 12 triangles (two per face), each wound counter-clockwise when viewed from
/// outside the cube so back-face culling keeps the exterior and drops the inside.
#[rustfmt::skip]
const CUBE_INDICES: [u32; 36] = [
    4, 5, 6,  4, 6, 7, // front  (+z)
    0, 2, 1,  0, 3, 2, // back   (-z)
    1, 2, 6,  1, 6, 5, // right  (+x)
    0, 4, 7,  0, 7, 3, // left   (-x)
    3, 7, 6,  3, 6, 2, // top    (+y)
    0, 1, 5,  0, 5, 4, // bottom (-y)
];

/// Rotate a point by `yaw` around Y then `pitch` around X (radians).
fn rotate(p: [f32; 3], yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    // Yaw about Y.
    let x = p[0] * cy + p[2] * sy;
    let z = -p[0] * sy + p[2] * cy;
    let y = p[1];
    // Pitch about X.
    let y2 = y * cp - z * sp;
    let z2 = y * sp + z * cp;
    [x, y2, z2]
}

/// A consumer that tumbles a cube by re-uploading its rotated corners each frame.
#[derive(Default)]
struct CubeDemo {
    /// Accumulated rotation, advanced every frame.
    angle: f32,
}

impl CubeDemo {
    /// Build the cube mesh at the current rotation.
    fn mesh(&self) -> Mesh {
        let vertices = CUBE_CORNERS
            .iter()
            .map(|&(position, color)| Vertex {
                position: rotate(position, self.angle, self.angle * 0.6),
                color,
            })
            .collect();
        Mesh::new(vertices, CUBE_INDICES.to_vec())
    }
}

impl Application for CubeDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        renderer.set_meshes(&[self.mesh()]);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Fixed per-frame step: simplest cross-platform spin (no timer). The rate
        // is frame-rate dependent, which is fine for a demo.
        self.angle += 0.01;
        renderer.set_meshes(&[self.mesh()]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(CubeDemo::default()) {
        eprintln!("cube example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Errors here can't be propagated to JS meaningfully; `run` logs to the
    // browser console on its own.
    let _ = run(CubeDemo::default());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
