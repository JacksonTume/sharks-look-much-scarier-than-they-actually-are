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

use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};

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

/// How an instance looks: a color tint with an alpha channel.
///
/// This is what makes two placements of the *same* mesh tell apart. Color
/// otherwise lives in the shared vertex buffer every instance reads, so without
/// it a mesh drawn twenty times is twenty identical objects by construction —
/// and duplicating the mesh per color would undo the point of instancing.
///
/// The tint **multiplies** [`Vertex::color`](crate::Vertex::color) rather than
/// replacing it, so a mesh authored with its own shading (a gradient down a box,
/// a height palette across a landscape) keeps that detail and gets recolored on
/// top. A white tint therefore means "as authored".
///
/// Deliberately *not* a material system — no shader graph, no pipeline
/// permutations, no textures. When something demands those, they are named in
/// `ROADMAP.md` under *Beyond*. There is no specular or shininess field either:
/// the lighting model is Lambert diffuse, which has no specular term, so the
/// field would be storage for a number nothing reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Linear RGBA multiplied into the vertex color.
    ///
    /// Alpha below `1.0` moves the instance into the transparent pass: it is
    /// blended over whatever is behind it and does not write depth. Alpha lives
    /// here rather than on [`Vertex`](crate::Vertex) because "see-through" is a
    /// property of *this placement of a mesh*, not of the mesh's corners.
    pub tint: [f32; 4],
}

impl Material {
    /// Draw the mesh exactly as authored: white, fully opaque.
    pub const OPAQUE: Self = Self {
        tint: [1.0, 1.0, 1.0, 1.0],
    };

    /// An opaque tint from linear RGB.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            tint: [r, g, b, 1.0],
        }
    }

    /// This material at `alpha` opacity (`1.0` opaque, `0.0` invisible).
    pub const fn with_alpha(mut self, alpha: f32) -> Self {
        self.tint[3] = alpha;
        self
    }

    /// Whether this material needs the blended, depth-write-off draw.
    pub(crate) fn is_transparent(&self) -> bool {
        self.tint[3] < 1.0
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::OPAQUE
    }
}

/// One entry in the draw-list: draw this mesh, here, like this.
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
    /// How it looks. Defaults to [`Material::OPAQUE`].
    pub material: Material,
}

impl Instance {
    /// Draw `mesh` at `transform`, as authored.
    pub const fn new(mesh: MeshHandle, transform: Transform) -> Self {
        Self {
            mesh,
            transform,
            material: Material::OPAQUE,
        }
    }

    /// Draw `mesh` at the origin, unrotated and unscaled.
    ///
    /// The whole of a static demo's draw-list: geometry already authored in world
    /// space needs no placement on top of it.
    pub const fn at(mesh: MeshHandle) -> Self {
        Self::new(mesh, Transform::IDENTITY)
    }

    /// This instance with `material` applied.
    pub const fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }
}

/// The per-instance payload the vertex shader reads: the model matrix, the
/// matrix that transforms its normals, and the material tint.
///
/// `repr(C)` + `Pod` so a `&[InstanceRaw]` uploads straight into a buffer, the
/// same trick [`Vertex`](crate::Vertex) uses.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct InstanceRaw {
    model: [[f32; 4]; 4],
    /// The 3×3 inverse-transpose of `model`'s rotation/scale block, as three
    /// columns. See [`InstanceRaw::normal_matrix`].
    normal: [[f32; 3]; 3],
    tint: [f32; 4],
}

impl InstanceRaw {
    /// Bake an instance's transform and material into what the shader reads.
    pub(crate) fn from_instance(instance: &Instance) -> Self {
        let model = instance.transform.mat4();
        Self {
            model: model.to_cols_array_2d(),
            normal: Self::normal_matrix(model),
            tint: instance.material.tint,
        }
    }

    /// The matrix that takes an object-space normal to world space.
    ///
    /// **Not** the model matrix's upper 3×3, which is the tempting answer and is
    /// wrong the moment a scale is non-uniform: scaling a box's height stretches
    /// its normals along with its geometry, so the flat top shades as though it
    /// were tilted. The correct transform is the *inverse-transpose*, which
    /// undoes exactly that skew while leaving rotation alone.
    ///
    /// A flattened transform (any scale component of zero) has no invertible 3×3
    /// block, so it falls back to the plain matrix rather than filling the buffer
    /// with `NaN` — a degenerate mesh then shades oddly instead of vanishing.
    fn normal_matrix(model: Mat4) -> [[f32; 3]; 3] {
        let basis = Mat3::from_mat4(model);
        let usable = if basis.determinant().abs() > 1e-8 {
            basis.inverse().transpose()
        } else {
            basis
        };
        usable.to_cols_array_2d()
    }

    /// A `mat4x4` costs four attribute slots — WGSL has no matrix vertex
    /// attribute, so the shader reassembles it from four `vec4` columns at
    /// locations 3–6, then the normal matrix from three `vec3` columns at 7–9,
    /// then the tint at 10. Locations 0–2 belong to [`Vertex`](crate::Vertex).
    ///
    /// That is eleven of the sixteen vertex attributes WebGL2 guarantees. Room
    /// remains, but it is no longer generous — worth knowing before anything else
    /// asks to ride this buffer.
    #[rustfmt::skip]
    const ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x3, 8 => Float32x3, 9 => Float32x3,
        10 => Float32x4,
    ];

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
