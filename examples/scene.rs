//! A stage of independently moving objects — the demo that pulled per-object
//! transforms into the engine.
//!
//! Every demo before this one drew geometry that was already baked into world
//! space, so "move something" meant rebuilding its vertices and handing them over
//! again. That works for one cube and for nothing else. This one holds **two
//! meshes and dozens of objects**, and after `init` it never uploads a vertex
//! again: each frame it hands the engine a list of *placements*.
//!
//! Deliberately content-free — no terrain, no game, nothing but "several things,
//! in different places, that a viewer can tell apart." What it proves:
//!
//! - **One mesh, many instances.** The box is uploaded once. Every object on the
//!   stage is the same [`MeshHandle`] at a different [`Transform`], and the engine
//!   batches them into a single instanced draw call.
//! - **Two meshes, two draw calls.** The ground is a second handle, so the
//!   draw-list groups by mesh rather than issuing one call per object.
//! - **Scale is a transform too.** The ground is a *unit* quad stretched to the
//!   size of the stage, which is why widening the stage moves no geometry either.
//! - **Zero per-frame vertex traffic.** The HUD says so; the number is real.
//!
//! What it does *not* do yet, and honestly: every box is the same color, because
//! color lives in the shared vertex buffer that all instances read. Telling two
//! instances apart by material is the next roadblock (`ROADMAP.md`, Slice 10), and
//! this demo is what raises it. The shading you can see is baked into the box's
//! corners, so it turns with the object — the argument for real normals and
//! in-pipeline lighting (Slice 9).
//!
//! Run it:
//!   native — `cargo run --example scene`
//!   web    — `cargo xtask serve scene`

use slmsttaa::ui::{Anchor, Theme};
use slmsttaa::{
    run, Application, Instance, Key, Mesh, MeshHandle, MouseButton, RenderMode, Renderer,
    Transform, Vertex,
};

/// Width of the parameter panel and the HUD, in logical points.
const PANEL_W: f32 = 240.0;
const HUD_W: f32 = 210.0;

/// World-space spacing between object centers.
const SPACING: f32 = 1.5;
/// Objects per side of the stage, and the range the slider covers.
const SIDE_DEFAULT: f32 = 5.0;
const SIDE_MIN: f32 = 1.0;
const SIDE_MAX: f32 = 8.0;

// --- Geometry ----------------------------------------------------------------

/// A unit box centered on the origin, with a vertical gradient baked into its
/// corners so the faces read as separate surfaces.
///
/// Object space, not world space: it is authored once, at the origin, and every
/// object on the stage is this same mesh somewhere else. That is the whole point
/// of the slice — before it, this function would have taken a position.
fn box_mesh() -> Mesh {
    /// Corner shade at the bottom and the top of the box.
    const LOW: [f32; 3] = [0.14, 0.17, 0.24];
    const HIGH: [f32; 3] = [0.42, 0.48, 0.60];

    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];

    let vertices = corners
        .iter()
        .map(|&position| {
            // 0 at the base, 1 at the top.
            let t = position[1] + 0.5;
            Vertex {
                position,
                color: [
                    LOW[0] + (HIGH[0] - LOW[0]) * t,
                    LOW[1] + (HIGH[1] - LOW[1]) * t,
                    LOW[2] + (HIGH[2] - LOW[2]) * t,
                ],
            }
        })
        .collect();

    // 12 triangles, each wound counter-clockwise seen from outside, so back-face
    // culling keeps the exterior.
    #[rustfmt::skip]
    let indices = vec![
        4, 5, 6,  4, 6, 7, // front  (+z)
        0, 2, 1,  0, 3, 2, // back   (-z)
        1, 2, 6,  1, 6, 5, // right  (+x)
        0, 4, 7,  0, 7, 3, // left   (-x)
        3, 7, 6,  3, 6, 2, // top    (+y)
        0, 1, 5,  0, 5, 4, // bottom (-y)
    ];

    Mesh::new(vertices, indices)
}

/// A **unit** quad on the XZ plane, wound counter-clockwise seen from above.
///
/// One unit across, deliberately: the stage grows and shrinks with the object
/// count, and stretching this with a scale is free where rebuilding it would cost
/// an upload every time the slider moved.
fn ground_mesh() -> Mesh {
    const SHADE: [f32; 3] = [0.10, 0.12, 0.16];
    let vertices = [
        [-0.5, 0.0, -0.5],
        [-0.5, 0.0, 0.5],
        [0.5, 0.0, 0.5],
        [0.5, 0.0, -0.5],
    ]
    .iter()
    .map(|&position| Vertex {
        position,
        color: SHADE,
    })
    .collect();
    Mesh::new(vertices, vec![0, 1, 2, 0, 2, 3])
}

/// A deterministic pseudo-random number in `[0, 1)` from an integer seed (a PCG
/// output hash). Gives each object its own size and rhythm without storing a
/// per-object struct or pulling in an RNG dependency.
fn hash01(seed: u32) -> f32 {
    let state = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    let hashed = (word >> 22) ^ word;
    hashed as f32 / u32::MAX as f32
}

// --- The consumer ------------------------------------------------------------

/// A stage of boxes that move independently over a ground plane.
struct SceneDemo {
    /// The two uploaded meshes. `None` until `init`.
    ground: Option<MeshHandle>,
    boxes: Option<MeshHandle>,
    /// This frame's draw-list, rebuilt in place so the allocation is reused.
    instances: Vec<Instance>,

    /// Objects per side of the square stage (a slider, so it's an `f32`).
    side: f32,
    /// How fast objects turn, and how far they bob.
    spin: f32,
    bob: f32,
    /// Draw the stage as a wireframe.
    wireframe: bool,
    /// The UI theme, owned by the consumer and re-applied every frame.
    theme: Theme,

    /// Seconds since start, accumulated from the frame clock. Frame-rate
    /// *independent* motion, unlike `cube`'s fixed per-frame step — but still
    /// wall-clock, which is the defect the fixed-timestep slice exists to fix.
    time: f32,

    /// Orbit camera state (azimuth, elevation, range).
    yaw: f32,
    pitch: f32,
    distance: f32,

    /// Smoothed frames-per-second for the HUD.
    fps: f32,
}

impl Default for SceneDemo {
    fn default() -> Self {
        Self {
            ground: None,
            boxes: None,
            instances: Vec::new(),
            side: SIDE_DEFAULT,
            spin: 0.6,
            bob: 0.25,
            wireframe: false,
            theme: Theme::dark(),
            time: 0.0,
            yaw: 0.7,
            pitch: 0.45,
            distance: 14.0,
            fps: 60.0,
        }
    }
}

impl SceneDemo {
    /// Objects per side, snapped to a whole number.
    fn side_count(&self) -> usize {
        self.side.round().clamp(SIDE_MIN, SIDE_MAX) as usize
    }

    /// How wide the stage is, in world units.
    fn stage_extent(&self) -> f32 {
        self.side_count() as f32 * SPACING + SPACING
    }

    /// Rebuild the draw-list for the current time.
    ///
    /// This is the per-frame cost of a moving scene now: one [`Transform`] per
    /// object. No mesh is touched, which is why the HUD can honestly claim zero
    /// vertex uploads per frame.
    fn rebuild_instances(&mut self) {
        let (Some(ground), Some(boxes)) = (self.ground, self.boxes) else {
            return;
        };

        self.instances.clear();

        // The ground: one unit quad, stretched to the size of the stage. Scale is
        // part of the transform, so widening the stage costs nothing.
        let extent = self.stage_extent();
        self.instances.push(Instance::new(
            ground,
            Transform::IDENTITY.with_scale([extent, 1.0, extent]),
        ));

        // The objects: the same box, once per stage cell, each with its own size,
        // spin rate, and bob phase.
        let n = self.side_count();
        let origin = -(n as f32 - 1.0) * 0.5 * SPACING;
        for row in 0..n {
            for col in 0..n {
                let i = (row * n + col) as u32;

                // Deterministic per-object character.
                let height = 0.5 + hash01(i * 3) * 1.6;
                let rate = 0.4 + hash01(i * 3 + 1) * 1.6;
                let phase = hash01(i * 3 + 2) * std::f32::consts::TAU;

                // Bob about the resting height, never sinking through the ground.
                let lift = height * 0.5 + self.bob * (1.0 + (self.time * rate + phase).sin());

                self.instances.push(Instance::new(
                    boxes,
                    Transform {
                        position: [
                            origin + col as f32 * SPACING,
                            lift,
                            origin + row as f32 * SPACING,
                        ],
                        rotation: [0.0, self.time * self.spin * rate + phase, 0.0],
                        scale: [0.6, height, 0.6],
                    },
                ));
            }
        }
    }

    /// Declare the UI. Returns whether the pointer is over a widget, so the orbit
    /// camera can leave the mouse alone while a slider is being dragged.
    fn build_ui(&mut self, renderer: &mut Renderer) -> bool {
        let theme = self.theme;
        let fps = self.fps;
        let objects = self.instances.len().saturating_sub(1);
        let mut light = self.theme == Theme::light();

        let mut ui = renderer.ui();
        ui.set_theme(theme);

        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.title("Scene");
            ui.separator();
            ui.slider("objects per side", &mut self.side, SIDE_MIN, SIDE_MAX)
                .decimals(0)
                .show();
            ui.slider("spin", &mut self.spin, 0.0, 2.0).show();
            ui.slider("bob", &mut self.bob, 0.0, 0.8).show();
        });

        ui.panel(Anchor::TopRight, HUD_W, |ui| {
            ui.label_value("fps", &format!("{fps:.0}"));
            ui.label_value("meshes", "2");
            ui.label_value("objects", &format!("{objects}"));
            // Two meshes appear in the draw-list, so two instanced calls carry
            // however many objects there are.
            ui.label_value("draw calls", "2");
            ui.label_value("uploads/frame", "0");
            ui.checkbox("wireframe", &mut self.wireframe);
            ui.checkbox("light", &mut light);
        });

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        self.theme = if light { Theme::light() } else { Theme::dark() };
        wants_pointer
    }

    /// Orbit the stage: drag to turn, arrow keys likewise, scroll to zoom.
    fn drive_camera(&mut self, renderer: &mut Renderer, ui_has_pointer: bool) {
        let input = renderer.input();
        let dragging = input.is_mouse_held(MouseButton::Left) && !ui_has_pointer;
        let (mdx, mdy) = input.mouse_delta();
        let scroll = if ui_has_pointer {
            0.0
        } else {
            input.scroll_delta()
        };
        let left = input.is_key_held(Key::Left);
        let right = input.is_key_held(Key::Right);
        let up = input.is_key_held(Key::Up);
        let down = input.is_key_held(Key::Down);

        if dragging {
            self.yaw -= mdx * 0.005;
            self.pitch -= mdy * 0.005;
        }
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
        self.distance -= scroll * 0.8;

        self.pitch = self.pitch.clamp(0.05, 1.4);
        self.distance = self.distance.clamp(3.0, 40.0);

        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let eye = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        renderer.camera_mut().look_from_to(eye, [0.0, 0.8, 0.0]);
    }
}

impl Application for SceneDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        // The only geometry upload in the entire program.
        self.ground = Some(renderer.upload_mesh(&ground_mesh()));
        self.boxes = Some(renderer.upload_mesh(&box_mesh()));
    }

    fn update(&mut self, renderer: &mut Renderer) {
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }
        // Motion off the frame clock, so the stage moves at the same speed
        // whatever the frame rate.
        self.time += dt;

        let ui_has_pointer = self.build_ui(renderer);
        self.drive_camera(renderer, ui_has_pointer);

        self.rebuild_instances();
        renderer.set_instances(&self.instances);
        renderer.set_render_mode(if self.wireframe {
            RenderMode::Wireframe
        } else {
            RenderMode::Solid
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(SceneDemo::default()) {
        eprintln!("scene example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let _ = run(SceneDemo::default());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
