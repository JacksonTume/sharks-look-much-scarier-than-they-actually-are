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

    /// This transform applied **inside** `parent`, as a world matrix.
    ///
    /// The composition an articulated figure needs: an upper arm is placed
    /// relative to a torso, and when the torso turns the arm has to go with it.
    ///
    /// It returns a matrix rather than another [`Transform`] because it *cannot*
    /// return one — compose a rotation with a non-uniform scale and the result is
    /// a shear, which position/rotation/scale has no way to express. Attempting it
    /// would silently drop the shear and put the limb in the wrong place.
    ///
    /// The engine does the multiply so the consumer doesn't need a math library:
    /// the same rule [`Transform`] itself follows. Hand the result to
    /// [`Instance::from_matrix`].
    ///
    /// ```no_run
    /// # use slmsttaa::{Instance, MeshHandle, Transform};
    /// # fn demo(arm_mesh: MeshHandle, torso: Transform, arm: Transform) -> Instance {
    /// Instance::from_matrix(arm_mesh, arm.then(&torso))
    /// # }
    /// ```
    pub fn then(&self, parent: &Transform) -> [[f32; 4]; 4] {
        (parent.mat4() * self.mat4()).to_cols_array_2d()
    }

    /// This transform applied inside an already-composed parent matrix.
    ///
    /// What [`Transform::then`] is for two levels, this is for any number: a
    /// forearm inside an upper arm inside a torso is
    /// `forearm.then_matrix(upper.then(&torso))`. The chain stays in matrix form
    /// once it leaves the first join, which is the only representation that
    /// survives arbitrary nesting.
    pub fn then_matrix(&self, parent: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
        (Mat4::from_cols_array_2d(&parent) * self.mat4()).to_cols_array_2d()
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
/// `ROADMAP.md` under *Beyond*.
///
/// # The specular fields, and why they exist now
///
/// This type shipped with a tint and nothing else, and said so: "the lighting
/// model is Lambert diffuse, which has no specular term, so the field would be
/// storage for a number nothing reads." That was correct until something needed
/// to look *wet*. Water reads as water almost entirely through view-dependent
/// shading — a moving sun glint and a bright grazing edge — and under pure
/// Lambert a rippling surface and a flat one are very nearly the same picture.
/// So the lighting model grew the term first and these fields describe it.
///
/// They default to zero, which is exactly the old behavior: every demo that does
/// not ask for a highlight renders identically to before.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Linear RGBA multiplied into the vertex color.
    ///
    /// Alpha below `1.0` moves the instance into the transparent pass: it is
    /// blended over whatever is behind it and does not write depth.
    ///
    /// This is the *overall* strength of the transparency; [`Vertex::color`]'s
    /// alpha is its shape across the surface. A uniformly translucent object
    /// should use this one and leave the vertices opaque.
    ///
    /// [`Vertex::color`]: crate::Vertex::color
    pub tint: [f32; 4],

    /// How strong the specular highlight is. `0.0` is the Lambert-only surface
    /// every demo had before this field existed.
    pub specular: f32,

    /// Blinn-Phong exponent: how *tight* the highlight is. Low values give a
    /// broad sheen (a damp rock), high values a small sharp glint (still water).
    /// Ignored when [`specular`](Self::specular) is zero.
    pub shininess: f32,

    /// Schlick reflectance at normal incidence, driving a Fresnel edge.
    ///
    /// A transparent surface viewed face-on is mostly see-through and viewed at a
    /// grazing angle is mostly reflective — the effect that makes a lake mirror
    /// the sky at the far shore while showing its bed at your feet. Nonzero
    /// values brighten the surface toward [`fresnel_tint`](Self::fresnel_tint)
    /// and raise its opacity as the view angle flattens. Water is about `0.02`.
    ///
    /// This is a *stand-in* for a reflection, not a reflection: there is no
    /// second render pass, so the surface goes toward a flat colour rather than
    /// toward an image of the scene. That is the honest ceiling until the engine
    /// grows an offscreen target (see `ROADMAP.md`, *Beyond*).
    pub fresnel: f32,

    /// The colour a Fresnel edge tends toward — a sky colour, for water.
    pub fresnel_tint: [f32; 3],

    /// Strength of an **animated procedural ripple** applied to the surface
    /// normal, in the fragment shader. `0.0` leaves the normal alone.
    ///
    /// This is shading detail, not geometry: nothing moves, the surface just
    /// stops being locally flat. That distinction is the whole value of it — a
    /// consumer wanting a surface whose detail *animates* would otherwise have to
    /// rebuild and re-upload its mesh every frame to express it, which is exactly
    /// what terrain's water was doing at a measured 10 ms a frame. Here it costs
    /// no mesh work at all and gets per-*pixel* detail rather than per-vertex.
    ///
    /// Not a water feature, though water is what asked for it: it is a moving
    /// normal, which is equally a shimmer, a jelly, or heat haze.
    pub ripple_strength: f32,

    /// Spatial frequency of the ripple's largest wave, in world units. Higher is
    /// choppier. Ignored when [`ripple_strength`](Self::ripple_strength) is zero.
    pub ripple_scale: f32,

    /// Force the blended, depth-write-off pipeline even when
    /// [`tint`](Self::tint)'s alpha is `1.0`.
    ///
    /// Needed because per-vertex alpha is invisible to the pipeline choice: a
    /// mesh whose corners fade out but whose tint is fully opaque would otherwise
    /// be drawn in the opaque pass and its fade ignored. Terrain's water hits
    /// this the moment its opacity slider is dragged to maximum.
    pub blended: bool,

    /// How far this surface displaces what is seen *through* it, in screen
    /// widths. `0.0` is a surface you see straight through.
    ///
    /// Only the blended pass can do this — it samples the opaque scene rendered
    /// before it — so an opaque material ignores the field. Typical water is
    /// around `0.02`; past `0.05` the distortion stops reading as a surface and
    /// starts reading as a broken image, because a screen-space displacement has
    /// no idea what it is dragging across.
    pub refraction: f32,

    /// How quickly the surface takes on its own colour with thickness, as a
    /// Beer-Lambert coefficient per world unit.
    ///
    /// This is what separates a deep basin from a shallow one: at `0.0` the
    /// surface is uniformly its tint no matter how much of it the light crossed,
    /// which is exactly the "same blue everywhere" look Slice 14 shipped and
    /// Slice 16 set out to fix. Needs [`refraction`](Self::refraction) to be
    /// nonzero, since thickness is only known where the scene behind is sampled.
    pub absorption: f32,

    /// How much of the reflection is traced from the scene rather than taken
    /// from the sky. `0.0` reflects sky only; `1.0` uses the traced hit wherever
    /// the trace finds one.
    ///
    /// Kept separate from [`fresnel`](Self::fresnel), which decides *how
    /// reflective* the surface is, because this decides *what it reflects* —
    /// and the screen-space trace is the expensive half. A material can want a
    /// Fresnel edge without paying 28 depth samples a fragment for it.
    pub reflection: f32,
}

impl Material {
    /// Draw the mesh exactly as authored: white, fully opaque, no highlight.
    pub const OPAQUE: Self = Self {
        tint: [1.0, 1.0, 1.0, 1.0],
        specular: 0.0,
        shininess: 32.0,
        fresnel: 0.0,
        fresnel_tint: [1.0, 1.0, 1.0],
        ripple_strength: 0.0,
        ripple_scale: 8.0,
        blended: false,
        refraction: 0.0,
        absorption: 0.0,
        reflection: 0.0,
    };

    /// An opaque tint from linear RGB.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            tint: [r, g, b, 1.0],
            ..Self::OPAQUE
        }
    }

    /// This material at `alpha` opacity (`1.0` opaque, `0.0` invisible).
    pub const fn with_alpha(mut self, alpha: f32) -> Self {
        self.tint[3] = alpha;
        self
    }

    /// This material with a specular highlight of `strength`, tightened by
    /// `shininess` (the Blinn-Phong exponent).
    pub const fn with_specular(mut self, strength: f32, shininess: f32) -> Self {
        self.specular = strength;
        self.shininess = shininess;
        self
    }

    /// This material with a Fresnel edge of reflectance `f0`, tending toward
    /// `tint` at grazing angles. See [`fresnel`](Self::fresnel).
    pub const fn with_fresnel(mut self, f0: f32, tint: [f32; 3]) -> Self {
        self.fresnel = f0;
        self.fresnel_tint = tint;
        self
    }

    /// This material with an animated ripple of `strength` on its normal, whose
    /// largest wave has spatial frequency `scale`. See
    /// [`ripple_strength`](Self::ripple_strength).
    pub const fn with_ripples(mut self, strength: f32, scale: f32) -> Self {
        self.ripple_strength = strength;
        self.ripple_scale = scale;
        self
    }

    /// This material refracting what is behind it by `strength`, taking on its
    /// own colour at `absorption` per world unit of thickness.
    ///
    /// Implies [`blended`](Self::blended): refraction composites the scene behind
    /// the surface itself, which is only correct in the blended pass — and a
    /// caller who asked to see *through* a surface has already said which pass
    /// they meant. See [`refraction`](Self::refraction).
    pub const fn with_refraction(mut self, strength: f32, absorption: f32) -> Self {
        self.refraction = strength;
        self.absorption = absorption;
        self.blended = true;
        self
    }

    /// This material tracing `strength` of its reflection from the scene instead
    /// of taking all of it from the sky. See [`reflection`](Self::reflection).
    pub const fn with_reflection(mut self, strength: f32) -> Self {
        self.reflection = strength;
        self
    }

    /// Draw this instance in the blended pass whatever its tint alpha says.
    /// See [`blended`](Self::blended).
    pub const fn blended(mut self) -> Self {
        self.blended = true;
        self
    }

    /// Whether this material needs the blended, depth-write-off draw.
    pub(crate) fn is_transparent(&self) -> bool {
        self.blended || self.tint[3] < 1.0
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
    /// How it looks. Defaults to [`Material::OPAQUE`].
    pub material: Material,
    /// Where to draw it, already composed to world space.
    ///
    /// Private because there are two ways in — a [`Transform`] or a matrix from
    /// [`Transform::then`] — and storing the composed result is what lets both
    /// exist without the draw path caring which was used.
    model: [[f32; 4]; 4],
}

impl Instance {
    /// Draw `mesh` at `transform`, as authored.
    pub fn new(mesh: MeshHandle, transform: Transform) -> Self {
        Self {
            mesh,
            material: Material::OPAQUE,
            model: transform.matrix(),
        }
    }

    /// Draw `mesh` at an already-composed world matrix.
    ///
    /// The escape hatch for hierarchies: a limb's place in the world is its
    /// parent's transform times its own, and that product is a matrix (see
    /// [`Transform::then`], which is how you get one). Every other placement
    /// should use [`Instance::new`] — this is not the general way to position
    /// something, it is the way to position something *relative to something
    /// else*.
    pub fn from_matrix(mesh: MeshHandle, model: [[f32; 4]; 4]) -> Self {
        Self {
            mesh,
            material: Material::OPAQUE,
            model,
        }
    }

    /// The world matrix this instance will be drawn with.
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        self.model
    }

    /// Draw `mesh` at the origin, unrotated and unscaled.
    ///
    /// The whole of a static demo's draw-list: geometry already authored in world
    /// space needs no placement on top of it.
    pub fn at(mesh: MeshHandle) -> Self {
        Self::new(mesh, Transform::IDENTITY)
    }

    /// This instance with `material` applied.
    pub const fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }
}

/// The per-instance payload the shaders read: the model matrix, the matrix that
/// transforms its normals, and the material — tint plus the view-dependent
/// shading terms, which the vertex stage passes straight through to the
/// fragment stage.
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
    /// `[specular, shininess, fresnel, ripple strength]` — the view-dependent
    /// shading terms packed into one attribute slot rather than four. Scalars
    /// would have cost one of the sixteen WebGL2 guarantees each to carry four
    /// bytes; this costs one slot and wastes none.
    shading: [f32; 4],
    /// `[fresnel tint rgb, ripple scale]`. The ripple parameters ride the spare
    /// `w` channels of the two vectors above rather than claiming a slot of
    /// their own — with thirteen of sixteen attributes already spoken for, the
    /// padding was the cheaper place to put them.
    fresnel_tint: [f32; 4],
    /// `[refraction, absorption, reflection, unused]` — the terms that sample
    /// the scene texture.
    ///
    /// This one *did* have to claim a slot: Slice 15 spent the last two spare
    /// `w` channels on the ripple parameters, so there was no padding left to
    /// hide in. Its own `w` is now the only spare per-instance float in the
    /// buffer, and there are two attribute slots behind it.
    water: [f32; 4],
}

impl InstanceRaw {
    /// Bake an instance's transform and material into what the shader reads.
    pub(crate) fn from_instance(instance: &Instance) -> Self {
        let model = Mat4::from_cols_array_2d(&instance.model);
        let m = &instance.material;
        Self {
            model: model.to_cols_array_2d(),
            normal: Self::normal_matrix(model),
            tint: m.tint,
            shading: [m.specular, m.shininess, m.fresnel, m.ripple_strength],
            fresnel_tint: [
                m.fresnel_tint[0],
                m.fresnel_tint[1],
                m.fresnel_tint[2],
                m.ripple_scale,
            ],
            water: [m.refraction, m.absorption, m.reflection, 0.0],
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
    /// then the tint at 10, the packed shading terms at 11, the Fresnel tint at
    /// 12, and the scene-sampling terms at 13. Locations 0–2 belong to
    /// [`Vertex`](crate::Vertex).
    ///
    /// **That is fourteen of the sixteen vertex attributes WebGL2 guarantees**,
    /// and the count has now gone up in three consecutive slices: eleven (9/10),
    /// thirteen (14), fourteen (16). Slice 15 got in for free by packing into
    /// spare `w` channels, which was the mitigation Slice 14 predicted would be
    /// necessary; Slice 16 could not, because Slice 15 had spent the padding.
    ///
    /// **Two slots left is the end of the runway, and the next thing to want
    /// per-instance data should not assume it can have one.** The options in
    /// order of preference: use `water`'s spare `w`; pack several scalars into
    /// one slot as `shading` already does; or move per-instance data to a
    /// storage buffer, which is the real fix and which the WebGL2 fallback does
    /// not support at all — so taking it means deciding that fallback is over.
    #[rustfmt::skip]
    const ATTRS: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x3, 8 => Float32x3, 9 => Float32x3,
        10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4,
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
