//! Vertex format used by the demo pipeline.

/// A position + normal + color vertex.
///
/// `repr(C)` plus `Pod`/`Zeroable` lets us upload a `&[Vertex]` straight to a
/// GPU buffer with `bytemuck::cast_slice`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Object-space position.
    pub position: [f32; 3],
    /// Object-space surface normal, expected to be unit length.
    ///
    /// **Object space, not world space** — the engine transforms it by the
    /// instance's normal matrix, so one uploaded mesh lights correctly at every
    /// placement. That is the whole reason this field exists: before it, a demo
    /// baked shading into [`Vertex::color`], which is only correct while the mesh
    /// never moves.
    ///
    /// A mesh that genuinely has no surface (a line list, a point cloud) can pass
    /// [`Vertex::UP`] and ignore the lighting.
    pub normal: [f32; 3],
    /// Per-vertex RGB color, multiplied by the instance's
    /// [`Material::tint`](crate::Material::tint) and then by the light.
    pub color: [f32; 3],
}

impl Vertex {
    /// A straight-up normal, for flat horizontal surfaces and for geometry with
    /// no meaningful facing.
    pub const UP: [f32; 3] = [0.0, 1.0, 0.0];

    /// A vertex at `position` with `color`, facing straight up.
    pub const fn new(position: [f32; 3], color: [f32; 3]) -> Self {
        Self {
            position,
            normal: Self::UP,
            color,
        }
    }

    const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];

    /// The vertex buffer layout matching [`Vertex`] and `shader.wgsl`.
    pub const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}
