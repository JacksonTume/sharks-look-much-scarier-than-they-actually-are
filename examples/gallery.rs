//! A gallery of demo scenes with on-screen buttons to switch between them.
//!
//! Like the other examples this is a separate crate that only sees the public
//! API, but it does two extra things worth noting:
//!
//! (The default web build is now `terrain`; this gallery stays as the example of
//! runtime scene-switching and the DOM-button input hack.)
//!
//! - **It owns several scenes** (a triangle, a quad, a spinning cube, and an
//!   orbitable terrain grid) and swaps the engine's draw-list between them at
//!   runtime via [`Renderer::set_meshes`] — no engine restart, no page reload.
//! - **The grid scene is camera-driven.** It reads the engine's winit-free input
//!   via [`Renderer::input`] and aims the camera with [`Renderer::camera_mut`]:
//!   drag the left mouse button to orbit, scroll to zoom (it drifts on its own
//!   when idle). The flat scenes keep a fixed front-on view.
//! - **On the web it builds real buttons** for *scene selection*. The engine now
//!   exposes raw keyboard/mouse input but no UI-widget abstraction, so the demo
//!   still creates DOM `<button>`s with `web-sys` and shares an `Rc<Cell<usize>>`
//!   with its `update` loop: a click sets the selected scene, and the next
//!   `update` notices and swaps geometry.
//!
//! On native there is no DOM, so the gallery instead **auto-cycles** through the
//! scenes every few seconds — the example stays meaningful on both targets
//! (principle: always keep something on screen).
//!
//! Run it:
//!   native — `cargo run --example gallery`
//!   web    — build for wasm and run `wasm-bindgen` (see `README.md`), substituting
//!            `gallery` for `triangle`.

use std::cell::Cell;
use std::rc::Rc;

use slmsttaa::{run, Application, Mesh, MouseButton, Renderer, Vertex};

/// The scenes the gallery can show, in button order.
const SCENES: [Scene; 4] = [Scene::Triangle, Scene::Quad, Scene::Cube, Scene::Grid];

/// On native, advance to the next scene every this many frames (~3s at 60fps).
#[cfg(not(target_arch = "wasm32"))]
const NATIVE_CYCLE_FRAMES: u32 = 180;

/// One selectable scene. Each knows its button label, whether it animates, and how
/// to build its [`Mesh`] (rotated by `angle` for the ones that spin).
#[derive(Clone, Copy)]
enum Scene {
    Triangle,
    Quad,
    Cube,
    Grid,
}

impl Scene {
    /// Text shown on the scene's button. Only used on the web (native auto-cycles).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn label(self) -> &'static str {
        match self {
            Scene::Triangle => "Triangle",
            Scene::Quad => "Quad",
            Scene::Cube => "Cube",
            Scene::Grid => "Grid",
        }
    }

    /// Whether the scene animates its *geometry* and so must be re-uploaded every
    /// frame. (The grid moves only the camera, so it stays `false`.)
    fn spins(self) -> bool {
        matches!(self, Scene::Cube)
    }

    /// Whether the consumer drives the camera for this scene (orbit). Flat scenes
    /// keep a fixed front-on view instead.
    fn orbits(self) -> bool {
        matches!(self, Scene::Grid)
    }

    /// Build the scene's geometry at rotation `angle` (ignored by static scenes).
    fn build(self, angle: f32) -> Mesh {
        match self {
            Scene::Triangle => Mesh::new(
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
            ),
            Scene::Quad => Mesh::new(
                vec![
                    Vertex {
                        position: [-0.5, -0.5, 0.0],
                        color: [1.0, 0.2, 0.3],
                    },
                    Vertex {
                        position: [0.5, -0.5, 0.0],
                        color: [0.2, 1.0, 0.4],
                    },
                    Vertex {
                        position: [0.5, 0.5, 0.0],
                        color: [0.3, 0.4, 1.0],
                    },
                    Vertex {
                        position: [-0.5, 0.5, 0.0],
                        color: [1.0, 1.0, 0.3],
                    },
                ],
                // Two triangles, wound CCW (front toward the camera).
                vec![0, 1, 2, 0, 2, 3],
            ),
            Scene::Cube => cube_mesh(angle),
            Scene::Grid => grid_mesh(),
        }
    }
}

/// The 8 corners of a unit cube, colored by position (matches `examples/cube.rs`).
const CUBE_CORNERS: [([f32; 3], [f32; 3]); 8] = [
    ([-0.5, -0.5, -0.5], [0.0, 0.0, 0.0]),
    ([0.5, -0.5, -0.5], [1.0, 0.0, 0.0]),
    ([0.5, 0.5, -0.5], [1.0, 1.0, 0.0]),
    ([-0.5, 0.5, -0.5], [0.0, 1.0, 0.0]),
    ([-0.5, -0.5, 0.5], [0.0, 0.0, 1.0]),
    ([0.5, -0.5, 0.5], [1.0, 0.0, 1.0]),
    ([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
    ([-0.5, 0.5, 0.5], [0.0, 1.0, 1.0]),
];

/// 12 triangles, each wound CCW from outside so back-face culling keeps the solid.
#[rustfmt::skip]
const CUBE_INDICES: [u32; 36] = [
    4, 5, 6,  4, 6, 7, // front  (+z)
    0, 2, 1,  0, 3, 2, // back   (-z)
    1, 2, 6,  1, 6, 5, // right  (+x)
    0, 4, 7,  0, 7, 3, // left   (-x)
    3, 7, 6,  3, 6, 2, // top    (+y)
    0, 1, 5,  0, 5, 4, // bottom (-y)
];

/// Build the cube rotated by `angle` (yaw about Y, plus a gentler pitch about X).
fn cube_mesh(angle: f32) -> Mesh {
    let (sy, cy) = angle.sin_cos();
    let (sp, cp) = (angle * 0.6).sin_cos();
    let vertices = CUBE_CORNERS
        .iter()
        .map(|&(p, color)| {
            let x = p[0] * cy + p[2] * sy;
            let z = -p[0] * sy + p[2] * cy;
            let y = p[1];
            Vertex {
                position: [x, y * cp - z * sp, y * sp + z * cp],
                color,
            }
        })
        .collect();
    Mesh::new(vertices, CUBE_INDICES.to_vec())
}

// --- Grid (mirrors `examples/grid.rs`; the orbit scene) ----------------------

/// Vertices per side of the grid.
const GRID_N: usize = 64;
/// Half-extent of the grid in world units (spans `[-HALF, HALF]` on X and Z).
const GRID_HALF: f32 = 2.0;

/// Static terrain height at `(x, z)`: a broad central hill plus gentle ripples.
fn grid_height(x: f32, z: f32) -> f32 {
    let r2 = x * x + z * z;
    let hill = 0.9 * (-r2 * 0.6).exp();
    let ripple = 0.08 * (x * 3.0).sin() * (z * 3.0).cos();
    hill + ripple
}

/// Build the static `GRID_N x GRID_N` terrain grid, colored low→high.
fn grid_mesh() -> Mesh {
    let step = (2.0 * GRID_HALF) / (GRID_N as f32 - 1.0);

    let mut vertices = Vec::with_capacity(GRID_N * GRID_N);
    for i in 0..GRID_N {
        for j in 0..GRID_N {
            let x = -GRID_HALF + j as f32 * step;
            let z = -GRID_HALF + i as f32 * step;
            let y = grid_height(x, z);
            let t = y.clamp(0.0, 1.0);
            let color = [
                0.16 + (0.92 - 0.16) * t,
                0.42 + (0.93 - 0.42) * t,
                0.18 + (0.88 - 0.18) * t,
            ];
            vertices.push(Vertex {
                position: [x, y, z],
                color,
            });
        }
    }

    // Two triangles per cell, wound CCW from above so culling keeps the top.
    let mut indices = Vec::with_capacity((GRID_N - 1) * (GRID_N - 1) * 6);
    let idx = |i: usize, j: usize| (i * GRID_N + j) as u32;
    for i in 0..GRID_N - 1 {
        for j in 0..GRID_N - 1 {
            let a = idx(i, j);
            let b = idx(i, j + 1);
            let c = idx(i + 1, j + 1);
            let d = idx(i + 1, j);
            indices.extend_from_slice(&[a, d, b, b, d, c]);
        }
    }

    Mesh::new(vertices, indices)
}

/// The gallery consumer: tracks which scene is selected and re-uploads on change.
struct GalleryDemo {
    /// Selected scene index. On the web, button clicks write to this; `update`
    /// reads it. Shared via `Rc` so the DOM click closures can hold their own clone.
    selected: Rc<Cell<usize>>,
    /// The scene currently uploaded, so we only re-upload when it changes.
    current: usize,
    /// Rotation accumulator for animated scenes.
    angle: f32,
    /// Orbit state for the camera-driven grid scene (azimuth, elevation, range).
    yaw: f32,
    pitch: f32,
    distance: f32,
    /// Native-only frame counter driving the auto-cycle.
    #[cfg(not(target_arch = "wasm32"))]
    frames: u32,
}

impl GalleryDemo {
    fn new() -> Self {
        Self {
            selected: Rc::new(Cell::new(0)),
            current: 0,
            angle: 0.0,
            yaw: 0.7,
            pitch: 0.6,
            distance: 6.0,
            #[cfg(not(target_arch = "wasm32"))]
            frames: 0,
        }
    }

    /// Aim the camera for the current scene: orbit the grid (drag/scroll, with a
    /// gentle idle drift), or sit at a fixed front-on view for the flat scenes.
    fn drive_camera(&mut self, renderer: &mut Renderer) {
        if !SCENES[self.current].orbits() {
            // Matches the engine's default eye, so the flat scenes look unchanged.
            renderer
                .camera_mut()
                .look_from_to([0.0, 1.0, 3.0], [0.0, 0.0, 0.0]);
            return;
        }

        // Read input out first (ends the immutable borrow before `camera_mut`).
        let input = renderer.input();
        let dragging = input.is_mouse_held(MouseButton::Left);
        let (mdx, mdy) = input.mouse_delta();
        let scroll = input.scroll_delta();

        if dragging {
            self.yaw -= mdx * 0.005;
            self.pitch -= mdy * 0.005;
        } else {
            // Idle: drift slowly so the grid stays lively on native too.
            self.yaw += 0.004;
        }
        self.distance -= scroll * 0.5;
        self.pitch = self.pitch.clamp(0.08, 1.5);
        self.distance = self.distance.clamp(2.0, 20.0);

        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let eye = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        renderer.camera_mut().look_from_to(eye, [0.0, 0.0, 0.0]);
    }
}

impl Application for GalleryDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        #[cfg(target_arch = "wasm32")]
        create_buttons(&self.selected);

        let sel = self.selected.get();
        renderer.set_meshes(&[SCENES[sel].build(0.0)]);
        self.current = sel;
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Native has no buttons, so step through the scenes automatically.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.frames += 1;
            if self.frames % NATIVE_CYCLE_FRAMES == 0 {
                let next = (self.selected.get() + 1) % SCENES.len();
                self.selected.set(next);
            }
        }

        let sel = self.selected.get();
        if sel != self.current {
            // Scene changed (button click or auto-cycle): swap geometry.
            self.current = sel;
            self.angle = 0.0;
            // Start the orbit scene from a pleasant three-quarter view.
            self.yaw = 0.7;
            self.pitch = 0.6;
            self.distance = 6.0;
            renderer.set_meshes(&[SCENES[sel].build(0.0)]);
        } else if SCENES[self.current].spins() {
            // Same scene, but it animates: advance and re-upload.
            self.angle += 0.01;
            renderer.set_meshes(&[SCENES[self.current].build(self.angle)]);
        }

        // Aim the camera (orbit the grid; fixed front view otherwise).
        self.drive_camera(renderer);
    }
}

/// Build a small button bar (one button per scene) over the canvas. Each button's
/// click sets the shared selected-scene index that `update` polls. Web only.
#[cfg(target_arch = "wasm32")]
fn create_buttons(selected: &Rc<Cell<usize>>) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };

    let Ok(container) = document.create_element("div") else {
        return;
    };
    let _ = container.set_attribute(
        "style",
        "position:fixed;top:12px;left:12px;z-index:10;display:flex;gap:8px;",
    );

    for (i, scene) in SCENES.iter().enumerate() {
        let Ok(button) = document.create_element("button") else {
            continue;
        };
        button.set_inner_html(scene.label());
        let _ = button.set_attribute(
            "style",
            "padding:6px 12px;border:0;border-radius:6px;cursor:pointer;\
             background:#1f6feb;color:#fff;font:14px system-ui,sans-serif;",
        );

        // Each closure owns a clone of the shared selection and the scene index.
        let selected = selected.clone();
        let on_click = Closure::<dyn FnMut()>::new(move || selected.set(i));
        let _ = button.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
        // Hand the closure to the JS GC's keep-alive; it must outlive this call.
        on_click.forget();

        let _ = container.append_child(&button);
    }

    let _ = body.append_child(&container);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(GalleryDemo::new()) {
        eprintln!("gallery example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Errors here can't be propagated to JS meaningfully; `run` logs to the
    // browser console on its own.
    let _ = run(GalleryDemo::new());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
