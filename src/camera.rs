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
