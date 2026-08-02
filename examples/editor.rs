//! A scene you can edit — the demo that asks the engine to let a *pointer* reach
//! the 3D world.
//!
//! Every demo before this one is a one-way street: the consumer computes a scene
//! and the engine draws it. Input only ever moved the camera, or moved a slider
//! that moved a number. Nothing has ever asked "what did I just click on?", and
//! the answer needs something the engine has never handed over — a world-space
//! ray through the cursor.
//!
//! Deliberately content-free, in the same way `scene.rs` is: no game, no tool
//! chain, nothing but "objects you can point at, pick up, change, and throw
//! away." What it exercises:
//!
//! - **Picking.** A click becomes a ray ([`Renderer::pointer_ray`]), the ray is
//!   tested against the objects *by this demo*, and the nearest hit wins. The
//!   engine supplies the ray and knows nothing about what it might hit — the same
//!   split that keeps the erosion solver in the terrain demo.
//! - **Direct manipulation.** Dragging a selected object intersects the same ray
//!   with a horizontal plane, so the object follows the pointer across the ground
//!   rather than tracking a screen-space delta that would drift as the camera
//!   turns.
//! - **Object lifetime.** Objects are spawned and deleted at runtime. They cost
//!   no mesh traffic to create or destroy, because an object *is* a placement of
//!   one of three shared meshes — which is exactly the property instancing was
//!   added for.
//! - **The UI editing the world rather than a parameter.** The inspector writes
//!   to the selected object's transform, so the panel and the pointer are two
//!   ways of doing the same thing.
//!
//! Run it:
//!   native — `cargo run --example editor`
//!   web    — `cargo xtask serve editor`
//!
//! Controls: **left-click** an object to select it, **left-drag** it to move it
//! across the ground, **left-drag empty space** (or right-drag anywhere) to
//! orbit, **scroll** to zoom.

use slmsttaa::ui::{Anchor, Theme, Variant};
use slmsttaa::{
    run, Application, Instance, Key, Material, Mesh, MeshHandle, MouseButton, Ray, RenderMode,
    Renderer, Transform,
};

/// Width of the inspector panel and the HUD, in logical points.
const PANEL_W: f32 = 250.0;
const HUD_W: f32 = 210.0;

/// How far the ground plane reaches, in world units.
const GROUND: f32 = 14.0;

/// The three shapes an object can be. Each is one uploaded mesh, shared by every
/// object wearing it — swapping an object's shape names a different handle and
/// uploads nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    Box,
    Ball,
    Pill,
}

impl Shape {
    /// Every shape, for the spawn row and for cycling in the inspector.
    const ALL: [Shape; 3] = [Shape::Box, Shape::Ball, Shape::Pill];

    /// The label this shape wears in the UI. Short on purpose: the spawn row
    /// divides the panel into three, and a third of 250 points is not many
    /// glyphs (the lesson UI Slice 3 recorded and paid for).
    fn label(self) -> &'static str {
        match self {
            Shape::Box => "box",
            Shape::Ball => "ball",
            Shape::Pill => "pill",
        }
    }

    /// Half the shape's bounding box in its own space, before the object's scale.
    ///
    /// This is the demo's model of its own geometry, and it is *not* something the
    /// engine could supply — a bounding box is a decision about how forgiving
    /// clicking should be, not a fact about a mesh. A capsule's box is loose
    /// around its waist and exactly right at its caps, which is the trade that
    /// makes a thin pill easy to hit.
    fn half_extents(self) -> [f32; 3] {
        match self {
            Shape::Box => [BOX_SIZE * 0.5; 3],
            Shape::Ball => [BALL_RADIUS; 3],
            Shape::Pill => [PILL_RADIUS, PILL_LENGTH * 0.5 + PILL_RADIUS, PILL_RADIUS],
        }
    }
}

// --- Shape proportions, in world units and object space ----------------------

const BOX_SIZE: f32 = 0.9;
const BALL_RADIUS: f32 = 0.5;
const PILL_RADIUS: f32 = 0.32;
const PILL_LENGTH: f32 = 0.8;

/// The three uploaded meshes, plus the ground.
#[derive(Clone, Copy)]
struct Meshes {
    ground: MeshHandle,
    shapes: [MeshHandle; 3],
}

impl Meshes {
    fn of(&self, shape: Shape) -> MeshHandle {
        self.shapes[shape as usize]
    }
}

/// One thing on the stage.
///
/// Note what is *not* here: a mesh, a buffer, or anything the GPU has heard of.
/// An object is a shape name, a placement, and a colour — which is why spawning
/// and deleting them costs nothing.
#[derive(Clone, Copy)]
struct Object {
    shape: Shape,
    position: [f32; 3],
    /// Rotation about Y only. Enough to make the box's corners visibly turn, and
    /// the reason the hit test below can be a rotated slab test rather than a
    /// matrix inverse the demo has no math library to compute.
    yaw: f32,
    scale: [f32; 3],
    /// Position on the hue circle, in `[0, 1)`.
    hue: f32,
}

impl Object {
    /// Half the object's world-space bounding box, along its *own* axes.
    fn half_extents(&self) -> [f32; 3] {
        let base = self.shape.half_extents();
        [
            base[0] * self.scale[0],
            base[1] * self.scale[1],
            base[2] * self.scale[2],
        ]
    }

    /// The lowest point of the object, so it can be stood on the ground.
    fn resting_y(&self) -> f32 {
        self.half_extents()[1]
    }

    fn transform(&self) -> Transform {
        Transform::from_position(self.position)
            .with_rotation([0.0, self.yaw, 0.0])
            .with_scale(self.scale)
    }

    fn material(&self) -> Material {
        let [r, g, b] = hue_rgb(self.hue);
        Material::rgb(0.30 + r * 0.62, 0.30 + g * 0.62, 0.30 + b * 0.62)
    }

    /// Where along `ray` this object is hit, if it is hit at all.
    ///
    /// **This is the half the engine does not do.** It hands over a ray; what
    /// counts as a hit is the consumer's model of its own scene (roadmap
    /// principle 3 — the same ruling that keeps stream-power erosion in the
    /// terrain demo). Here it is a slab test against an axis-aligned box in the
    /// object's own frame.
    ///
    /// Rotating the ray into that frame is six lines *because* objects only turn
    /// about Y: a general rotation would need the inverse of the model matrix,
    /// which is exactly the math dependency this API exists to avoid. A demo that
    /// wanted tumbling objects would have to ask a different question.
    fn hit(&self, ray: &Ray) -> Option<f32> {
        let (sin, cos) = self.yaw.sin_cos();
        let rel = [
            ray.origin[0] - self.position[0],
            ray.origin[1] - self.position[1],
            ray.origin[2] - self.position[2],
        ];
        // World → object: undo the yaw.
        let origin = [
            cos * rel[0] - sin * rel[2],
            rel[1],
            sin * rel[0] + cos * rel[2],
        ];
        let dir = [
            cos * ray.direction[0] - sin * ray.direction[2],
            ray.direction[1],
            sin * ray.direction[0] + cos * ray.direction[2],
        ];

        let half = self.half_extents();
        let mut near = f32::NEG_INFINITY;
        let mut far = f32::INFINITY;
        for axis in 0..3 {
            if dir[axis].abs() < 1e-6 {
                // Parallel to this pair of faces: a miss unless already between
                // them, and no constraint on `t` if it is.
                if origin[axis].abs() > half[axis] {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / dir[axis];
            let mut t0 = (-half[axis] - origin[axis]) * inv;
            let mut t1 = (half[axis] - origin[axis]) * inv;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            near = near.max(t0);
            far = far.min(t1);
            if near > far {
                return None;
            }
        }
        // `far < 0` means the whole box is behind the eye.
        (far >= 0.0).then(|| near.max(0.0))
    }
}

/// A fully saturated RGB for a hue in `[0, 1)` — the six-sector HSV ramp with
/// saturation and value pinned at 1.
fn hue_rgb(hue: f32) -> [f32; 3] {
    let sector = hue.rem_euclid(1.0) * 6.0;
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

/// What the left button is currently doing.
///
/// Decided on the *press* and held until release, which is what stops a drag from
/// changing its mind: once you have grabbed an object, moving the pointer off it
/// keeps moving the object rather than starting to orbit.
#[derive(Clone, Copy, PartialEq)]
enum Grab {
    None,
    /// Turning the camera — the gesture that starts on empty space.
    Orbit,
    /// Sliding an object across a horizontal plane. `offset` is where on the
    /// object the pointer landed, so it doesn't snap to the object's centre; the
    /// plane is the height it was grabbed at.
    Move {
        offset: [f32; 3],
        plane_y: f32,
    },
}

/// A stage of objects, and a pointer that can reach them.
struct EditorDemo {
    meshes: Option<Meshes>,
    objects: Vec<Object>,
    /// Index into `objects`, or none. An index (rather than a handle) is honest
    /// about what it is: deleting an object below the selection would shift it,
    /// which is why deletion clears the selection outright.
    selected: Option<usize>,
    grab: Grab,

    /// This frame's draw-list, rebuilt in place so the allocation is reused.
    instances: Vec<Instance>,

    /// Whether the UI took the pointer *last* frame.
    ///
    /// Picking has to happen before the panel is declared — otherwise the
    /// inspector shows the previous selection for a frame — but whether the
    /// pointer is over a widget is only known once the panel *has* been declared.
    /// One frame of lag on that flag is invisible; one frame of lag on the whole
    /// inspector is not.
    ui_pointer: bool,

    /// Orbit camera state (azimuth, elevation, range).
    yaw: f32,
    pitch: f32,
    distance: f32,

    wireframe: bool,
    theme: Theme,
    fps: f32,
}

impl Default for EditorDemo {
    fn default() -> Self {
        Self {
            meshes: None,
            objects: starting_scene(),
            selected: None,
            grab: Grab::None,
            instances: Vec::new(),
            ui_pointer: false,
            yaw: 0.7,
            pitch: 0.45,
            distance: 11.0,
            wireframe: false,
            theme: Theme::dark(),
            fps: 60.0,
        }
    }
}

/// A handful of objects to point at, so the demo opens with something to click
/// rather than an empty plane and a hidden verb.
fn starting_scene() -> Vec<Object> {
    let mut objects = Vec::new();
    for (i, shape) in [
        Shape::Box,
        Shape::Ball,
        Shape::Pill,
        Shape::Box,
        Shape::Ball,
        Shape::Pill,
    ]
    .into_iter()
    .enumerate()
    {
        let angle = i as f32 * std::f32::consts::TAU / 6.0;
        let mut object = Object {
            shape,
            position: [angle.cos() * 2.6, 0.0, angle.sin() * 2.6],
            yaw: angle,
            scale: [1.0; 3],
            // Wrapped, not just stepped: `hue_rgb` takes any real number, but
            // the inspector's slider is a 0..1 range and would show 1.85.
            hue: (i as f32 * 0.618_034).fract(),
        };
        object.position[1] = object.resting_y();
        objects.push(object);
    }
    objects
}

impl EditorDemo {
    /// The nearest object the ray hits.
    fn pick(&self, ray: &Ray) -> Option<usize> {
        self.objects
            .iter()
            .enumerate()
            .filter_map(|(i, object)| object.hit(ray).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Where `ray` crosses the horizontal plane at height `y`.
    ///
    /// The other half of direct manipulation: a screen-space delta would slide the
    /// object at a rate that depends on how far away it is and how the camera is
    /// turned, so it drifts out from under the pointer. Meeting the pointer's ray
    /// on a plane keeps the grabbed point of the object exactly where the cursor
    /// is, at any angle and any zoom.
    fn ray_on_plane(ray: &Ray, y: f32) -> Option<[f32; 3]> {
        // Near-parallel to the plane: the crossing is somewhere off at infinity
        // and any answer would be garbage.
        if ray.direction[1].abs() < 1e-4 {
            return None;
        }
        let t = (y - ray.origin[1]) / ray.direction[1];
        (t > 0.0).then(|| {
            [
                ray.origin[0] + ray.direction[0] * t,
                y,
                ray.origin[2] + ray.direction[2] * t,
            ]
        })
    }

    /// Select, grab, drag, release.
    ///
    /// **The press is handled before the "still held?" check, and that ordering
    /// is load-bearing.** The obvious shape — bail out unless the button is down,
    /// then look for a press edge — reads correctly and silently drops any click
    /// whose press *and* release land inside a single frame. There is always a
    /// held frame when a human clicks at 75 fps, so this worked perfectly on
    /// native and did nothing at all in a browser, where a click can be delivered
    /// as two events between one frame and the next. A press edge means the
    /// button went down; whether it is still down is a separate question, and
    /// only the drag below needs to ask it.
    fn handle_pointer(&mut self, renderer: &Renderer) {
        let input = renderer.input();
        let held = input.is_mouse_held(MouseButton::Left);
        let pressed = input.is_mouse_pressed(MouseButton::Left) && !self.ui_pointer;

        let Some(ray) = renderer.pointer_ray() else {
            if !held {
                self.grab = Grab::None;
            }
            return;
        };

        if pressed {
            self.grab = match self.pick(&ray) {
                Some(index) => {
                    self.selected = Some(index);
                    let object = self.objects[index];
                    let plane_y = object.position[1];
                    // Where on the object the pointer actually landed. Grabbing a
                    // box by its corner and having it jump so its centre is under
                    // the cursor is the single most obvious way to make direct
                    // manipulation feel wrong.
                    let offset = match Self::ray_on_plane(&ray, plane_y) {
                        Some(p) => [object.position[0] - p[0], 0.0, object.position[2] - p[2]],
                        None => [0.0; 3],
                    };
                    Grab::Move { offset, plane_y }
                }
                None => {
                    self.selected = None;
                    Grab::Orbit
                }
            };
        }

        // Released already — a click that came and went inside this frame. The
        // selection it made above stands; there is nothing left to drag.
        if !held {
            self.grab = Grab::None;
            return;
        }

        if let (Grab::Move { offset, plane_y }, Some(index)) = (self.grab, self.selected) {
            if let Some(p) = Self::ray_on_plane(&ray, plane_y) {
                let object = &mut self.objects[index];
                let limit = GROUND * 0.5;
                object.position[0] = (p[0] + offset[0]).clamp(-limit, limit);
                object.position[2] = (p[2] + offset[2]).clamp(-limit, limit);
            }
        }
    }

    /// Rebuild the draw-list: the ground, every object, and the selection.
    ///
    /// Objects are emitted **grouped by shape** so that repeats of one mesh are
    /// contiguous — that is what lets the engine batch them into one instanced
    /// draw call each, and it is the whole reason a scene of thirty objects costs
    /// four calls rather than thirty.
    fn rebuild_instances(&mut self) {
        let Some(meshes) = self.meshes else { return };
        self.instances.clear();

        self.instances.push(
            Instance::new(
                meshes.ground,
                Transform::IDENTITY.with_scale([GROUND, 1.0, GROUND]),
            )
            .with_material(Material::rgb(0.10, 0.11, 0.14)),
        );

        for shape in Shape::ALL {
            let handle = meshes.of(shape);
            for object in self.objects.iter().filter(|o| o.shape == shape) {
                self.instances.push(
                    Instance::new(handle, object.transform()).with_material(object.material()),
                );
            }
        }

        if let Some(object) = self.selected.and_then(|i| self.objects.get(i)).copied() {
            self.push_selection_cage(meshes, &object);
        }
    }

    /// Mark the selected object with a wireframe cage around its bounds: twelve
    /// thin boxes, one per edge.
    ///
    /// **The first thing tried was a translucent shell** — the same mesh drawn a
    /// little larger in white — and it was rejected on sight. It washes the whole
    /// object pale, which destroys exactly the property the inspector is there to
    /// edit: a hue slider is useless when selecting the object drains its colour.
    /// A real outline needs to draw the silhouette *only*, and the engine has no
    /// way to say that — no per-instance render mode, no front-face culling, no
    /// stencil.
    ///
    /// It also does not need one. A cage assembled from the cuboid this demo has
    /// already uploaded says "selected" unambiguously, leaves the object's own
    /// colour alone, and is re-implementable by any consumer from public API —
    /// which is the test the roadmap sets before anything is pushed into the
    /// engine. Twelve extra instances of one mesh cost one draw call.
    fn push_selection_cage(&mut self, meshes: Meshes, object: &Object) {
        // The object's frame: placed and turned, but *not* scaled — the extents
        // below already carry the scale, and letting it through twice would
        // square it.
        let frame = Transform::from_position(object.position).with_rotation([0.0, object.yaw, 0.0]);
        let h = object.half_extents();
        let margin = CAGE_MARGIN;
        let e = [h[0] + margin, h[1] + margin, h[2] + margin];
        let bar = CAGE_THICKNESS;

        // A cuboid mesh is `BOX_SIZE` across, so a scale of `l / BOX_SIZE` makes
        // an edge `l` long.
        let s = |l: f32| l / BOX_SIZE;
        for (axis, sign_a, sign_b) in EDGES {
            let mut position = [0.0f32; 3];
            let mut scale = [s(bar); 3];
            // The two axes this edge is *offset* along, and the one it runs down.
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            position[a] = e[a] * sign_a;
            position[b] = e[b] * sign_b;
            scale[axis] = s(2.0 * e[axis] + bar);
            self.instances.push(
                Instance::from_matrix(
                    meshes.of(Shape::Box),
                    Transform::from_position(position)
                        .with_scale(scale)
                        .then(&frame),
                )
                .with_material(Material::rgb(1.4, 1.5, 1.7)),
            );
        }
    }

    /// Declare the UI. Returns whether the pointer is over a widget.
    fn build_ui(&mut self, renderer: &mut Renderer) -> bool {
        let theme = self.theme;
        let fps = self.fps;
        let count = self.objects.len();
        let selected = self.selected;
        let mut light = self.theme == Theme::light();

        // Actions the panel decides on but that need `self.objects` mutably —
        // taken after the closures release the renderer's borrow.
        let mut spawn: Option<Shape> = None;
        let mut duplicate = false;
        let mut delete = false;
        let mut clear = false;

        let mut ui = renderer.ui();
        ui.set_theme(theme);

        ui.panel(Anchor::TopLeft, PANEL_W, |ui| {
            ui.title("Editor");
            ui.separator();

            ui.section("Add", |ui| {
                // `columns`, not `horizontal`: a button allocates whatever is left
                // of the line, so three in a row would be one button and two
                // clipped stubs.
                ui.columns(3, |ui, column| {
                    let shape = Shape::ALL[column];
                    if ui
                        .button(shape.label())
                        .variant(Variant::Secondary)
                        .show()
                        .clicked
                    {
                        spawn = Some(shape);
                    }
                });
            });

            match selected {
                Some(index) => {
                    ui.section("Selected", |ui| {
                        let object = &mut self.objects[index];
                        let limit = GROUND * 0.5;
                        ui.slider("x", &mut object.position[0], -limit, limit)
                            .decimals(2)
                            .show();
                        ui.slider("z", &mut object.position[2], -limit, limit)
                            .decimals(2)
                            .show();
                        ui.slider("yaw", &mut object.yaw, 0.0, std::f32::consts::TAU)
                            .decimals(2)
                            .show();
                        ui.slider("hue", &mut object.hue, 0.0, 1.0).show();

                        ui.label("scale");
                        ui.indent(|ui| {
                            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                                ui.slider(name, &mut object.scale[axis], 0.25, 3.0)
                                    .decimals(2)
                                    .show();
                            }
                        });
                        // Whatever the sliders did, the object stays on the
                        // ground: scaling something taller should grow it upward,
                        // not sink half of it.
                        object.position[1] = object.resting_y();

                        ui.columns(2, |ui, column| {
                            if column == 0 {
                                duplicate = ui.button("copy").show().clicked;
                            } else {
                                delete = ui
                                    .button("delete")
                                    .variant(Variant::Destructive)
                                    .show()
                                    .clicked;
                            }
                        });
                    });
                }
                None => {
                    ui.section("Selected", |ui| {
                        // ASCII only: the atlas is baked from a fixed charset, so
                        // an em dash here draws as a missing-glyph box. Found by
                        // running it, which is the only way this is ever found.
                        ui.label("nothing - click an object");
                    });
                }
            }

            ui.section("Scene", |ui| {
                clear = ui
                    .button("clear all")
                    .variant(Variant::Destructive)
                    .show()
                    .clicked;
            });
        });

        ui.panel(Anchor::TopRight, HUD_W, |ui| {
            ui.label_value("fps", &format!("{fps:.0}"));
            ui.label_value("objects", &format!("{count}"));
            ui.label_value("meshes", "4");
            ui.label_value(
                "selected",
                &match selected {
                    Some(i) => format!("#{i}"),
                    None => "none".to_string(),
                },
            );
            ui.separator();
            ui.separator();
            ui.checkbox("wireframe", &mut self.wireframe);
            ui.checkbox("light", &mut light);
        });

        let wants_pointer = ui.wants_pointer();
        drop(ui);

        self.theme = if light { Theme::light() } else { Theme::dark() };

        if let Some(shape) = spawn {
            self.spawn(shape);
        }
        if duplicate {
            if let Some(index) = self.selected {
                let mut copy = self.objects[index];
                copy.position[0] += 0.9;
                copy.position[2] += 0.9;
                copy.hue = (copy.hue + 0.14).fract();
                self.objects.push(copy);
                self.selected = Some(self.objects.len() - 1);
            }
        }
        if delete {
            if let Some(index) = self.selected {
                self.objects.remove(index);
                // The selection is an index into a list that just shifted, so it
                // is dropped rather than guessed at.
                self.selected = None;
            }
        }
        if clear {
            self.objects.clear();
            self.selected = None;
        }

        wants_pointer
    }

    /// Add an object of `shape`, placed in front of the camera so it lands where
    /// the user is looking rather than behind them.
    fn spawn(&mut self, shape: Shape) {
        let (sin, cos) = self.yaw.sin_cos();
        let mut object = Object {
            shape,
            position: [sin * 2.0, 0.0, cos * 2.0],
            yaw: 0.0,
            scale: [1.0; 3],
            hue: (self.objects.len() as f32 * 0.618_034).fract(),
        };
        object.position[1] = object.resting_y();
        self.objects.push(object);
        self.selected = Some(self.objects.len() - 1);
    }

    /// Orbit the stage. Unlike `scene.rs`, the left button is spoken for — it
    /// belongs to the objects — so an orbit drag is one that *started* on empty
    /// space, or any drag of the right button.
    fn drive_camera(&mut self, renderer: &mut Renderer) {
        let input = renderer.input();
        let orbiting = self.grab == Grab::Orbit || input.is_mouse_held(MouseButton::Right);
        let (mdx, mdy) = input.mouse_delta();
        let scroll = if self.ui_pointer {
            0.0
        } else {
            input.scroll_delta()
        };
        let left = input.is_key_held(Key::Left);
        let right = input.is_key_held(Key::Right);
        let up = input.is_key_held(Key::Up);
        let down = input.is_key_held(Key::Down);

        if orbiting {
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

        self.pitch = self.pitch.clamp(0.08, 1.4);
        self.distance = self.distance.clamp(3.0, 34.0);

        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let eye = [
            self.distance * cp * sy,
            self.distance * sp,
            self.distance * cp * cy,
        ];
        renderer.camera_mut().look_from_to(eye, [0.0, 0.5, 0.0]);
    }
}

/// How far the selection cage stands off the object's bounds, and how thick its
/// bars are, in world units.
const CAGE_MARGIN: f32 = 0.05;
const CAGE_THICKNESS: f32 = 0.03;

/// The twelve edges of a box, as `(axis the edge runs along, sign on the next
/// axis, sign on the one after)`. Four edges per axis, which is the whole cage.
const EDGES: [(usize, f32, f32); 12] = [
    (0, -1.0, -1.0),
    (0, -1.0, 1.0),
    (0, 1.0, -1.0),
    (0, 1.0, 1.0),
    (1, -1.0, -1.0),
    (1, -1.0, 1.0),
    (1, 1.0, -1.0),
    (1, 1.0, 1.0),
    (2, -1.0, -1.0),
    (2, -1.0, 1.0),
    (2, 1.0, -1.0),
    (2, 1.0, 1.0),
];

impl Application for EditorDemo {
    fn init(&mut self, renderer: &mut Renderer) {
        self.meshes = Some(Meshes {
            ground: renderer.upload_mesh(&Mesh::plane([1.0, 1.0])),
            shapes: [
                renderer.upload_mesh(&Mesh::cuboid([BOX_SIZE; 3])),
                renderer.upload_mesh(&Mesh::sphere(BALL_RADIUS, 20, 14)),
                renderer.upload_mesh(&Mesh::capsule(PILL_RADIUS, PILL_LENGTH, 16, 6)),
            ],
        });
    }

    fn update(&mut self, renderer: &mut Renderer) {
        let dt = renderer.dt();
        if dt > 0.0 {
            self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
        }

        // Pointer first, so the inspector below describes the object that is
        // selected *now* rather than the one that was selected a frame ago.
        self.handle_pointer(renderer);
        self.rebuild_instances();
        self.ui_pointer = self.build_ui(renderer);
        self.drive_camera(renderer);

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
    if let Err(err) = run(EditorDemo::default()) {
        eprintln!("editor example exited with an error: {err}");
        std::process::exit(1);
    }
}

/// WASM entry point. `wasm-bindgen` calls this once the module is instantiated.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    let _ = run(EditorDemo::default());
}

// A bin example still needs a `main` to compile for the wasm target; the real
// entry point there is `start` above.
#[cfg(target_arch = "wasm32")]
fn main() {}
