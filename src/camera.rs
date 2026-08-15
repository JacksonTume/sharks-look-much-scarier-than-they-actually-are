//! A minimal perspective camera.
//!
//! The camera owns its position and orientation (as a look-at target) and knows
//! how to produce a combined view-projection matrix suitable for uploading to a
//! shader as a uniform.

use glam::{Mat4, Vec3};

/// wgpu's normalized device coordinates put Z in `[0, 1]`, whereas the OpenGL
/// convention glam targets uses `[-1, 1]`. This matrix remaps the depth range so
/// our projection matches the backend.
#[rustfmt::skip]
const OPENGL_TO_WGPU_MATRIX: Mat4 = Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
]);

/// A half-line in world space: where it starts, and which way it goes.
///
/// The engine's answer to "what am I pointing at?" — and deliberately only half
/// an answer. Producing a ray needs the camera and the size of the render
/// target, which are the engine's; deciding what the ray *hits* needs a model of
/// the scene, which is the consumer's. So the engine hands over the ray and
/// stops, the same way it hands over a heightmap's worth of pixels and lets the
/// terrain demo own the erosion.
///
/// Plain arrays, like [`Transform`](crate::Transform) and
/// [`Camera::look_from_to`]: a consumer intersects this with its own boxes and
/// planes without taking a math dependency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where the ray starts, in world space — a point on the near plane.
    pub origin: [f32; 3],
    /// Unit direction the ray travels.
    pub direction: [f32; 3],
}

impl Ray {
    /// The point `distance` units along the ray.
    pub fn at(&self, distance: f32) -> [f32; 3] {
        [
            self.origin[0] + self.direction[0] * distance,
            self.origin[1] + self.direction[1] * distance,
            self.origin[2] + self.direction[2] * distance,
        ]
    }
}

/// A perspective camera positioned somewhere in the world, looking at a target.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// World-space position of the eye.
    pub eye: Vec3,
    /// World-space point the camera is aimed at.
    pub target: Vec3,
    /// Which way is up (usually `Vec3::Y`).
    pub up: Vec3,
    /// Width / height of the render target.
    pub aspect: f32,
    /// Vertical field of view, in radians.
    pub fov_y: f32,
    /// Near clip plane distance.
    pub z_near: f32,
    /// Far clip plane distance.
    pub z_far: f32,
}

impl Camera {
    /// Create a camera with sensible defaults for the given aspect ratio.
    pub fn new(aspect: f32) -> Self {
        Self {
            eye: Vec3::new(0.0, 1.0, 3.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect,
            fov_y: 45.0_f32.to_radians(),
            z_near: 0.1,
            z_far: 100.0,
        }
    }

    /// The combined view-projection matrix, corrected for wgpu's clip space.
    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fov_y, self.aspect, self.z_near, self.z_far);
        OPENGL_TO_WGPU_MATRIX * proj * view
    }

    /// Aim the camera: place the eye at `eye`, looking at `target`.
    ///
    /// A convenience for consumers driving the viewpoint (e.g. an orbit camera)
    /// that takes plain `[x, y, z]` arrays, so a demo never has to depend on
    /// `glam` just to move the camera — matching how [`Vertex`](crate::Vertex)
    /// stays array-based.
    pub fn look_from_to(&mut self, eye: [f32; 3], target: [f32; 3]) {
        self.eye = Vec3::from(eye);
        self.target = Vec3::from(target);
    }

    /// The world-space ray through a point in normalized device coordinates:
    /// `x` and `y` both in `[-1, 1]`, with `+y` **up**.
    ///
    /// Unprojects the near and far plane points and joins them, which is the
    /// formulation that stays correct if this camera ever stops being a
    /// perspective one — the eye position is not a valid ray origin under an
    /// orthographic projection, and the near-plane point always is.
    ///
    /// Engine-internal because nothing has asked to cast a ray through anywhere
    /// but the pointer. [`Renderer::pointer_ray`](crate::Renderer::pointer_ray)
    /// is the public door, and it owns the pixels-to-NDC conversion because it is
    /// the half that knows how big the render target is.
    pub(crate) fn ray_through_ndc(&self, ndc: [f32; 2]) -> Ray {
        let inverse = self.view_projection().inverse();
        // wgpu's clip space puts the near plane at z = 0 and the far at z = 1.
        let unproject = |z: f32| {
            let clip = inverse * glam::Vec4::new(ndc[0], ndc[1], z, 1.0);
            clip.truncate() / clip.w
        };
        let near = unproject(0.0);
        let direction = (unproject(1.0) - near).normalize_or_zero();
        Ray {
            origin: near.to_array(),
            direction: direction.to_array(),
        }
    }

    /// Update the aspect ratio, e.g. after a window resize.
    pub fn set_aspect(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect = width as f32 / height as f32;
        }
    }
}

/// Which inputs an [`Orbit`] should read on a given frame.
///
/// Three flags rather than one, because the three are gated by genuinely
/// different things and every demo in this repo gates them differently. The
/// pointer belongs to the UI when it is over a panel; the *keyboard* belongs to
/// the UI only while a text field has focus; and which button orbits is a policy
/// the consumer owns — `editor.rs` reserves the left button for picking objects
/// and orbits on the right, where `scene.rs` orbits on the left.
///
/// Deciding all of that is the consumer's job (principle 3). This type is just
/// how the answer is handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrbitInput {
    /// Whether a mouse drag should turn the camera this frame. The consumer has
    /// already decided *which* button, and whether the drag belongs to the scene
    /// rather than to a panel.
    pub drag: bool,
    /// Whether the arrow keys should turn the camera this frame. False while a
    /// text field has the keyboard, or the camera flies as you type.
    pub keys: bool,
    /// Whether the wheel should zoom this frame. False while the pointer is over
    /// a scroll area, or one notch does both jobs.
    pub zoom: bool,
}

impl OrbitInput {
    /// Read everything — the right answer for a demo with no UI over its scene.
    pub const ALL: Self = Self {
        drag: true,
        keys: true,
        zoom: true,
    };
    /// Read nothing. The camera still clamps and still reports an eye, so a
    /// consumer can freeze the viewpoint without special-casing its draw code.
    pub const NONE: Self = Self {
        drag: false,
        keys: false,
        zoom: false,
    };
}

/// A viewpoint that circles a point in the world: an azimuth, an elevation and a
/// distance, driven from [`Input`](crate::Input).
///
/// # Why this is in the engine
///
/// It was **not**, for nineteen slices. Slice 3 put the orbit math in `grid.rs`
/// and said so explicitly — "a single consumer doesn't justify one yet" — which
/// was right at the time and stopped being right without anyone noticing. Six
/// demos ended up with the same spherical-coordinate block, the same two magic
/// constants, and the same four `is_key_held` calls; the duplication was found by
/// fixing a frame-rate bug in it **six times in a row**.
///
/// What is here is only the arithmetic every one of them agreed on. What stays in
/// the consumer is every part they disagreed on: which button orbits, whether the
/// UI has first claim on the pointer, where the camera *aims* (all six orbit a
/// pivot and look slightly above it, at six different heights), and how the
/// limits scale — `terrain.rs` quotes every distance against its map span,
/// because a cell stopped being a fixed amount of ground when the continent grew.
///
/// # It is unprivileged
///
/// Every line of `drive` is written against the same public API a demo has:
/// [`Input::is_mouse_held`](crate::Input::is_mouse_held),
/// [`Input::mouse_delta`](crate::Input::mouse_delta),
/// [`Input::scroll_delta`](crate::Input::scroll_delta) and
/// [`Input::is_key_held`](crate::Input::is_key_held). Nothing here reaches for
/// anything a consumer could not, which is the rule the UI toolkit already holds
/// its widgets to, applied to the engine.
///
/// # Aiming is not orbiting
///
/// [`Orbit::eye`] gives a position and stops there. It does **not** aim the
/// camera, because the point a camera orbits and the point it looks at are
/// routinely different — every demo here circles the origin and aims a little way
/// up it, so that the subject sits centred rather than the horizon. So:
///
/// ```no_run
/// # use slmsttaa::{Camera, Orbit, OrbitInput, Input};
/// # let (mut camera, mut orbit, input, dt) = (Camera::new(1.0), Orbit::new(0.7, 0.6, 6.0), Input::default(), 0.016);
/// orbit.drive(&input, dt, OrbitInput::ALL);
/// camera.look_from_to(orbit.eye(), [0.0, 0.9, 0.0]);
/// ```
///
/// # On frame-rate independence
///
/// The keys are a **rate** (radians a second) and the drag is a **ratio**
/// (radians a pixel), and the difference is the whole reason this type is worth
/// having in one place. A held key is a duration, so it scales by `dt`; a mouse
/// delta is *already* this frame's motion, so scaling it by `dt` would make the
/// same mistake in the opposite direction. Both were got wrong in the demos —
/// the keys advanced by a flat step per frame, so an orbit ran twice as fast at
/// 144 Hz as at 72 Hz — and that is now asserted rather than commented.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// The point the eye circles. Usually the origin; not usually the point the
    /// camera is aimed at — see the type docs.
    pub pivot: [f32; 3],
    /// Azimuth around the pivot, in radians.
    pub yaw: f32,
    /// Elevation above the pivot's horizontal plane, in radians.
    pub pitch: f32,
    /// Distance from the pivot to the eye, in world units.
    pub distance: f32,
    /// Inclusive `(min, max)` the pitch is held inside, in radians.
    ///
    /// The default keeps the eye above the ground and off the pole. A pole is a
    /// genuine degeneracy rather than a matter of taste: at `±π/2` the eye is
    /// directly over the pivot, the view direction is parallel to *up*, and the
    /// look-at basis is undefined.
    pub pitch_range: (f32, f32),
    /// Inclusive `(min, max)` the distance is held inside, in world units.
    pub distance_range: (f32, f32),
    /// Radians turned per pixel of mouse motion while dragging.
    pub drag_sensitivity: f32,
    /// Radians turned per **second** while an arrow key is held.
    pub key_rate: f32,
    /// World units the distance changes per wheel notch.
    pub zoom_per_notch: f32,
}

impl Orbit {
    /// An orbit at `yaw`/`pitch`/`distance`, circling the origin, with limits and
    /// rates matching what the demos converged on.
    ///
    /// Every field is public, so anything that disagrees is a struct update:
    ///
    /// ```
    /// # use slmsttaa::Orbit;
    /// let orbit = Orbit {
    ///     pitch_range: (0.05, 1.4),
    ///     distance_range: (3.0, 40.0),
    ///     ..Orbit::new(0.7, 0.6, 12.0)
    /// };
    /// ```
    pub fn new(yaw: f32, pitch: f32, distance: f32) -> Self {
        Self {
            pivot: [0.0; 3],
            yaw,
            pitch,
            distance,
            pitch_range: (0.08, 1.5),
            distance_range: (2.0, 20.0),
            drag_sensitivity: 0.005,
            key_rate: 1.8,
            zoom_per_notch: 0.5,
        }
    }

    /// Advance the viewpoint by one frame of input, then re-apply the limits.
    ///
    /// `dt` is wall-clock seconds ([`Renderer::dt`](crate::Renderer::dt)), not a
    /// fixed simulation step: where the camera is looking is not part of a
    /// consumer's simulation, and a paused world is still one you can walk round.
    ///
    /// The limits are applied **whether or not anything moved**, which is what
    /// lets a consumer widen or narrow them at runtime and have the current
    /// viewpoint follow — `terrain.rs` rescales both ranges when the map does,
    /// and the eye has to come back inside the new ones on that frame rather than
    /// the next one the user happens to drag on.
    pub fn drive(&mut self, input: &crate::Input, dt: f32, allow: OrbitInput) {
        use crate::input::Key;

        if allow.drag {
            let (dx, dy) = input.mouse_delta();
            self.yaw -= dx * self.drag_sensitivity;
            self.pitch -= dy * self.drag_sensitivity;
        }

        if allow.keys {
            let step = self.key_rate * dt;
            if input.is_key_held(Key::Left) {
                self.yaw += step;
            }
            if input.is_key_held(Key::Right) {
                self.yaw -= step;
            }
            if input.is_key_held(Key::Up) {
                self.pitch += step;
            }
            if input.is_key_held(Key::Down) {
                self.pitch -= step;
            }
        }

        if allow.zoom {
            self.distance -= input.scroll_delta() * self.zoom_per_notch;
        }

        self.pitch = self.pitch.clamp(self.pitch_range.0, self.pitch_range.1);
        self.distance = self
            .distance
            .clamp(self.distance_range.0, self.distance_range.1);
    }

    /// Where the eye has ended up, in world space.
    ///
    /// Spherical to Cartesian about [`pivot`](Self::pivot), with `+y` up and
    /// `yaw` measured from `+z` toward `+x` — the convention every demo here was
    /// already using, kept so that porting them changed no pictures.
    pub fn eye(&self) -> [f32; 3] {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        [
            self.pivot[0] + self.distance * cp * sy,
            self.pivot[1] + self.distance * sp,
            self.pivot[2] + self.distance * cp * cy,
        ]
    }
}

/// GPU-friendly view-projection uniform, plus where the eye is.
///
/// `glam::Mat4` is already 16-byte aligned and `repr(C)`-compatible, so we can
/// hand it straight to the GPU once wrapped in a `Pod` type.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    /// The inverse of [`view_proj`](Self::view_proj), for going the other way:
    /// from a point on the screen back out to a direction in the world.
    ///
    /// The sky pass is what needs it. A fullscreen triangle has no geometry and
    /// therefore no world position to shade from — the only thing a sky fragment
    /// knows is where it is on the screen, so it unprojects that to a ray and
    /// asks which way it is looking. Inverting a 4x4 once per frame on the CPU is
    /// free; doing it per fragment would not be.
    inv_view_proj: [[f32; 4]; 4],
    /// World-space eye position, padded to `vec4` for std140 alignment (`w` is
    /// unused).
    ///
    /// The matrix alone is enough to *place* a fragment but not to shade one
    /// view-dependently: a specular highlight and a Fresnel edge both need to
    /// know which way the viewer is, and that direction cannot be recovered from
    /// a projection matrix in the fragment stage without inverting it. So the eye
    /// rides along beside it.
    eye: [f32; 4],
    /// `[seconds since start, 0, 0, 0]` — wall-clock time, for shading that
    /// animates.
    ///
    /// It rides here rather than in a uniform of its own because it is wanted in
    /// exactly the same place and at exactly the same rate as the view: once per
    /// frame, by every pipeline. A surface whose *detail* moves — ripples on
    /// water, a shimmer, a scroll — should not have to rebuild and re-upload its
    /// mesh every frame to express that, which is what a consumer is forced into
    /// when the shader has no clock. Terrain's water was paying 10 ms a frame to
    /// do on the CPU what this makes free.
    frame: [f32; 4],
}

impl CameraUniform {
    /// Build the uniform payload from a camera and the frame clock.
    pub fn new(camera: &Camera, time: f32) -> Self {
        let view_proj = camera.view_projection();
        Self {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, 0.0],
            frame: [time, 0.0, 0.0, 0.0],
        }
    }
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            eye: [0.0; 4],
            frame: [0.0; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Input, Key};

    /// A camera looking down -Z from a little above the origin.
    fn camera() -> Camera {
        let mut camera = Camera::new(16.0 / 9.0);
        camera.look_from_to([0.0, 2.0, 6.0], [0.0, 0.0, 0.0]);
        camera
    }

    /// One second of a held arrow key must turn the same amount however many
    /// frames it is chopped into.
    ///
    /// This is the assertion the six copies of this code in `examples/` never
    /// had, and it is the bug they all shipped: a flat step per frame turned
    /// twice as far at 144 Hz as at 72 Hz. `Timeline` has the same test for the
    /// simulation clock (`time.rs`); this is the half of the problem that lives
    /// above the clock, in input the consumer reads every frame.
    #[test]
    fn a_held_key_turns_by_time_not_by_frames() {
        let input = Input::for_test(&[Key::Left], (0.0, 0.0), 0.0);

        let mut coarse = Orbit::new(0.0, 0.6, 6.0);
        coarse.drive(&input, 1.0, OrbitInput::ALL);

        let mut fine = Orbit::new(0.0, 0.6, 6.0);
        for _ in 0..100 {
            fine.drive(&input, 0.01, OrbitInput::ALL);
        }

        assert!(
            (coarse.yaw - fine.yaw).abs() < 1e-4,
            "one 1s step gave {} but a hundred 10ms steps gave {}",
            coarse.yaw,
            fine.yaw
        );
        // And it is the documented rate, not merely a self-consistent one.
        assert!((coarse.yaw - 1.8).abs() < 1e-5, "yaw was {}", coarse.yaw);
    }

    /// A drag is a *ratio*, so it must NOT scale with `dt` — the same motion
    /// delivered on a long frame and a short one has to turn the same amount.
    ///
    /// The mirror image of the test above, and the reason both constants live on
    /// one type: getting this one "consistent" with the other would be a bug.
    #[test]
    fn a_drag_turns_by_distance_not_by_time() {
        let input = Input::for_test(&[], (40.0, 0.0), 0.0);

        let mut slow = Orbit::new(0.0, 0.6, 6.0);
        slow.drive(&input, 0.1, OrbitInput::ALL);
        let mut fast = Orbit::new(0.0, 0.6, 6.0);
        fast.drive(&input, 0.001, OrbitInput::ALL);

        assert_eq!(slow.yaw, fast.yaw);
        assert!((slow.yaw + 0.2).abs() < 1e-6, "yaw was {}", slow.yaw);
    }

    /// Each flag gates exactly its own input and nothing else.
    #[test]
    fn the_gates_are_independent() {
        let input = Input::for_test(&[Key::Left], (40.0, 0.0), 3.0);
        let start = Orbit::new(0.5, 0.6, 6.0);

        let mut keys_only = start;
        keys_only.drive(
            &input,
            1.0,
            OrbitInput {
                keys: true,
                ..OrbitInput::NONE
            },
        );
        assert!((keys_only.yaw - (0.5 + 1.8)).abs() < 1e-5);
        assert_eq!(keys_only.distance, start.distance);

        let mut zoom_only = start;
        zoom_only.drive(
            &input,
            1.0,
            OrbitInput {
                zoom: true,
                ..OrbitInput::NONE
            },
        );
        assert_eq!(zoom_only.yaw, start.yaw);
        assert!((zoom_only.distance - (6.0 - 1.5)).abs() < 1e-5);

        let mut nothing = start;
        nothing.drive(&input, 1.0, OrbitInput::NONE);
        assert_eq!(nothing, start);
    }

    /// The limits are re-applied every frame, not only on frames that moved —
    /// which is what lets `terrain.rs` rescale both ranges when its map grows and
    /// have the viewpoint follow on that frame.
    #[test]
    fn limits_apply_without_any_input() {
        let input = Input::for_test(&[], (0.0, 0.0), 0.0);
        let mut orbit = Orbit {
            distance_range: (2.0, 20.0),
            ..Orbit::new(0.0, 0.6, 6.0)
        };

        orbit.distance_range = (40.0, 90.0);
        orbit.pitch_range = (0.9, 1.2);
        orbit.drive(&input, 0.016, OrbitInput::NONE);

        assert_eq!(orbit.distance, 40.0);
        assert_eq!(orbit.pitch, 0.9);
    }

    /// The eye is on the sphere it says it is on, offset by the pivot.
    #[test]
    fn the_eye_sits_on_the_pivots_sphere() {
        let orbit = Orbit {
            pivot: [1.0, 2.0, -3.0],
            ..Orbit::new(0.7, 0.4, 9.0)
        };
        let eye = Vec3::from(orbit.eye());
        let radius = (eye - Vec3::from(orbit.pivot)).length();
        assert!((radius - 9.0).abs() < 1e-4, "radius was {radius}");
        // Pitch is elevation, so a positive one puts the eye above the pivot.
        assert!(eye.y > orbit.pivot[1]);
    }

    /// Zero pitch and zero yaw looks straight down `+z`, which is the convention
    /// every demo was already using and the one porting them must not change.
    #[test]
    fn the_zero_orientation_is_on_the_z_axis() {
        let orbit = Orbit::new(0.0, 0.0, 5.0);
        let eye = orbit.eye();
        assert!((eye[0]).abs() < 1e-6, "x was {}", eye[0]);
        assert!((eye[1]).abs() < 1e-6, "y was {}", eye[1]);
        assert!((eye[2] - 5.0).abs() < 1e-6, "z was {}", eye[2]);
    }

    #[test]
    fn centre_ray_aims_at_the_target() {
        let camera = camera();
        let ray = camera.ray_through_ndc([0.0, 0.0]);
        let to_target = (camera.target - camera.eye).normalize();
        let direction = Vec3::from(ray.direction);
        assert!(
            (direction - to_target).length() < 1e-4,
            "centre ray {direction:?} should aim at the target along {to_target:?}"
        );
    }

    #[test]
    fn directions_are_unit_length() {
        let camera = camera();
        for ndc in [[0.0, 0.0], [-1.0, -1.0], [1.0, 1.0], [0.7, -0.3]] {
            let length = Vec3::from(camera.ray_through_ndc(ndc).direction).length();
            assert!(
                (length - 1.0).abs() < 1e-5,
                "ndc {ndc:?} gave length {length}"
            );
        }
    }

    /// The property that actually matters for picking: project a known world
    /// point, cast a ray back through the pixel it landed on, and the point must
    /// lie on that ray. This is what catches a flipped Y or the wrong near-plane
    /// depth convention — both of which produce a plausible-looking ray that
    /// selects the wrong object.
    #[test]
    fn a_ray_through_a_projected_point_hits_that_point() {
        let camera = camera();
        for point in [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.5, 0.5, -2.0),
            Vec3::new(-2.0, 1.0, 1.0),
        ] {
            let clip = camera.view_projection() * point.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            let ray = camera.ray_through_ndc([ndc.x, ndc.y]);

            let origin = Vec3::from(ray.origin);
            let direction = Vec3::from(ray.direction);
            let along = (point - origin).dot(direction);
            assert!(along > 0.0, "{point:?} landed behind the ray origin");
            let closest = origin + direction * along;
            assert!(
                (closest - point).length() < 1e-3,
                "{point:?} missed by {}",
                (closest - point).length()
            );
        }
    }

    /// Right on screen is right in the world, and up is up. A Y flip passes both
    /// tests above and fails this one.
    #[test]
    fn ndc_axes_point_the_expected_way() {
        let camera = camera();
        let centre = Vec3::from(camera.ray_through_ndc([0.0, 0.0]).direction);
        let right = Vec3::from(camera.ray_through_ndc([0.9, 0.0]).direction);
        let up = Vec3::from(camera.ray_through_ndc([0.0, 0.9]).direction);

        // The camera looks down -Z from +Z, so world +X is to the right of centre.
        assert!(
            right.x > centre.x,
            "ndc +x should look further along world +x"
        );
        assert!(up.y > centre.y, "ndc +y should look further along world +y");
    }
}
