//! A spinning, solid cube — the proof for indexed meshes + depth + culling.
//!
//! Like `triangle`, this lives in `examples/` and is compiled as a separate crate
//! that can only see `slmsttaa`'s public API. It exercises three things at once:
//!
//! - **Indexed drawing:** the cube's 12 triangles index into a shared vertex
//!   pool rather than spelling out 36 loose vertices.
//! - **Depth testing:** as it tumbles, near faces correctly occlude far ones.
//! - **Back-face culling:** every face is wound counter-clockwise *from outside*,
//!   so the inward-facing back triangles are dropped and the solid stays solid.
//!
//! It also shows what a per-object transform is *for*. The cube used to spin by
//! rotating its 8 corners on the CPU and re-uploading the whole mesh every frame;
//! now the mesh is uploaded once in `init` and every frame hands over one
//! [`Transform`]. The geometry never moves — the object does.
//!
//! The cube itself is [`Mesh::cuboid`] — this file used to spell out its corners,
//! its six faces and their normals by hand, and now it doesn't. Color comes from
//! a per-instance [`Material`] instead of from the corners, which is why the
//! faces still read apart as it turns: the *lighting* distinguishes them now,
//! rather than a rainbow baked into the vertices.
//!
//! Run it:
//!   native — `cargo run --example cube`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `cube` for `triangle`.

use slmsttaa::{run, Application, Instance, Material, Mesh, MeshHandle, Renderer, Transform};

/// A consumer that tumbles a cube by moving it, not by rebuilding it.
#[derive(Default)]
struct CubeDemo {
    /// The uploaded geometry, set once in `init` and never re-uploaded.
    cube: Option<MeshHandle>,
    /// Accumulated rotation, advanced one fixed step at a time.
    angle: f32,
}

/// How fast the cube spins, in radians per second of simulation time.
const SPIN_RATE: f32 = 0.6;

impl Application for CubeDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        self.cube = Some(renderer.upload_mesh(&Mesh::cuboid([1.0; 3])));
    }

    /// This demo used to add a flat `0.01` per *frame*, so the cube spun half as
    /// fast on a 30 Hz machine and twice as fast on a 144 Hz one. It was the
    /// clearest instance of the defect in the tree, and moving one line into the
    /// fixed hook is the whole fix: the step is the same number everywhere.
    fn fixed_update(&mut self, _renderer: &mut Renderer, dt: f32) {
        self.angle += SPIN_RATE * dt;
    }

    fn update(&mut self, renderer: &mut Renderer) {
        let Some(cube) = self.cube else { return };
        // Yaw about Y and pitch about X, exactly as the old CPU rotation did —
        // but as one matrix the GPU applies, rather than eight rotated corners
        // uploaded again every frame.
        renderer.set_instances(&[Instance::new(
            cube,
            Transform::IDENTITY.with_rotation([self.angle * 0.6, self.angle, 0.0]),
        )
        .with_material(Material::rgb(0.55, 0.62, 0.78))]);
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
