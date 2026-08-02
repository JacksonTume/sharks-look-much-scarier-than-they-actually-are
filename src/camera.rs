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

    /// A camera looking down -Z from a little above the origin.
    fn camera() -> Camera {
        let mut camera = Camera::new(16.0 / 9.0);
        camera.look_from_to([0.0, 2.0, 6.0], [0.0, 0.0, 0.0]);
        camera
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
