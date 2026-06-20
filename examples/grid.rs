//! An orbitable terrain grid — the proof for Slice 3 (a camera the consumer drives).
//!
//! Like the other examples this is a separate crate that can only see `slmsttaa`'s
//! public API. It demonstrates the new input + camera seam:
//!
//! - **Input, winit-free:** the engine funnels mouse/keyboard events into an
//!   [`Input`] snapshot the demo reads via [`Renderer::input`] — no `winit` in
//!   sight (engine principle 1).
//! - **A consumer-driven camera:** the demo keeps its own orbit state
//!   (`yaw`/`pitch`/`distance`) and writes the eye position through
//!   [`Renderer::camera_mut`] each frame. The *orbit math lives here*, not in the
//!   engine — the engine only exposes input and the camera.
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

use slmsttaa::{run, Application, Key, Mesh, MouseButton, Renderer, Vertex};

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
                color,
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

/// The orbit-camera consumer. Holds the viewpoint as spherical coordinates around
/// a fixed target and rebuilds the eye position from input every frame.
struct GridDemo {
    /// Azimuth around the target, in radians.
    yaw: f32,
    /// Elevation above the ground plane, in radians (clamped away from the poles).
    pitch: f32,
    /// Distance from the target to the eye.
    distance: f32,
}

impl Default for GridDemo {
    fn default() -> Self {
        Self {
            yaw: 0.7,
            pitch: 0.6,
            distance: 6.0,
        }
    }
}

impl Application for GridDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        // Static geometry: upload once. Only the camera moves after this.
        renderer.set_meshes(&[grid_mesh()]);
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Read everything we need out of the input snapshot first; that ends the
        // immutable borrow of `renderer` before we take the mutable `camera_mut`
        // borrow below. (`Input`'s getters all return `Copy` values.)
        let input = renderer.input();
        let mouse_held = input.is_mouse_held(MouseButton::Left);
        let (mdx, mdy) = input.mouse_delta();
        let scroll = input.scroll_delta();
        let left = input.is_key_held(Key::Left);
        let right = input.is_key_held(Key::Right);
        let up = input.is_key_held(Key::Up);
        let down = input.is_key_held(Key::Down);

        // Mouse drag: only orbit while the left button is held.
        if mouse_held {
            self.yaw -= mdx * 0.005;
            self.pitch -= mdy * 0.005;
        }

        // Arrow keys: the "with keys" path. Fixed per-frame step (frame-rate
        // dependent, like `cube`); a real frame clock is a later slice.
        const KEY_STEP: f32 = 0.03;
        if left {
            self.yaw += KEY_STEP;
        }
        if right {
            self.yaw -= KEY_STEP;
        }
        if up {
            self.pitch += KEY_STEP;
        }
        if down {
            self.pitch -= KEY_STEP;
        }

        // Scroll to zoom.
        self.distance -= scroll * 0.5;

        // Keep the eye above the ground (out of the poles) and the zoom sane.
        self.pitch = self.pitch.clamp(0.08, 1.5);
        self.distance = self.distance.clamp(2.0, 20.0);

        // Spherical → Cartesian around the origin target.
        let target = [0.0, 0.0, 0.0];
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let eye = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        renderer.camera_mut().look_from_to(eye, target);
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
