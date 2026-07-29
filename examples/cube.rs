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
//! It also shows what a per-object transform is *for*. The cube used to spin by
//! rotating its 8 corners on the CPU and re-uploading the whole mesh every frame;
//! now the mesh is uploaded once in `init` and every frame hands over one
//! [`Transform`]. The geometry never moves — the object does.
//!
//! Run it:
//!   native — `cargo run --example cube`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `cube` for `triangle`.

use slmsttaa::{run, Application, Instance, Mesh, MeshHandle, Renderer, Transform, Vertex};

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

/// The six faces: four corner indices wound counter-clockwise seen from outside,
/// and the direction that face points.
///
/// **A lit cube cannot share its eight corners.** A corner belongs to three faces
/// pointing three different ways, and a vertex carries exactly one normal, so the
/// eight tidy corners above become 24 vertices — each face gets its own copy,
/// with the face's normal. That is the cost of real lighting, and writing it out
/// by hand for a fifth shape is the wall `ROADMAP.md` Slice 11 exists to remove.
#[rustfmt::skip]
const CUBE_FACES: [([usize; 4], [f32; 3]); 6] = [
    ([4, 5, 6, 7], [ 0.0,  0.0,  1.0]), // front  (+z)
    ([0, 3, 2, 1], [ 0.0,  0.0, -1.0]), // back   (-z)
    ([1, 2, 6, 5], [ 1.0,  0.0,  0.0]), // right  (+x)
    ([0, 4, 7, 3], [-1.0,  0.0,  0.0]), // left   (-x)
    ([3, 7, 6, 2], [ 0.0,  1.0,  0.0]), // top    (+y)
    ([0, 1, 5, 4], [ 0.0, -1.0,  0.0]), // bottom (-y)
];

/// Build the cube mesh in **object space** — centered on the origin, unrotated.
/// Where it ends up in the world is the transform's business, not the mesh's.
fn cube_mesh() -> Mesh {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (corners, normal) in CUBE_FACES {
        let base = vertices.len() as u32;
        for corner in corners {
            let (position, color) = CUBE_CORNERS[corner];
            vertices.push(Vertex {
                position,
                normal,
                color,
            });
        }
        // Two triangles over the quad's four fresh vertices.
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

/// A consumer that tumbles a cube by moving it, not by rebuilding it.
#[derive(Default)]
struct CubeDemo {
    /// The uploaded geometry, set once in `init` and never re-uploaded.
    cube: Option<MeshHandle>,
    /// Accumulated rotation, advanced every frame.
    angle: f32,
}

impl Application for CubeDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        self.cube = Some(renderer.upload_mesh(&cube_mesh()));
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Fixed per-frame step: simplest cross-platform spin (no timer). The rate
        // is frame-rate dependent, which is fine for a demo.
        self.angle += 0.01;

        let Some(cube) = self.cube else { return };
        // Yaw about Y and pitch about X, exactly as the old CPU rotation did —
        // but as one matrix the GPU applies, rather than eight rotated corners
        // uploaded again every frame.
        renderer.set_instances(&[Instance::new(
            cube,
            Transform::IDENTITY.with_rotation([self.angle * 0.6, self.angle, 0.0]),
        )]);
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
