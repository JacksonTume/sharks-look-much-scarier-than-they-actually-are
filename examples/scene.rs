//! A stage of articulated figures — the demo that pulled per-object transforms,
//! per-instance material, and primitive geometry into the engine.
//!
//! Every demo before this one drew geometry that was already baked into world
//! space, so "move something" meant rebuilding its vertices and handing them over
//! again. That works for one cube and for nothing else. This one holds **four
//! meshes and dozens of jointed parts**, and after `init` it never uploads a
//! vertex again: each frame it hands the engine a list of *placements*.
//!
//! Deliberately content-free — no game, no story, nothing but "several things, in
//! different places, that a viewer can tell apart." What it proves:
//!
//! - **One mesh, many instances.** Every limb on the stage is the same capsule at
//!   a different transform, batched into a single instanced draw call.
//! - **Per-instance material.** Figures are told apart by a `Material` tint, not
//!   by duplicating geometry per color.
//! - **Nothing is hand-written.** All four meshes come from `Mesh::plane`,
//!   `Mesh::cuboid`, `Mesh::sphere`, and `Mesh::capsule` — no vertex arrays in
//!   this file at all, which is the point of the primitives slice.
//! - **Real joints.** A forearm is placed inside an upper arm inside a torso.
//!   That composition is `Transform::then` / `then_matrix`, and it is the reason
//!   those exist: a walking figure that also *turns* cannot be expressed by adding
//!   Euler angles, because rotations about different axes do not compose that way.
//! - **Zero per-frame vertex traffic.** The HUD says so; the number is real.
//!
//! Run it:
//!   native — `cargo run --example scene`
//!   web    — `cargo xtask serve scene`

use slmsttaa::ui::{Anchor, Theme, Variant};
use slmsttaa::{
    run, Application, Instance, Material, Mesh, MeshHandle, MouseButton, Orbit, OrbitInput,
    RenderMode, Renderer, Transform,
};

/// Width of the parameter panel and the HUD, in logical points.
const PANEL_W: f32 = 240.0;
const HUD_W: f32 = 210.0;

/// World-space spacing between figures.
const SPACING: f32 = 2.4;
/// Figures per side of the stage, and the range the slider covers.
const SIDE_DEFAULT: f32 = 3.0;
const SIDE_MIN: f32 = 1.0;
const SIDE_MAX: f32 = 5.0;

/// How far the scrub slider reaches, in seconds of simulation time. An arbitrary
/// window: the walk cycle never repeats exactly (every figure has its own
/// irrational rate), so any instant is as good as any other to jump to.
const SCRUB_MAX: f32 = 60.0;

// --- Figure proportions ------------------------------------------------------
//
// All in world units, all object-space. A figure stands on the ground plane with
// its origin between its feet.

const HIP_HEIGHT: f32 = 0.95;
const TORSO_SIZE: [f32; 3] = [0.46, 0.62, 0.28];
const HEAD_RADIUS: f32 = 0.17;
const LIMB_RADIUS: f32 = 0.075;
const UPPER_ARM: f32 = 0.30;
const FOREARM: f32 = 0.28;
const THIGH: f32 = 0.38;
const SHIN: f32 = 0.36;
const SHOULDER_X: f32 = 0.30;
const HIP_X: f32 = 0.14;

// --- The consumer ------------------------------------------------------------

/// The four uploaded meshes. Every one of a figure's ten parts is one of these.
#[derive(Clone, Copy)]
struct Meshes {
    ground: MeshHandle,
    torso: MeshHandle,
    head: MeshHandle,
    limb: MeshHandle,
}

/// A stage of figures that walk on the spot, independently.
struct SceneDemo {
    meshes: Option<Meshes>,
    /// This frame's draw-list, rebuilt in place so the allocation is reused.
    instances: Vec<Instance>,

    /// Figures per side of the square stage (a slider, so it's an `f32`).
    side: f32,
    /// How fast the figures move their limbs, and how far.
    tempo: f32,
    swing: f32,
    /// Whether figures turn on the spot. This is the control that makes the
    /// hierarchy visible: with it off, adding angles would look correct.
    turn: bool,
    /// Draw the stage as a wireframe.
    wireframe: bool,
    /// The UI theme, owned by the consumer and re-applied every frame.
    theme: Theme,

    /// Seconds of *simulation* time, accumulated one fixed step at a time in
    /// [`Application::fixed_update`] and nowhere else. That restriction is the
    /// whole contract: it is what makes the stage reproduce, and what lets the
    /// transport controls below pause and scrub it.
    ///
    /// It is also this demo's entire simulation state — everything else you see
    /// is derived from it every frame.
    time: f32,

    /// The viewpoint. Its limits are this demo's — a stage you want to see the
    /// whole of wants a shallower floor and a much longer leash than a hill does.
    orbit: Orbit,

    /// Seconds of *wall* time since start, for the HUD row beside `time`. The
    /// two agreeing at any frame rate is this demo's proof; they part only when
    /// the transport controls make them.
    wall: f32,

    /// Smoothed frames-per-second for the HUD.
    fps: f32,
}

impl Default for SceneDemo {
    fn default() -> Self {
        Self {
            meshes: None,
            instances: Vec::new(),
            side: SIDE_DEFAULT,
            tempo: 1.6,
            swing: 0.7,
            turn: true,
            wireframe: false,
            theme: Theme::dark(),
            time: 0.0,
            wall: 0.0,
            orbit: Orbit {
                pitch_range: (0.05, 1.4),
                distance_range: (3.0, 40.0),
                zoom_per_notch: 0.8,
                ..Orbit::new(0.7, 0.35, 12.0)
            },
            fps: 60.0,
        }
    }
}

/// A deterministic pseudo-random number in `[0, 1)` from an integer seed (a PCG
/// output hash). Gives each figure its own rhythm and color without storing a
/// per-figure struct or pulling in an RNG dependency.
fn hash01(seed: u32) -> f32 {
    let state = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    let hashed = (word >> 22) ^ word;
    hashed as f32 / u32::MAX as f32
}

/// A per-figure color, spread around the hue circle so neighbours contrast.
///
/// Deliberately *not* random: stepping by the golden-ratio conjugate means
/// consecutive figures land far apart on the wheel, which is what makes a grid of
/// them legible rather than a muddle of similar pastels.
fn tint_for(index: u32) -> Material {
    let hue = (index as f32 * 0.618_034).fract();
    let [r, g, b] = hue_rgb(hue);
    // Toward white a little, so the lighting still reads on a saturated figure.
    Material::rgb(0.35 + r * 0.6, 0.35 + g * 0.6, 0.35 + b * 0.6)
}

/// A fully saturated RGB for a hue in `[0, 1)` — the six-sector HSV ramp with
/// saturation and value pinned at 1.
fn hue_rgb(hue: f32) -> [f32; 3] {
    let sector = hue * 6.0;
    let ramp = sector.fract();
    match sector as u32 % 6 {
        0 => [1.0, ramp, 0.0],
        1 => [1.0 - ramp, 1.0, 0.0],
        2 => [0.0, 1.0, ramp],
        3 => [0.0, 1.0 - ramp, 1.0],
        4 => [ramp, 0.0, 1.0],
        _ => [1.0, 0.0, 1.0 - ramp],
    }
}

/// One figure's pose at an instant, in its own space.
///
/// Split out from the drawing so the *articulation* is readable on its own: this
/// is a pure function of time, and it is the "spatial synthesis" the roadmap says
/// belongs in the consumer rather than the engine.
struct Pose {
    /// Where the figure stands and which way it faces.
    root: Transform,
    /// Shoulder and elbow angles, left then right.
    arm: [(f32, f32); 2],
    /// Hip and knee angles, left then right.
    leg: [(f32, f32); 2],
    /// Vertical bob of the whole body.
    bounce: f32,
}

impl Pose {
    /// A walk cycle: limbs swing in opposition, knees and elbows fold on the
    /// return stroke, and the body rises twice per stride.
    fn walking(position: [f32; 3], facing: f32, phase: f32, swing: f32) -> Self {
        let (sin, cos) = phase.sin_cos();
        // Elbows and knees only ever bend one way, hence the `max(0.0)`: a leg
        // that folded forwards would read as broken rather than as walking.
        let knee = |s: f32| (-s * 1.4 * swing).max(0.0);
        Self {
            root: Transform::from_position(position).with_rotation([0.0, facing, 0.0]),
            arm: [
                (-sin * swing * 0.8, (sin * 0.9 * swing).max(0.0)),
                (sin * swing * 0.8, (-sin * 0.9 * swing).max(0.0)),
            ],
            leg: [(sin * swing, knee(sin)), (-sin * swing, knee(-sin))],
            bounce: cos.abs() * 0.035 * swing,
        }
    }
}

impl SceneDemo {
    /// Figures per side, snapped to a whole number.
    fn side_count(&self) -> usize {
        self.side.round().clamp(SIDE_MIN, SIDE_MAX) as usize
    }

    /// How wide the stage is, in world units.
    fn stage_extent(&self) -> f32 {
        self.side_count() as f32 * SPACING + SPACING
    }

    /// Append one figure's eleven parts to the draw-list.
    ///
    /// **This is the slice's centerpiece.** Every part below the torso is placed
    /// *relative to its parent*, not in world space: the arm is positioned in the
    /// torso's frame, and the forearm in the arm's. `Transform::then` composes one
    /// level and `then_matrix` continues the chain, and neither could be replaced
    /// by adding Euler angles — a figure that turns about Y while swinging a limb
    /// about X is exactly the case where that fails.
    fn push_figure(&mut self, meshes: Meshes, pose: &Pose, tint: Material) {
        let mut push = |mesh: MeshHandle, model: [[f32; 4]; 4]| {
            self.instances
                .push(Instance::from_matrix(mesh, model).with_material(tint));
        };

        // The body: hips carry the torso, torso carries the head.
        let body = Transform::from_position([0.0, HIP_HEIGHT + pose.bounce, 0.0])
            .with_rotation(pose.root.rotation);
        let body = body.then(&Transform::from_position(pose.root.position));

        let torso_local = Transform::from_position([0.0, TORSO_SIZE[1] * 0.5, 0.0]);
        push(meshes.torso, torso_local.then_matrix(body));

        let head_local = Transform::from_position([0.0, TORSO_SIZE[1] + HEAD_RADIUS * 0.9, 0.0]);
        push(meshes.head, head_local.then_matrix(body));

        // A limb segment: the shared capsule, hanging *down* from the joint it is
        // drawn inside. The capsule is authored one unit long, so the Y scale is
        // simply the segment's length and one mesh serves all eight segments.
        //
        // The scale is non-uniform, which is worth noticing rather than glossing:
        // it is exactly the case that makes the model matrix's 3x3 the wrong
        // transform for normals, and the reason the engine ships an
        // inverse-transpose. It also squashes the hemispherical caps into
        // ellipsoids — an accepted cosmetic cost at these proportions, and the
        // alternative is four near-identical limb meshes.
        let segment = |length: f32| {
            Transform::from_position([0.0, -length * 0.5, 0.0]).with_scale([1.0, length, 1.0])
        };

        for (side, &(shoulder, elbow)) in pose.arm.iter().enumerate() {
            let x = if side == 0 { -SHOULDER_X } else { SHOULDER_X };
            let joint = Transform::from_position([x, TORSO_SIZE[1] * 0.86, 0.0])
                .with_rotation([shoulder, 0.0, 0.0]);
            let upper = joint.then_matrix(body);
            push(meshes.limb, segment(UPPER_ARM).then_matrix(upper));

            // The elbow hangs at the end of the upper arm, and everything below it
            // inherits the shoulder's swing for free — that inheritance is the
            // whole reason to compose rather than to place each part in world
            // space.
            let elbow_joint =
                Transform::from_position([0.0, -UPPER_ARM, 0.0]).with_rotation([elbow, 0.0, 0.0]);
            let lower = elbow_joint.then_matrix(upper);
            push(meshes.limb, segment(FOREARM).then_matrix(lower));
        }

        for (side, &(hip, knee)) in pose.leg.iter().enumerate() {
            let x = if side == 0 { -HIP_X } else { HIP_X };
            let joint = Transform::from_position([x, 0.0, 0.0]).with_rotation([hip, 0.0, 0.0]);
            let thigh = joint.then_matrix(body);
            push(meshes.limb, segment(THIGH).then_matrix(thigh));

            let knee_joint =
                Transform::from_position([0.0, -THIGH, 0.0]).with_rotation([knee, 0.0, 0.0]);
            let shin = knee_joint.then_matrix(thigh);
            push(meshes.limb, segment(SHIN).then_matrix(shin));
        }
    }

    /// Rebuild the draw-list for an instant on the timeline.
    ///
    /// The per-frame cost of a moving scene: a handful of matrices per figure. No
    /// mesh is touched, which is why the HUD can honestly claim zero uploads.
    ///
    /// `time` is deliberately a *parameter* rather than `self.time`. The stage's
    /// pose is a pure function of time, so rendering between two fixed steps means
    /// evaluating that function at a sub-step instant — no snapshot blending
    /// required. See the call site in `update`.
    fn rebuild_instances(&mut self, time: f32) {
        let Some(meshes) = self.meshes else { return };
        self.instances.clear();

        // The ground: one plane, stretched to the size of the stage. Scale is part
        // of the transform, so widening the stage moves no geometry.
        let extent = self.stage_extent();
        self.instances.push(
            Instance::new(
                meshes.ground,
                Transform::IDENTITY.with_scale([extent, 1.0, extent]),
            )
            // Dark, so the figures carry the frame. Note these are *linear*
            // values written to an sRGB surface, so they read lighter on screen
            // than the numbers suggest.
            .with_material(Material::rgb(0.10, 0.11, 0.14)),
        );

        let n = self.side_count();
        let origin = -(n as f32 - 1.0) * 0.5 * SPACING;
        for row in 0..n {
            for col in 0..n {
                let i = (row * n + col) as u32;
                let rate = 0.7 + hash01(i * 3) * 0.9;
                let offset = hash01(i * 3 + 1) * std::f32::consts::TAU;

                // Each figure turns at its own rate. This is what the hierarchy
                // buys: the limb angles below are all about X, the facing is about
                // Y, and only a matrix composes the two correctly.
                let facing = if self.turn {
                    time * 0.6 * rate + offset
                } else {
                    0.0
                };

                let pose = Pose::walking(
                    [
                        origin + col as f32 * SPACING,
                        0.0,
                        origin + row as f32 * SPACING,
                    ],
                    facing,
                    time * self.tempo * rate + offset,
                    self.swing,
                );
                self.push_figure(meshes, &pose, tint_for(i));
            }
        }
    }

    /// Declare the UI. Returns whether the pointer is over a widget, so the orbit
    /// camera can leave the mouse alone while a slider is being dragged.
    fn build_ui(&mut self, renderer: &mut Renderer) -> bool {
        let theme = self.theme;
        let fps = self.fps;
        let figures = self.side_count() * self.side_count();
        let parts = self.instances.len().saturating_sub(1);
        let mut light = self.theme == Theme::light();

        // The transport state belongs to the *engine's* clock, so it is
        // snapshotted here and applied after the panel closes — the same shape
        // the `light` toggle above already uses, because `ui` borrows the
        // renderer for as long as the closures run.
        let mut paused = renderer.time().is_paused();
        let mut speed = renderer.time().scale();
        let mut scrub = self.time;
        let mut single_step = false;
        let mut seeked = false;
        let sim = self.time;
        let wall = self.wall;
        let steps = renderer.time().steps();

        let mut ui = renderer.ui();
        ui.set_theme(theme);

        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.title("Scene");
            ui.separator();
            ui.slider("figures per side", &mut self.side, SIDE_MIN, SIDE_MAX)
                .decimals(0)
                .show();
            ui.slider("tempo", &mut self.tempo, 0.0, 4.0).show();
            ui.slider("swing", &mut self.swing, 0.0, 1.4).show();
            ui.checkbox("turn on the spot", &mut self.turn);

            // Transport, composed from the button and slider the toolkit already
            // ships — no new widget. The UI roadmap predicted this slice would ask
            // for a scrubber and ruled that the crude version comes first; a
            // dedicated one waits until this is demonstrably not enough.
            ui.section("Time", |ui| {
                // `columns`, not `horizontal`: a button allocates "whatever is
                // left of the line", so two in a row means the first takes the
                // whole width and the second is pushed off the panel. Columns
                // divide the line up front.
                ui.columns(2, |ui, column| {
                    if column == 0 {
                        let label = if paused { "play" } else { "pause" };
                        if ui.button(label).show().clicked {
                            paused = !paused;
                        }
                    } else {
                        // Only meaningful while paused — stepping a running clock
                        // just takes a step it was about to take anyway.
                        single_step = ui.button("step").variant(Variant::Secondary).show().clicked;
                    }
                });
                ui.slider("speed", &mut speed, 0.1, 4.0).show();
                seeked = ui
                    .slider("scrub", &mut scrub, 0.0, SCRUB_MAX)
                    .decimals(1)
                    .show()
                    .changed;
            });
        });

        ui.panel(Anchor::TopRight, HUD_W, |ui| {
            ui.label_value("fps", &format!("{fps:.0}"));
            ui.label_value("meshes", "4");
            ui.label_value("figures", &format!("{figures}"));
            ui.label_value("parts", &format!("{parts}"));
            // Four meshes appear in the draw-list, so four instanced calls carry
            // however many parts there are.
            ui.label_value("draw calls", "4");
            ui.label_value("uploads/frame", "0");
            ui.separator();
            // The slice's proof, in two rows: whatever the frame rate does, these
            // two track each other. They diverge only when *you* make them —
            // pausing, scaling, or scrubbing.
            ui.label_value("sim time", &format!("{sim:.2}s"));
            ui.label_value("wall time", &format!("{wall:.2}s"));
            ui.label_value("steps", &format!("{steps}"));
            ui.checkbox("wireframe", &mut self.wireframe);
            ui.checkbox("light", &mut light);
        });

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        self.theme = if light { Theme::light() } else { Theme::dark() };

        renderer.time_mut().set_paused(paused);
        renderer.time_mut().set_scale(speed);
        if single_step {
            renderer.time_mut().step_once();
        }
        if seeked {
            // Both halves of a seek, and the honest cost of the engine's clock-only
            // rule: the demo moves its own state, then keeps the engine's clock in
            // agreement. It can do that at all only because its state is a pure
            // function of time — a consumer carrying irreversible state (eroded
            // terrain) would offer no scrub control.
            self.time = scrub;
            renderer.time_mut().seek(scrub);
        }

        wants_pointer
    }

    /// Orbit the stage: drag to turn, arrow keys likewise, scroll to zoom.
    fn drive_camera(&mut self, renderer: &mut Renderer, ui_has_pointer: bool) {
        // The panel has first claim on the pointer; the keyboard is never spoken
        // for here, because this demo has no text field to type into.
        let dragging = renderer.input().is_mouse_held(MouseButton::Left) && !ui_has_pointer;
        let dt = renderer.dt();
        self.orbit.drive(
            renderer.input(),
            dt,
            OrbitInput {
                drag: dragging,
                keys: true,
                zoom: !ui_has_pointer,
            },
        );

        // Aimed a little above the pivot so the figures sit centred rather than
        // the floor — the reason `Orbit` hands over an eye and stops.
        renderer
            .camera_mut()
            .look_from_to(self.orbit.eye(), [0.0, 0.9, 0.0]);
    }
}

impl Application for SceneDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        // The only geometry upload in the entire program — and not a vertex array
        // in sight. A limb is one capsule, stretched and posed per placement.
        self.meshes = Some(Meshes {
            ground: renderer.upload_mesh(&Mesh::plane([1.0, 1.0])),
            torso: renderer.upload_mesh(&Mesh::cuboid(TORSO_SIZE)),
            head: renderer.upload_mesh(&Mesh::sphere(HEAD_RADIUS, 18, 12)),
            // One unit long, so a segment's Y scale *is* its length.
            limb: renderer.upload_mesh(&Mesh::capsule(LIMB_RADIUS, 1.0, 12, 5)),
        });
    }

    /// The stage's entire simulation: one float, advanced by a step that is the
    /// same number on every machine.
    ///
    /// Nothing else in this file touches `self.time`, which is what the fixed hook
    /// is for — the contract is a *place*, not a convention. (The one exception is
    /// the scrub control, which sets it outright and tells the engine's clock so.)
    fn fixed_update(&mut self, _renderer: &mut Renderer, dt: f32) {
        self.time += dt;
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // Wall time, deliberately: an FPS readout that froze with the simulation
        // would be useless for judging whether pausing cost anything.
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }
        self.wall += dt;

        // Render *between* steps. At 144 Hz against a 60 Hz step most frames fall
        // mid-step, and drawing the last completed one on each of them judders;
        // `alpha` is how far through the pending step this frame is. The pose is a
        // pure function of time, so this evaluates it at a sub-step instant rather
        // than blending two stored snapshots — which is a consumer's choice to
        // make, not the engine's.
        let time = self.time + renderer.time().alpha() * renderer.time().step();

        // Built before the UI so the HUD's part count describes this frame.
        self.rebuild_instances(time);
        let ui_has_pointer = self.build_ui(renderer);
        self.drive_camera(renderer, ui_has_pointer);

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
