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

    /// Update the aspect ratio, e.g. after a window resize.
    pub fn set_aspect(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect = width as f32 / height as f32;
        }
    }
}

/// GPU-friendly view-projection uniform.
///
/// `glam::Mat4` is already 16-byte aligned and `repr(C)`-compatible, so we can
/// hand it straight to the GPU once wrapped in a `Pod` type.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    /// Build the uniform payload from a camera.
    pub fn from_camera(camera: &Camera) -> Self {
        Self {
            view_proj: camera.view_projection().to_cols_array_2d(),
        }
    }
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }
}
