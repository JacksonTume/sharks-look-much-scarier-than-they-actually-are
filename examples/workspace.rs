//! The scene as **a panel among panels** — an application layout, not a
//! backdrop.
//!
//! Every demo before this one is the same shape: the 3D view fills the window
//! and the UI floats on top of it. That is the right shape for a terrain toy and
//! the wrong one for an application, where the rendered view is one pane in a
//! workspace — a viewport beside an inspector beside a list. This demo is the
//! inversion, and the roadblock it hit is the whole of engine Slice 18: there was
//! no way to say *where* the scene goes.
//!
//! What it exercises, in the order the bugs would have been found:
//!
//! - **[`Renderer::set_scene_rect`]** — the scene is inset into the space the
//!   panels leave. The rect is computed here, from public theme metrics, because
//!   deciding how a workspace is laid out is a consumer's job.
//! - **Aspect.** The pane is much wider than it is tall. The sphere in the middle
//!   is the check: if the engine were still deriving aspect from the window, it
//!   would be an ellipse.
//! - **Picking into an inset pane.** Click an object to select it. This is the
//!   latent bug the slice existed to find — [`Renderer::pointer_ray`] used to
//!   unproject through the whole window, which is indistinguishable from correct
//!   right up until the scene stops filling it.
//! - **The transition.** "fill window" turns the rect off and back on, so the
//!   `None` ↔ `Some` path and the texture re-allocation behind it both run at
//!   runtime rather than only at startup.
//!
//! **The pane is deliberately not centred vertically** — there is more room below
//! it than above. A vertically symmetric rect hides a flipped Y completely, and
//! the GL/WebGL2 path renders through an offscreen framebuffer with a flipping
//! blit, so that is a real thing to be able to see.
//!
//! Content-free on purpose, in the same way `scene.rs` and `editor.rs` are: some
//! shapes on a ground plane. The point is the layout.
//!
//! Run it:
//!   native — `cargo run --example workspace`
//!   web    — `cargo xtask serve workspace`
//!
//! Drag in the pane to orbit, scroll to zoom, click a shape to select it.

use std::f32::consts::TAU;

use slmsttaa::ui::{Anchor, Theme, Variant};
use slmsttaa::{
    run, Application, Instance, Material, Mesh, MeshHandle, MouseButton, Renderer, Transform,
};

/// Width of the left-hand control panel, in points.
const CONTROLS_W: f32 = 232.0;
/// Width of the right-hand readout panel, in points.
const INSPECTOR_W: f32 = 190.0;
/// Extra breathing room below the pane, on top of the theme's margin.
///
/// This is what makes the rect asymmetric top-to-bottom. It is not decoration:
/// see the module docs on why a centred pane cannot show a flipped Y.
const FOOTER_H: f32 = 44.0;

/// How many shapes the ring holds. Fixed — this demo is about the frame, not
/// about spawning.
const COUNT: usize = 7;

/// One shape in the ring.
#[derive(Clone, Copy)]
struct Prop {
    /// Which uploaded mesh it is a placement of.
    mesh: usize,
    /// Where it sits on the ring, in radians.
    phase: f32,
    /// Half-extent used for the ray test, and for the selection cage.
    radius: f32,
}

impl Prop {
    /// Where it is this frame. The ring turns, so this moves.
    fn position(&self, spin: f32) -> [f32; 3] {
        let angle = self.phase + spin;
        [angle.sin() * 2.6, self.radius, angle.cos() * 2.6]
    }
}

/// A workspace: two panels, a 3D pane between them, and a selection.
struct Workspace {
    /// Sphere, cuboid, capsule — uploaded once, placed many times.
    meshes: Vec<MeshHandle>,
    /// The ground the props stand on.
    ground: Option<MeshHandle>,
    /// A unit cuboid, reused as the twelve edges of a selection cage.
    edge: Option<MeshHandle>,
    props: Vec<Prop>,
    /// Index into `props`, or `None`.
    selected: Option<usize>,
    /// Ring rotation, advanced on the fixed clock.
    spin: f32,
    /// Whether the ring turns at all.
    turning: bool,
    /// When false the scene fills the window, exactly as every earlier demo does.
    inset: bool,
    /// Orbit camera state.
    yaw: f32,
    pitch: f32,
    distance: f32,
    theme: Theme,
    /// Whether the UI claimed the pointer this frame, so a drag on a panel does
    /// not also orbit the stage.
    ui_pointer: bool,
    /// Whether the drag in progress started inside the pane. A drag that begins
    /// on the scene keeps orbiting even when the cursor wanders onto a panel.
    orbiting: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            meshes: Vec::new(),
            ground: None,
            edge: None,
            props: Vec::new(),
            selected: None,
            spin: 0.0,
            turning: true,
            inset: true,
            yaw: 0.6,
            pitch: 0.42,
            distance: 8.5,
            theme: Theme::dark(),
            ui_pointer: false,
            orbiting: false,
        }
    }
}

impl Workspace {
    /// The rectangle the scene should occupy, in logical points.
    ///
    /// Plain arithmetic over the panel widths and the theme's own margin, and
    /// that is the correct place for it: the toolkit anchors panels to window
    /// corners and reserves nothing, so "what is left over" is a question about
    /// *this* layout rather than a fact the toolkit knows. A generic
    /// `Ui::remaining()` would have to invent a policy for overlapping panels and
    /// would inherit the bottom-anchor measurement lag; four lines here are exact.
    fn pane(&self, renderer: &Renderer) -> Option<[f32; 4]> {
        if !self.inset {
            return None;
        }
        // The *window*, not `scene_rect` — that one already is the pane, and
        // measuring the next rect from the last one shrinks it a little every
        // frame until it hits the one-pixel clamp. Which is what the first run of
        // this demo did, on screen, while every test passed.
        let [w, h] = renderer.window_size();
        let m = self.theme.space.margin;
        let x = m + CONTROLS_W + m;
        let right = m + INSPECTOR_W + m;
        Some([x, m, w - x - right, h - m - FOOTER_H])
    }

    /// Nearest prop the ray hits, as a sphere test against each.
    ///
    /// The engine hands over a ray and stops there (see [`Renderer::pointer_ray`]),
    /// so what counts as a hit is decided here — a sphere is forgiving enough for
    /// a capsule and a cuboid alike at this size.
    fn pick(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, prop) in self.props.iter().enumerate() {
            let c = prop.position(self.spin);
            let oc = [origin[0] - c[0], origin[1] - c[1], origin[2] - c[2]];
            let b = oc[0] * dir[0] + oc[1] * dir[1] + oc[2] * dir[2];
            let r = prop.radius * 1.25;
            let c_term = oc[0] * oc[0] + oc[1] * oc[1] + oc[2] * oc[2] - r * r;
            let disc = b * b - c_term;
            if disc < 0.0 {
                continue;
            }
            let t = -b - disc.sqrt();
            if t <= 0.0 {
                continue;
            }
            // `is_none_or` would read better, but it is newer than the MSRV
            // clippy enforces here.
            if best.map_or(true, |(bt, _)| t < bt) {
                best = Some((t, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Orbit and zoom, driven only by drags that belong to the scene.
    fn drive_camera(&mut self, renderer: &mut Renderer) {
        let input = renderer.input();
        let (mdx, mdy) = input.mouse_delta();
        let scroll = if self.ui_pointer {
            0.0
        } else {
            input.scroll_delta()
        };
        if self.orbiting {
            self.yaw -= mdx * 0.005;
            self.pitch = (self.pitch - mdy * 0.005).clamp(-1.2, 1.35);
        }
        self.distance = (self.distance - scroll * 0.5).clamp(3.5, 20.0);

        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let eye = [
            sy * cp * self.distance,
            sp * self.distance + 1.0,
            cy * cp * self.distance,
        ];
        renderer.camera_mut().look_from_to(eye, [0.0, 0.6, 0.0]);
    }

    /// Build this frame's draw-list: ground, props, and the selection cage.
    fn draw_list(&self) -> Vec<Instance> {
        let mut out = Vec::with_capacity(COUNT + 14);
        if let Some(ground) = self.ground {
            out.push(
                Instance::at(ground)
                    .with_material(Material::rgb(0.30, 0.33, 0.38).with_specular(0.12, 24.0)),
            );
        }
        for (i, prop) in self.props.iter().enumerate() {
            // A hue sweep, spelled out as three phase-shifted sines because the
            // engine has no colour helpers and this demo is not the place to add
            // one. Boring pastels on purpose: Slice 17 found the web's missing
            // sRGB encode precisely because a flat grey ground under pastel
            // objects made the difference obvious, where terrain never did.
            let hue = i as f32 / COUNT as f32;
            let band = |offset: f32| 0.45 + 0.4 * ((hue + offset) * TAU).sin().abs();
            let material =
                Material::rgb(band(0.0), band(0.33), band(0.66)).with_specular(0.35, 48.0);
            out.push(
                Instance::new(
                    self.meshes[prop.mesh],
                    Transform::from_position(prop.position(self.spin)).with_rotation([
                        0.0,
                        prop.phase + self.spin,
                        0.0,
                    ]),
                )
                .with_material(material),
            );
        }
        if let (Some(index), Some(edge)) = (self.selected, self.edge) {
            let prop = self.props[index];
            out.extend(self.cage(edge, prop.position(self.spin), prop.radius * 1.3));
        }
        out
    }

    /// The twelve edges of a box, drawn as thin cuboids.
    ///
    /// The same answer `editor.rs` reached, and for the same reason: a translucent
    /// shell washes the object pale, which is useless when what you are looking at
    /// *is* its colour. A cage reads as "selected" and leaves the object alone —
    /// and it is composed entirely from public API, which is the test this project
    /// applies before anything gets pushed into the engine.
    fn cage(&self, edge: MeshHandle, centre: [f32; 3], half: f32) -> Vec<Instance> {
        const THICK: f32 = 0.035;
        let material = Material::rgb(1.0, 0.86, 0.35).with_specular(0.0, 1.0);
        let mut out = Vec::with_capacity(12);
        let span = half * 2.0;
        // Four bars along each axis, offset to the four parallel edges.
        for axis in 0..3 {
            for corner in 0..4 {
                let mut offset = [0.0f32; 3];
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                offset[a] = if corner & 1 == 0 { -half } else { half };
                offset[b] = if corner & 2 == 0 { -half } else { half };
                let mut scale = [THICK; 3];
                scale[axis] = span;
                out.push(
                    Instance::new(
                        edge,
                        Transform::from_position([
                            centre[0] + offset[0],
                            centre[1] + offset[1],
                            centre[2] + offset[2],
                        ])
                        .with_scale(scale),
                    )
                    .with_material(material),
                );
            }
        }
        out
    }
}

impl Application for Workspace {
    fn init(&mut self, renderer: &mut Renderer) {
        self.meshes = vec![
            renderer.upload_mesh(&Mesh::sphere(0.5, 28, 20)),
            renderer.upload_mesh(&Mesh::cuboid([0.8; 3])),
            renderer.upload_mesh(&Mesh::capsule(0.32, 0.55, 20, 10)),
        ];
        self.ground = Some(renderer.upload_mesh(&Mesh::plane([14.0, 14.0])));
        self.edge = Some(renderer.upload_mesh(&Mesh::cuboid([1.0; 3])));
        self.props = (0..COUNT)
            .map(|i| Prop {
                mesh: i % 3,
                phase: i as f32 / COUNT as f32 * TAU,
                radius: [0.5, 0.55, 0.6][i % 3],
            })
            .collect();
    }

    fn fixed_update(&mut self, _renderer: &mut Renderer, dt: f32) {
        if self.turning {
            self.spin += dt * 0.25;
        }
    }

    fn update(&mut self, renderer: &mut Renderer) {
        // The rect goes in *first*, before anything reads a ray or a camera. It
        // is what the aspect ratio and the pointer mapping are derived from, so
        // setting it after picking would pick against last frame's layout.
        let rect = self.pane(renderer);
        renderer.set_scene_rect(rect);

        // Picking. A press that lands in the pane either selects a prop or starts
        // an orbit; one that lands on a panel does neither.
        //
        // Note the press is handled *before* asking whether the button is still
        // held — a browser can deliver mousedown and mouseup between two frames,
        // and doing it the other way round discards the entire click. That trap
        // is recorded in `ARCHITECTURE.md`; it cost Slice 17 a day.
        let input = renderer.input();
        let pressed = input.is_mouse_pressed(MouseButton::Left);
        let held = input.is_mouse_held(MouseButton::Left);
        // Gated on `pointer_in_scene` and *not* on `ui_pointer`, which is the
        // first thing a workspace layout does differently from a fullscreen one.
        //
        // `ui_pointer` is last frame's answer — it is only knowable once the
        // panels have been declared, which happens below. In a fullscreen demo
        // that staleness is invisible, because the cursor is over the UI for many
        // frames before a click lands. Here the panels and the scene are disjoint
        // rectangles, so the fresh question is the better one, and asking the
        // stale one swallowed the first click after every panel interaction —
        // which is what the first run of this demo did.
        //
        // `ui_pointer` still earns its keep for scroll (see `drive_camera`),
        // where "a widget is active" matters and a frame of lag does not.
        if pressed && renderer.pointer_in_scene() {
            if let Some(ray) = renderer.pointer_ray() {
                self.selected = self.pick(ray.origin, ray.direction);
                // A press on empty scene starts an orbit instead of selecting.
                self.orbiting = self.selected.is_none();
            }
        }
        if !held {
            self.orbiting = false;
        }

        self.drive_camera(renderer);

        let fps = 1.0 / renderer.dt().max(1e-4);
        let [px, py, pw, ph] = renderer.scene_rect();
        let selected = self.selected;
        // The checkbox reads the opposite of the field it drives, so the label
        // can say the thing the user wants rather than the thing the code stores.
        let mut fill_window = !self.inset;
        let mut turning = self.turning;
        let mut light = self.theme == Theme::light();
        let mut clear = false;

        let mut ui = renderer.ui();
        ui.set_theme(self.theme);

        ui.panel(Anchor::TopLeft, CONTROLS_W, |ui| {
            ui.title("Workspace");
            ui.section("Layout", |ui| {
                ui.checkbox("fill window", &mut fill_window);
                ui.label_muted("off = one pane among panels");
            });
            ui.section("Stage", |ui| {
                ui.checkbox("turning", &mut turning);
                clear = ui
                    .button("deselect")
                    .variant(Variant::Destructive)
                    .show()
                    .clicked;
            });
            ui.section("Theme", |ui| {
                ui.checkbox("light", &mut light);
            });
        });

        ui.panel(Anchor::TopRight, INSPECTOR_W, |ui| {
            ui.label_value("fps", &format!("{fps:.0}"));
            ui.separator();
            ui.label_muted("scene rect (pt)");
            ui.label_value("x, y", &format!("{px:.0}, {py:.0}"));
            ui.label_value("w, h", &format!("{pw:.0}, {ph:.0}"));
            ui.label_value("aspect", &format!("{:.2}", pw / ph.max(1.0)));
            ui.separator();
            ui.label_value(
                "selected",
                &match selected {
                    Some(i) => format!("#{i}"),
                    None => "none".to_string(),
                },
            );
        });

        self.ui_pointer = ui.wants_pointer();
        drop(ui);

        self.inset = !fill_window;
        self.turning = turning;
        self.theme = if light { Theme::light() } else { Theme::dark() };
        if clear {
            self.selected = None;
        }

        renderer.set_instances(&self.draw_list());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = run(Workspace::default()) {
        eprintln!("workspace example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let _ = run(Workspace::default());
}

#[cfg(target_arch = "wasm32")]
fn main() {}
