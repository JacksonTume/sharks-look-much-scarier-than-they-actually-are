//! An orbitable terrain grid — the proof for Slice 3 (a camera the consumer drives).
//!
//! Like the other examples this is a separate crate that can only see `slmsttaa`'s
//! public API. It demonstrates the new input + camera seam:
//!
//! - **Input, winit-free:** the engine funnels mouse/keyboard events into an
//!   [`Input`] snapshot the demo reads via [`Renderer::input`] — no `winit` in
//!   sight (engine principle 1).
//! - **A consumer-driven camera:** the demo owns an [`Orbit`] — its viewpoint,
//!   its limits, and the decision of which button turns it — and writes the eye
//!   through [`Renderer::camera_mut`] each frame.
//!
//!   *This used to say the orbit math lived here, and for nineteen slices it
//!   did.* Slice 3 declined to push it down because there was one consumer; by
//!   the time there were six, all six had the same spherical-coordinate block
//!   and the same frame-rate bug in it. What moved into the engine is only the
//!   arithmetic they agreed on. What is still the demo's is everything below:
//!   where the camera aims, which button orbits, and what the limits are.
//!
//! The geometry is a static height-mapped grid (a gentle hill), uploaded once: the
//! scene is interesting precisely because you move the *camera*, not the mesh.
//! Slice 4 will reuse this grid and start mutating its heights (erosion).
//!
//! Controls: **drag the left mouse button** to orbit, **scroll** to zoom, or use
//! the **arrow keys** to orbit.
//!
//! Run it:
//!   native — `cargo run --example grid`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `grid` for `triangle`.

use slmsttaa::{
    run, Application, Instance, Mesh, MouseButton, Orbit, OrbitInput, Renderer, Vertex,
};

/// Vertices per side of the grid. `N * N` vertices, `(N-1)^2 * 2` triangles.
const N: usize = 64;
/// Half-extent of the grid in world units (it spans `[-HALF, HALF]` on X and Z).
const HALF: f32 = 2.0;

/// Static terrain height at grid position `(x, z)`: one broad central hill plus a
/// couple of gentle ripples, so the relief reads clearly from any orbit angle.
fn height(x: f32, z: f32) -> f32 {
    let r2 = x * x + z * z;
    let hill = 0.9 * (-r2 * 0.6).exp();
    let ripple = 0.08 * (x * 3.0).sin() * (z * 3.0).cos();
    hill + ripple
}

/// Linearly blend two RGB colors by `t` in `[0, 1]`.
fn lerp_color(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The surface normal at `(x, z)`, by central difference over one cell.
///
/// A heightfield's normal falls out of its slopes: step one cell each way, and
/// the cross product of the two tangents is `(-dy/dx, 1, -dy/dz)` normalized.
fn surface_normal(x: f32, z: f32, step: f32) -> [f32; 3] {
    let dx = (height(x + step, z) - height(x - step, z)) / (2.0 * step);
    let dz = (height(x, z + step) - height(x, z - step)) / (2.0 * step);
    let len = (dx * dx + 1.0 + dz * dz).sqrt();
    [-dx / len, 1.0 / len, -dz / len]
}

/// Build the static grid mesh: an `N x N` lattice on the XZ plane, displaced by
/// [`height`] and colored low→high (green valley to pale peak).
fn grid_mesh() -> Mesh {
    let step = (2.0 * HALF) / (N as f32 - 1.0);

    let mut vertices = Vec::with_capacity(N * N);
    for i in 0..N {
        for j in 0..N {
            let x = -HALF + j as f32 * step;
            let z = -HALF + i as f32 * step;
            let y = height(x, z);
            // Normalize height into [0, 1] for coloring (hill peaks near ~0.95).
            let t = (y / 1.0).clamp(0.0, 1.0);
            let color = lerp_color([0.16, 0.42, 0.18], [0.92, 0.93, 0.88], t);
            vertices.push(Vertex {
                position: [x, y, z],
                normal: surface_normal(x, z, step),
                color: [color[0], color[1], color[2], 1.0],
            });
        }
    }

    // Two triangles per cell, wound CCW as seen from above (+Y) so back-face
    // culling keeps the top surface.
    let mut indices = Vec::with_capacity((N - 1) * (N - 1) * 6);
    let idx = |i: usize, j: usize| (i * N + j) as u32;
    for i in 0..N - 1 {
        for j in 0..N - 1 {
            let a = idx(i, j);
            let b = idx(i, j + 1);
            let c = idx(i + 1, j + 1);
            let d = idx(i + 1, j);
            indices.extend_from_slice(&[a, d, b, b, d, c]);
        }
    }

    Mesh::new(vertices, indices)
}

/// The orbit-camera consumer. The viewpoint is an [`Orbit`]; the limits and the
/// gating are this demo's to choose.
struct GridDemo {
    orbit: Orbit,
}

impl Default for GridDemo {
    fn default() -> Self {
        Self {
            // The defaults happen to be exactly this demo's: it is where they were
            // read off. Written out anyway, because a demo that relied on the
            // engine's taste would stop being a statement of what it wants.
            orbit: Orbit {
                pitch_range: (0.08, 1.5),
                distance_range: (2.0, 20.0),
                zoom_per_notch: 0.5,
                ..Orbit::new(0.7, 0.6, 6.0)
            },
        }
    }
}

impl Application for GridDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        // Static geometry: upload once and place it once. Only the camera moves
        // after this, so the draw-list is never touched again.
        let grid = renderer.upload_mesh(&grid_mesh());
        renderer.set_instances(&[Instance::at(grid)]);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Nothing overlays the scene here, so the keys and the wheel are always
        // the camera's. Only the drag is a decision: it turns while the left
        // button is held and not otherwise.
        let dragging = renderer.input().is_mouse_held(MouseButton::Left);
        let dt = renderer.dt();
        self.orbit.drive(
            renderer.input(),
            dt,
            OrbitInput {
                drag: dragging,
                ..OrbitInput::ALL
            },
        );

        // Where it aims is still the demo's: this grid is centred on the origin,
        // so it looks straight at it.
        renderer
            .camera_mut()
            .look_from_to(self.orbit.eye(), [0.0, 0.0, 0.0]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(GridDemo::default()) {
        eprintln!("grid example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Errors here can't be propagated to JS meaningfully; `run` logs to the
    // browser console on its own.
    let _ = run(GridDemo::default());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
