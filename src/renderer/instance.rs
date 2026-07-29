//! Where a mesh sits in the world: [`Transform`], and the [`Instance`] draw-list
//! entry that pairs one with an uploaded [`MeshHandle`].
//!
//! This is the seam that lets a mesh be uploaded once and drawn many times. Before
//! it, geometry arrived pre-baked into world space, so moving something meant
//! rebuilding its vertices and re-uploading them every frame — the wall the `cube`
//! demo used to live behind.
//!
//! Everything public here is plain arrays. The engine composes matrices with
//! [`glam`] internally, but a consumer never sees a `glam` type — the same rule
//! [`Camera::look_from_to`](crate::Camera::look_from_to) and
//! [`Vertex`](crate::Vertex) already follow, so a demo can place twenty objects
//! without taking a math dependency.

use glam::{EulerRot, Mat4, Quat, Vec3};

/// A handle to a mesh uploaded to the GPU.
///
/// Returned by [`Renderer::upload_mesh`](crate::Renderer::upload_mesh) and named
/// by every [`Instance`] that draws it. Handles are stable for the lifetime of
/// the renderer: nothing is ever freed, so a handle can never dangle or be
/// silently reused for different geometry. (A removal API waits for a demo that
/// actually spawns and destroys objects — see `ROADMAP.md`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshHandle(pub(crate) usize);

/// Position, rotation, and scale — a rigid placement plus a stretch.
///
/// Rotation is **Euler angles in radians**, `[x, y, z]`, applied in `Y → X → Z`
/// order: yaw about Y first, then pitch about X, then roll about Z. That order is
/// the conventional one for objects that mostly turn on the spot, and it is what
/// makes `rotation: [0.0, angle, 0.0]` read as "spinning" without further thought.
///
/// Euler angles cannot express every orientation cleanly — compose two rotations
/// about different axes and you can lose a degree of freedom (gimbal lock). That
/// is a real limit, accepted deliberately: a quaternion is the correct fix and an
/// unpleasant thing to author by hand without a math library, which is exactly
/// what this API exists to avoid. Revisit when a demo needs orientations Euler
/// angles genuinely can't reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// World-space position.
    pub position: [f32; 3],
    /// Euler rotation in radians, `[pitch about X, yaw about Y, roll about Z]`,
    /// applied `Y → X → Z`.
    pub rotation: [f32; 3],
    /// Per-axis scale. `[1.0; 3]` leaves the mesh at its authored size.
    pub scale: [f32; 3],
}

impl Transform {
    /// The identity placement: at the origin, unrotated, unscaled.
    pub const IDENTITY: Self = Self {
        position: [0.0; 3],
        rotation: [0.0; 3],
        scale: [1.0; 3],
    };

    /// An identity transform moved to `position`.
    pub const fn from_position(position: [f32; 3]) -> Self {
        Self {
            position,
            ..Self::IDENTITY
        }
    }

    /// This transform with its rotation replaced (radians, see [`Transform`]).
    pub const fn with_rotation(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }

    /// This transform with its per-axis scale replaced.
    pub const fn with_scale(mut self, scale: [f32; 3]) -> Self {
        self.scale = scale;
        self
    }

    /// This transform scaled equally on every axis.
    pub const fn with_uniform_scale(self, scale: f32) -> Self {
        self.with_scale([scale; 3])
    }

    /// The model matrix, in column-major order (each `[f32; 4]` is a column).
    ///
    /// The engine uploads this per instance; it is public because a consumer that
    /// wants to *read* where it put something shouldn't have to re-derive the
    /// composition.
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        self.mat4().to_cols_array_2d()
    }

    /// Scale, then rotate, then translate — the standard order, and the only one
    /// under which `scale` means "make the object bigger" rather than "move it
    /// further away".
    fn mat4(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            Quat::from_euler(
                EulerRot::YXZ,
                self.rotation[1],
                self.rotation[0],
                self.rotation[2],
            ),
            Vec3::from(self.position),
        )
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// One entry in the draw-list: draw this mesh, here.
///
/// Hand a slice of these to
/// [`Renderer::set_instances`](crate::Renderer::set_instances). The same
/// [`MeshHandle`] may appear any number of times — that is the point, and the
/// engine batches repeats of one mesh into a single instanced draw call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    /// Which uploaded mesh to draw.
    pub mesh: MeshHandle,
    /// Where to draw it.
    pub transform: Transform,
}

impl Instance {
    /// Draw `mesh` at `transform`.
    pub const fn new(mesh: MeshHandle, transform: Transform) -> Self {
        Self { mesh, transform }
    }

    /// Draw `mesh` at the origin, unrotated and unscaled.
    ///
    /// The whole of a static demo's draw-list: geometry already authored in world
    /// space needs no placement on top of it.
    pub const fn at(mesh: MeshHandle) -> Self {
        Self::new(mesh, Transform::IDENTITY)
    }
}

/// The per-instance payload the vertex shader reads: just the model matrix.
///
/// `repr(C)` + `Pod` so a `&[InstanceRaw]` uploads straight into a buffer, the
/// same trick [`Vertex`](crate::Vertex) uses.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct InstanceRaw {
    model: [[f32; 4]; 4],
}

impl InstanceRaw {
    /// Bake an instance's transform into the matrix the shader multiplies by.
    pub(crate) fn from_instance(instance: &Instance) -> Self {
        Self {
            model: instance.transform.matrix(),
        }
    }

    /// A `mat4x4` costs four attribute slots — WGSL has no matrix vertex
    /// attribute, so the shader reassembles it from four `vec4` columns at
    /// locations 2–5 (0 and 1 belong to [`Vertex`](crate::Vertex)).
    const ATTRS: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4];

    /// The instance-step buffer layout matching `shader.wgsl`.
    ///
    /// [`VertexStepMode::Instance`](wgpu::VertexStepMode::Instance) is what makes
    /// this per-object rather than per-vertex. It was chosen over a storage buffer
    /// (which the WebGL2 fallback does not have at all under
    /// `downlevel_webgl2_defaults`) and over a uniform with dynamic offsets (which
    /// would cost a bind group and a draw call per object, defeating the point).
    pub(crate) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}
