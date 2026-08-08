//! The wgpu rendering backend.
//!
//! [`Renderer`] owns the GPU surface, device/queue, and the render pipelines. It
//! is deliberately small: enough to draw a camera-transformed mesh supplied by
//! the consumer, with clear seams where a real engine would grow (a material
//! system, textures, an asset pipeline).
//!
//! A frame is six passes — sky, opaque, composite, blended, present, overlay —
//! declared in [`graph`] rather than hand-sequenced here, because Slice 16 made
//! pass ordering a correctness property: the water samples what the opaque pass
//! drew. All but the last two render into offscreen targets sized to the scene's
//! rectangle ([`Renderer::set_scene_rect`]); `present` is what decides where on
//! the window that rectangle lands, and the only pass that knows.

mod graph;
mod instance;
mod mesh;
mod overlay;
mod primitives;
mod vertex;

pub use instance::{Instance, Material, MeshHandle, Transform};
pub use mesh::Mesh;
pub use vertex::Vertex;

use instance::InstanceRaw;

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::{Camera, CameraUniform, Ray};
use crate::input::{Input, MouseButton};
use crate::time::{Clock, Timeline};
use graph::{Load, Pass, PassKind, RenderGraph, ResourceFormat, ResourceId, SWAPCHAIN};
use overlay::Overlay;
use slmsttaa_ui::{ImageId, Ui, UiInput, UiState};

/// Format of the depth buffer used for depth testing.
///
/// `Depth32Float` is a render-attachment format on every backend we target,
/// including the WebGL2 fallback. (If a future GL adapter ever rejects it, switch
/// to `Depth24Plus` — both the texture and the pipeline read this one constant.)
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Restate one engine [`Key`](crate::input::Key) as the toolkit's.
///
/// Two enums with the same variants, and no way to share one: the toolkit
/// depends on nothing, so it cannot import ours, and importing *its* into the
/// engine's public input API would make the engine's keyboard vocabulary the
/// UI crate's to define. This `match` is the price, and it is exhaustive — a
/// variant added to either side stops the build rather than quietly dropping a
/// key.
fn ui_key(key: crate::input::Key) -> slmsttaa_ui::Key {
    use crate::input::Key as E;
    use slmsttaa_ui::Key as U;
    match key {
        E::A => U::A,
        E::B => U::B,
        E::C => U::C,
        E::D => U::D,
        E::E => U::E,
        E::F => U::F,
        E::G => U::G,
        E::H => U::H,
        E::I => U::I,
        E::J => U::J,
        E::K => U::K,
        E::L => U::L,
        E::M => U::M,
        E::N => U::N,
        E::O => U::O,
        E::P => U::P,
        E::Q => U::Q,
        E::R => U::R,
        E::S => U::S,
        E::T => U::T,
        E::U => U::U,
        E::V => U::V,
        E::W => U::W,
        E::X => U::X,
        E::Y => U::Y,
        E::Z => U::Z,
        E::Digit0 => U::Digit0,
        E::Digit1 => U::Digit1,
        E::Digit2 => U::Digit2,
        E::Digit3 => U::Digit3,
        E::Digit4 => U::Digit4,
        E::Digit5 => U::Digit5,
        E::Digit6 => U::Digit6,
        E::Digit7 => U::Digit7,
        E::Digit8 => U::Digit8,
        E::Digit9 => U::Digit9,
        E::Up => U::Up,
        E::Down => U::Down,
        E::Left => U::Left,
        E::Right => U::Right,
        E::Escape => U::Escape,
        E::Tab => U::Tab,
        E::Enter => U::Enter,
        E::Space => U::Space,
        E::Backspace => U::Backspace,
        E::Delete => U::Delete,
        E::Home => U::Home,
        E::End => U::End,
        E::PageUp => U::PageUp,
        E::PageDown => U::PageDown,
    }
}

/// Restate one engine [`Modifiers`](crate::input::Modifiers) as the toolkit's.
fn ui_modifiers(modifiers: crate::input::Modifiers) -> slmsttaa_ui::Modifiers {
    slmsttaa_ui::Modifiers {
        shift: modifiers.shift,
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        logo: modifiers.logo,
    }
}

/// How the scene's meshes are rasterized.
///
/// Wireframe is drawn with portable line-list topology (a deduplicated edge
/// buffer derived from each mesh's triangles), **not** `PolygonMode::Line`: that
/// fill mode needs a wgpu feature WebGL2 doesn't expose, and the engine keeps
/// strict native/web parity. Lines, by contrast, work on every backend we target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Filled triangles with depth test and back-face culling (the default).
    #[default]
    Solid,
    /// Mesh edges only, drawn as lines — handy for inspecting a terrain grid's
    /// topology or seeing through geometry.
    Wireframe,
}

/// One mesh uploaded to the GPU: its vertex + index buffers and the index count
/// to draw. Built from a public [`Mesh`] by [`GpuMesh::upload`].
///
/// Two index buffers are kept: the consumer's triangle list for [`RenderMode::Solid`]
/// and a derived edge list for [`RenderMode::Wireframe`], so toggling render mode
/// never re-uploads geometry.
struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    line_index_buffer: wgpu::Buffer,
    line_index_count: u32,
}

impl GpuMesh {
    /// Upload a CPU-side [`Mesh`] into fresh GPU buffers.
    fn upload(device: &wgpu::Device, mesh: &Mesh) -> Self {
        let vertex_buffer = buffer_from(
            device,
            "mesh vertex buffer",
            &mesh.vertices,
            wgpu::BufferUsages::VERTEX,
        );
        let index_buffer = buffer_from(
            device,
            "mesh index buffer",
            &mesh.indices,
            wgpu::BufferUsages::INDEX,
        );
        // Derive a deduplicated edge list once, so wireframe is a buffer swap.
        let line_indices = edge_indices(&mesh.indices);
        let line_index_buffer = buffer_from(
            device,
            "mesh line index buffer",
            &line_indices,
            wgpu::BufferUsages::INDEX,
        );
        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
            line_index_buffer,
            line_index_count: line_indices.len() as u32,
        }
    }
}

/// Create a GPU buffer holding `data`.
///
/// An **empty** slice gets a minimum-size buffer rather than a zero-size one:
/// backends reject a zero-length allocation, and an empty mesh is a perfectly
/// ordinary state for a consumer to be in (the terrain demo's water surface is
/// empty whenever the landscape has no standing water). Nothing draws from it —
/// [`Renderer::render`] skips a mesh with no indices.
fn buffer_from<T: bytemuck::Pod>(
    device: &wgpu::Device,
    label: &str,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    if data.is_empty() {
        return device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<T>() as wgpu::BufferAddress,
            usage,
            mapped_at_creation: false,
        });
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage,
    })
}

/// One instanced draw call: a run of consecutive instances in the instance
/// buffer that all share a mesh.
///
/// `byte_offset` rather than a first-instance index is deliberate. WebGL2 has no
/// equivalent of a non-zero `first_instance`, so the portable way to draw the
/// second mesh's run is to *bind the buffer offset* and always draw from instance
/// zero. See `ARCHITECTURE.md`.
struct InstanceDraw {
    /// Index into [`Renderer::meshes`].
    mesh: usize,
    /// Where this run starts in the instance buffer, in bytes.
    byte_offset: wgpu::BufferAddress,
    /// How many instances are in the run.
    count: u32,
    /// Whether this run needs the blended, depth-write-off pipeline. Runs are
    /// ordered so every opaque one precedes every transparent one.
    transparent: bool,
}

/// Instances the instance buffer is sized for on creation; it grows on demand.
const INITIAL_INSTANCE_CAPACITY: usize = 64;

/// Build a deduplicated line-list index buffer from a triangle-list one.
///
/// Each triangle contributes its three edges; an edge shared by two triangles
/// (every interior edge of a grid) is emitted only once, roughly halving the line
/// count versus drawing each triangle's edges independently.
fn edge_indices(tris: &[u32]) -> Vec<u32> {
    use std::collections::HashSet;
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut edges = Vec::new();
    for t in tris.chunks_exact(3) {
        for &(a, b) in &[(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = if a < b { (a, b) } else { (b, a) };
            if seen.insert(key) {
                edges.push(a);
                edges.push(b);
            }
        }
    }
    edges
}

/// A cursor in physical pixels → normalized device coordinates within `rect`.
///
/// Top-left origin and +y down on the way in; centre origin and +y up on the way
/// out. `None` only for a degenerate rect, never for a cursor outside it: a
/// point beyond the rect projects to NDC beyond `[-1, 1]`, which is meaningful
/// and is what keeps a drag continuous when the pointer strays onto a panel.
///
/// Free-standing so it can be tested. It is four lines of arithmetic that a
/// screenshot cannot check: a flipped Y or a forgotten origin produces a ray
/// that looks entirely plausible and selects the wrong object.
fn ndc_in_rect(cursor: (f32, f32), rect: [u32; 4]) -> Option<[f32; 2]> {
    let [rx, ry, rw, rh] = rect;
    if rw == 0 || rh == 0 {
        return None;
    }
    let (u, v) = (cursor.0 - rx as f32, cursor.1 - ry as f32);
    Some([2.0 * u / rw as f32 - 1.0, 1.0 - 2.0 * v / rh as f32])
}

/// Create an instance buffer with room for `capacity` instances.
fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance buffer"),
        size: (capacity * std::mem::size_of::<InstanceRaw>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The bind group layout for reading the opaque scene: colour, a sampler, and
/// depth.
///
/// Only the blended pipeline and the composite pass use it. Depth is bound as a
/// depth texture (not a float one) because that is what WebGPU permits for a
/// depth format, and it is read with `textureLoad` rather than through the
/// sampler — filtering depth averages a near surface with a far one and produces
/// a distance where no geometry is.
fn create_scene_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene texture bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// Bind the graph's current scene colour and depth views.
///
/// Rebuilt on every resize, because the views it holds are replaced when the
/// graph re-allocates its textures. Forgetting that is a use-after-resize that
/// wgpu catches, but only once the pass actually runs.
fn create_scene_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene texture bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(color),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(depth),
            },
        ],
    })
}

/// Create a fullscreen-triangle pipeline (the sky and the composite).
///
/// No vertex buffers and no depth: both entry points generate their own geometry
/// from `@builtin(vertex_index)` and cover the target completely, so there is
/// nothing to test against.
fn create_fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    entry_point: &str,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry_point),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            // The fullscreen triangle's winding depends on the target's Y
            // convention and is not worth reasoning about; nothing is behind it.
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Create a scene render pipeline for the given primitive `topology`.
///
/// The three variants are identical except for topology, culling, and blending:
/// solid triangles cull their back faces and overwrite; lines never cull; the
/// transparent variant blends over what is already there and does not write
/// depth. All share the camera-transform shader, the color target, and depth
/// *testing*, so wireframe edges and translucent surfaces both occlude correctly
/// against solid geometry.
///
/// `transparent` turning off depth writes is the load-bearing half: a blended
/// surface that wrote depth would hide the geometry behind it, which is the one
/// thing a see-through surface must not do. It also selects the `fs_water`
/// fragment entry point and, with it, a layout carrying the scene textures —
/// see the two-entry-point note in `shader.wgsl` for why that split is forced
/// rather than chosen.
fn create_scene_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    transparent: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("slmsttaa scene pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            // Two buffers: the mesh's vertices, then the per-object instance
            // data that steps once per draw rather than once per vertex.
            buffers: &[Vertex::layout(), InstanceRaw::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if transparent { "fs_water" } else { "fs_main" }),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(if transparent {
                    wgpu::BlendState::ALPHA_BLENDING
                } else {
                    wgpu::BlendState::REPLACE
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            // Front faces are wound counter-clockwise; cull the back faces so a
            // closed solid doesn't paint its far, inward-facing triangles. Lines
            // have no facing, so the wireframe pipeline passes `None`.
            front_face: wgpu::FrontFace::Ccw,
            cull_mode,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        // Depth testing so nearer fragments occlude farther ones. The depth value
        // comes from the vertex `@builtin(position)`; the camera already remaps Z
        // into wgpu's [0, 1] range, so `Less` is the right test.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!transparent),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Holds all GPU state required to render a frame.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The format every pipeline renders into, which is **not** always
    /// [`Renderer::config`]'s. On a WebGPU canvas the surface is linear and this
    /// is an sRGB view of it, so shading stays linear and the GPU does the
    /// encode. See the selection in [`Renderer::new`].
    render_format: wgpu::TextureFormat,
    size: winit::dpi::PhysicalSize<u32>,

    pipeline: wgpu::RenderPipeline,
    /// Alpha-blended variant of [`Renderer::pipeline`], with depth writes off.
    /// Selected for draw-list runs whose material alpha is below `1.0`. Unlike
    /// the other two it samples the opaque scene, so it carries a second bind
    /// group and a different fragment entry point.
    blend_pipeline: wgpu::RenderPipeline,
    /// Wireframe variant of [`Renderer::pipeline`] (line topology, no culling),
    /// selected when the render mode is [`RenderMode::Wireframe`].
    line_pipeline: wgpu::RenderPipeline,
    /// Fullscreen analytic sky, drawn behind the scene.
    sky_pipeline: wgpu::RenderPipeline,
    /// Fullscreen copy of the offscreen scene colour onto the swapchain.
    composite_pipeline: wgpu::RenderPipeline,
    /// Whether meshes draw filled or as edges. Defaults to [`RenderMode::Solid`].
    render_mode: RenderMode,
    /// Every mesh uploaded via [`Renderer::upload_mesh`], indexed by
    /// [`MeshHandle`]. Append-only, so a handle stays valid forever.
    meshes: Vec<GpuMesh>,
    /// The instance buffer backing this frame's draw-list, grown as needed and
    /// reused between frames.
    instance_buffer: wgpu::Buffer,
    /// How many instances [`Renderer::instance_buffer`] can currently hold.
    instance_capacity: usize,
    /// The draw-list proper: one instanced draw per mesh that appears in it.
    /// Empty until [`Renderer::set_instances`]; the engine just clears the screen
    /// until then.
    draws: Vec<InstanceDraw>,

    /// The frame's passes and the textures between them. Owns the depth
    /// attachment and the offscreen scene colour, and re-allocates both on
    /// resize.
    graph: RenderGraph,
    /// The offscreen colour the opaque half of the scene is drawn into, and
    /// which the water samples to refract and reflect.
    scene_color: ResourceId,
    /// Depth, written by the opaque pass and read two ways by the blended one:
    /// tested as a read-only attachment, and sampled for the reflection trace.
    scene_depth: ResourceId,
    /// The finished scene: the opaque colour copied here, then the water drawn
    /// on top. What the present pass blits onto the swapchain.
    scene_blend: ResourceId,
    scene_layout: wgpu::BindGroupLayout,
    scene_sampler: wgpu::Sampler,
    /// Binds colour + depth of the *opaque* scene, for the water to sample.
    /// Rebuilt whenever the graph re-allocates, which replaces their views.
    scene_bind_group: wgpu::BindGroup,
    /// The same, over the finished scene, for the present blit.
    present_bind_group: wgpu::BindGroup,
    /// Where the 3D scene draws, in logical points, or `None` for the whole
    /// window. See [`Renderer::set_scene_rect`].
    scene_rect: Option<[f32; 4]>,
    /// [`Renderer::scene_rect`] resolved to physical pixels: `[x, y, w, h]`.
    ///
    /// Stored rather than recomputed because four things read it — the offscreen
    /// textures' size, the present pass's viewport, the camera's aspect ratio and
    /// [`Renderer::pointer_ray`] — and rounding it independently at each is how
    /// picking ends up half a pixel away from the picture on a fractional-scale
    /// display.
    scene_rect_px: [u32; 4],

    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

    /// This frame's input snapshot. The event loop feeds it; the consumer reads
    /// it via [`Renderer::input`] from `Application::update`.
    input: Input,

    /// Set by [`Renderer::request_exit`], read by the event loop at the end of
    /// the frame. A flag rather than a direct call because the consumer's `update`
    /// has no reach into the event loop — which is the whole point of the
    /// inversion.
    exit_requested: bool,

    /// The screen-space overlay pass (2D UI/HUD), drawn after the 3D scene.
    overlay: Overlay,
    /// Persistent immediate-mode UI state (active widget, panel height).
    ui_state: UiState,
    /// This frame's keyboard events, translated into the toolkit's vocabulary.
    ///
    /// The toolkit cannot see [`crate::input::Event`] — it depends on nothing —
    /// so [`Renderer::ui`] re-states the log in `slmsttaa_ui` types. The buffer
    /// lives here and is reused rather than allocated per frame.
    ui_events: Vec<slmsttaa_ui::Event>,
    /// Frame clock for delta-time and the FPS readout — wall time.
    clock: Clock,
    /// Wall-clock seconds since the first frame, handed to shaders so surface
    /// detail can animate without the consumer touching its mesh. Summed from
    /// clamped frame deltas, so a stalled or backgrounded window advances it
    /// slowly rather than jumping (the same trade [`Clock`] already makes).
    elapsed: f32,
    /// Fixed-timestep simulation clock, driven from `clock` each frame. The
    /// counterpart to the above: identical steps rather than real ones.
    timeline: Timeline,

    /// Keeps the window alive for as long as the surface borrows it, and is read
    /// for its scale factor so the UI can be laid out in logical points.
    window: Arc<Window>,

    /// Frame-level capture control, `Some` only when the environment asked for
    /// it. See [`crate::capture`].
    #[cfg(not(target_arch = "wasm32"))]
    capture: Option<crate::capture::Capture>,
}

impl Renderer {
    /// Create a renderer bound to `window`.
    ///
    /// This performs async GPU initialization; on native we block on it with
    /// [`pollster`], and on the web the caller should `.await` it.
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        // Surfaces can't be configured with a zero dimension; clamp to 1.
        let width = size.width.max(1);
        let height = size.height.max(1);

        // Native: the best primary backend (Vulkan/Metal/DX12).
        // Web: prefer WebGPU, but allow the GL (WebGL2) fallback so browsers
        // without WebGPU still run. `PRIMARY` alone excludes GL, which is why a
        // WebGPU-less browser would otherwise find no adapter.
        #[cfg(not(target_arch = "wasm32"))]
        let backends = wgpu::Backends::PRIMARY;
        #[cfg(target_arch = "wasm32")]
        let backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;

        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = backends;
        let instance = wgpu::Instance::new(instance_desc);

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter found");

        log::info!("using adapter: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("slmsttaa device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                // On the web fall back to the WebGL2 limit set so a GL adapter
                // can satisfy the request; native uses the broader downlevel
                // defaults.
                #[cfg(target_arch = "wasm32")]
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                #[cfg(not(target_arch = "wasm32"))]
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("failed to create device");

        let surface_caps = surface.get_capabilities(&adapter);
        // The format the surface is *configured* with, and the (possibly
        // different) format everything actually **renders through**.
        //
        // Shading is done in linear space and the sRGB encode is the GPU's job on
        // write, which is why every pipeline wants an sRGB target. A desktop
        // backend simply offers one: Vulkan here lists `Bgra8UnormSrgb` first.
        // **A WebGPU canvas offers none at all** — Chrome offers exactly
        // `[Bgra8Unorm, Rgba8Unorm, Rgba16Float]` — so the old code fell through
        // to a linear format and every colour in the frame was displayed without
        // its encode. That is not subtle: the ground plane read mid-grey on
        // native and near-black in a browser, for every demo, and it went
        // unnoticed because both targets *ran*.
        //
        // The fix is `view_formats`, which exists for exactly this: configure the
        // surface with the format it offered, then render into an sRGB **view**
        // of the same texture. `add_srgb_suffix` is the identity on a format that
        // is already sRGB, so the desktop path is unchanged.
        let surface_format = surface_caps.formats[0];
        let srgb_format = surface_format.add_srgb_suffix();

        // GLES/WebGL cannot re-view a surface texture at all (the flag's own docs
        // say so), so the WebGL2 fallback keeps the old behaviour rather than
        // failing to start. It is the one target where this is still wrong, and
        // it is wrong *visibly* rather than fatally.
        let can_review_surface = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let render_format = if can_review_surface {
            srgb_format
        } else {
            surface_format
        };

        log::info!(
            "surface format: {surface_format:?}; rendering through {render_format:?} \
             (srgb: {}); offered: {:?}",
            render_format.is_srgb(),
            surface_caps.formats
        );

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            // `AutoVsync` avoids tearing and keeps the GPU from melting; switch
            // to `AutoNoVsync` to benchmark uncapped frame rates.
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: if render_format == surface_format {
                vec![]
            } else {
                vec![render_format]
            },
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        log::debug!(
            "surface configured: {:?} {}x{}",
            config.format,
            config.width,
            config.height,
        );

        // --- Camera uniform ------------------------------------------------
        let mut camera = Camera::new(width as f32 / height as f32);
        camera.set_aspect(width, height);
        let camera_uniform = CameraUniform::new(&camera, 0.0);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Both stages: the vertex shader projects with `view_proj`,
                    // and the fragment shader needs `eye` for the view direction
                    // the specular and Fresnel terms are built on. Leaving this
                    // at VERTEX is a *pipeline creation* panic rather than a
                    // wrong picture, which is the good kind of failure.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // --- Shaders -------------------------------------------------------
        //
        // `common.wgsl` is textually prepended to both modules rather than
        // duplicated in each. WGSL has no `#include`, and the sky function has
        // two callers that must agree exactly — the sky pass draws it, and the
        // water reflects it. Two copies of a gradient that drift apart is a bug
        // that presents as "the water colour is slightly off".
        let common = include_str!("common.wgsl");
        let scene_src = format!("{common}\n{}", include_str!("shader.wgsl"));
        let fullscreen_src = format!("{common}\n{}", include_str!("fullscreen.wgsl"));

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene shader"),
            source: wgpu::ShaderSource::Wgsl(scene_src.into()),
        });
        let fullscreen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen shader"),
            source: wgpu::ShaderSource::Wgsl(fullscreen_src.into()),
        });

        // --- Pipelines -----------------------------------------------------
        let scene_layout = create_scene_layout(&device);

        // Two layouts, and the difference is the whole reason the frame has an
        // offscreen pass. A pipeline that *writes* the scene colour cannot also
        // declare it as a sampled input, so the opaque and wireframe pipelines
        // get a camera-only layout and the blended one gets both groups.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slmsttaa pipeline layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout)],
            immediate_size: 0,
        });
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("slmsttaa scene-sampling pipeline layout"),
                bind_group_layouts: &[Some(&camera_bind_group_layout), Some(&scene_layout)],
                immediate_size: 0,
            });

        // Three scene pipelines: solid (culled triangles), blended (the same,
        // over what's behind it, without writing depth, and able to read it),
        // and wireframe (lines, no culling). Render mode picks the wireframe
        // one; the material picks between the other two, per draw-list run.
        let pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            render_format,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            false,
        );
        let blend_pipeline = create_scene_pipeline(
            &device,
            &scene_pipeline_layout,
            &shader,
            render_format,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            true,
        );
        let line_pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            render_format,
            wgpu::PrimitiveTopology::LineList,
            None,
            false,
        );
        // The sky writes the scene colour, so like the opaque pipeline it must
        // not be able to sample it: camera-only layout.
        let sky_pipeline = create_fullscreen_pipeline(
            &device,
            &pipeline_layout,
            &fullscreen_shader,
            render_format,
            "fs_sky",
            "sky pipeline",
        );
        let composite_pipeline = create_fullscreen_pipeline(
            &device,
            &scene_pipeline_layout,
            &fullscreen_shader,
            render_format,
            "fs_composite",
            "composite pipeline",
        );

        // --- The frame -----------------------------------------------------
        //
        // Declared, not sequenced: `build` resolves the order from what each
        // pass reads and writes. Written in reading order anyway, because a
        // frame that reads top-to-bottom is easier to follow — the point is
        // that the order is no longer load-bearing.
        let mut graph = RenderGraph::new(render_format);
        let scene_color = graph.resource("scene color", ResourceFormat::Color);
        let scene_depth = graph.resource("scene depth", ResourceFormat::Depth);
        let scene_blend = graph.resource("scene blend", ResourceFormat::Color);

        graph.pass(
            Pass::new("sky", PassKind::Sky).writes(scene_color, Load::Clear(wgpu::Color::BLACK)),
        );
        graph.pass(
            Pass::new("opaque", PassKind::Opaque)
                .writes(scene_color, Load::Keep)
                .depth(scene_depth, Load::ClearDepth, true),
        );
        graph.pass(
            Pass::new("composite", PassKind::Composite)
                .reads(&[scene_color])
                .writes(scene_blend, Load::Clear(wgpu::Color::BLACK)),
        );
        graph.pass(
            Pass::new("blended", PassKind::Blended)
                // Reads the depth it is also testing against — legal precisely
                // because it does not write it, which is what `false` declares.
                .reads(&[scene_color, scene_depth])
                .writes(scene_blend, Load::Keep)
                .depth(scene_depth, Load::Keep, false),
        );
        // The only pass that touches the swapchain before the UI does, and the
        // only one that knows where the scene sits on screen. `Clear` is what
        // fills the window around an inset pane, so no clear-colour knob is
        // needed for the surround.
        //
        // It declares `scene_depth` as a read because it genuinely binds it: the
        // scene bind-group layout carries a depth entry that `fs_composite`
        // ignores, and declaring what is bound rather than what is sampled is
        // what keeps the graph's usage derivation from disagreeing with reality.
        graph.pass(
            Pass::new("present", PassKind::Present)
                .reads(&[scene_blend, scene_depth])
                .writes(SWAPCHAIN, Load::Clear(wgpu::Color::BLACK)),
        );
        graph.pass(Pass::new("overlay", PassKind::Overlay).writes(SWAPCHAIN, Load::Keep));

        // Sized to the scene's rectangle, which starts out as the whole window.
        graph.build(&device, config.width, config.height);

        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene sampler"),
            // Clamped: a refraction offset that runs off the edge should smear
            // the edge pixel, not wrap the far side of the screen into a lake.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let scene_bind_group = create_scene_bind_group(
            &device,
            &scene_layout,
            &scene_sampler,
            graph.view(scene_color),
            graph.view(scene_depth),
        );
        // The same layout over the finished scene instead of the opaque one, so
        // the present pass can reuse `composite_pipeline` verbatim. The depth
        // entry is bound because the layout has one, and ignored because
        // `fs_composite` does not mention it.
        let present_bind_group = create_scene_bind_group(
            &device,
            &scene_layout,
            &scene_sampler,
            graph.view(scene_blend),
            graph.view(scene_depth),
        );

        let instance_buffer = create_instance_buffer(&device, INITIAL_INSTANCE_CAPACITY);

        let overlay = Overlay::new(&device, &queue, render_format, width, height);

        // Read once, here, so the rest of the engine only ever sees a `Clock`
        // that reports deltas — pinned or measured, it cannot tell.
        #[cfg(not(target_arch = "wasm32"))]
        let capture = crate::capture::Capture::from_env();
        #[cfg(not(target_arch = "wasm32"))]
        let clock = {
            let mut clock = Clock::new();
            if let Some(dt) = capture.as_ref().and_then(|c| c.dt()) {
                clock.pin(dt);
            }
            clock
        };
        #[cfg(target_arch = "wasm32")]
        let clock = Clock::new();

        Self {
            surface,
            device,
            queue,
            config,
            render_format,
            size,
            pipeline,
            blend_pipeline,
            line_pipeline,
            sky_pipeline,
            composite_pipeline,
            render_mode: RenderMode::default(),
            // No geometry yet — the consumer supplies it in `Application::init`.
            meshes: Vec::new(),
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            draws: Vec::new(),
            graph,
            scene_color,
            scene_depth,
            scene_blend,
            scene_layout,
            scene_sampler,
            scene_bind_group,
            present_bind_group,
            // Fullscreen until a consumer says otherwise, which is what every
            // demo before Slice 19 assumed without being able to say so.
            scene_rect: None,
            scene_rect_px: [0, 0, width.max(1), height.max(1)],
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            input: Input::default(),
            exit_requested: false,
            overlay,
            ui_state: UiState::default(),
            ui_events: Vec::new(),
            clock,
            elapsed: 0.0,
            timeline: Timeline::new(),
            window,
            #[cfg(not(target_arch = "wasm32"))]
            capture,
        }
    }

    /// What this redraw should do, per the screenshot harness.
    ///
    /// Always [`Step::Running`] unless capture mode was asked for. See
    /// [`crate::capture`] for what a frozen frame skips and why.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_step(&mut self) -> crate::capture::Step {
        self.capture
            .as_mut()
            .map_or(crate::capture::Step::Running, |c| c.step())
    }

    /// Tell the capture controller a frame finished, so it can count and, if this
    /// was a checkpoint, freeze.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_end_frame(&mut self) {
        if let Some(capture) = self.capture.as_mut() {
            capture.end_frame();
        }
    }

    /// Physical pixels per logical point, from the window.
    ///
    /// The UI works in points so it looks the same size on every display; this
    /// is the conversion, and the only place the scale factor is consulted.
    fn scale_factor(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Current surface size in physical pixels.
    pub fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.size
    }

    /// Resolve [`Renderer::scene_rect`] to physical pixels, clamped to the
    /// surface.
    ///
    /// Rounds the rect's **edges** rather than its origin and size. Rounding the
    /// two independently lets the right edge drift a pixel as the rect moves,
    /// which is exactly the seam (or overlap) a consumer sees between a panel and
    /// the scene beside it. Integers also sidestep the WebGPU spec's licence to
    /// round a fractional viewport "to some uniform precision" however it likes —
    /// the one place this slice could have diverged native from web.
    fn resolve_scene_rect(&self) -> [u32; 4] {
        let (sw, sh) = (self.size.width.max(1), self.size.height.max(1));
        let Some([x, y, w, h]) = self.scene_rect else {
            return [0, 0, sw, sh];
        };
        let s = self.scale_factor();
        let (fw, fh) = (sw as f32, sh as f32);
        // Clamped because neither wgpu nor the browser will: a viewport hanging
        // off the surface is not an error, it silently draws nothing. A consumer
        // computing its rect from last frame's layout can overshoot for a frame
        // during a resize, and a blank window is a terrible way to find out.
        let x0 = (x * s).round().clamp(0.0, fw - 1.0);
        let y0 = (y * s).round().clamp(0.0, fh - 1.0);
        let x1 = ((x + w) * s).round().clamp(x0 + 1.0, fw);
        let y1 = ((y + h) * s).round().clamp(y0 + 1.0, fh);
        [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32]
    }

    /// Re-resolve the scene rect and bring everything that depends on it in line.
    ///
    /// Called from both places the rect can change — [`Renderer::resize`] and
    /// [`Renderer::set_scene_rect`] — because the two disagreeing is not a subtle
    /// bug: a stale aspect makes spheres oval, and stale textures make the water
    /// sample the wrong pixels.
    ///
    /// Re-allocates only when the *size* changed. A rect that moves without
    /// resizing is free, which is what stops a consumer that recomputes its
    /// layout every frame from recreating three textures every frame.
    fn sync_scene_rect(&mut self) {
        let next = self.resolve_scene_rect();
        let resized = next[2..] != self.scene_rect_px[2..];
        self.scene_rect_px = next;
        self.camera.set_aspect(next[2], next[3]);
        if !resized {
            return;
        }
        self.graph.resize(&self.device, next[2], next[3]);
        // Both bind groups hold *views*, which the re-allocation above replaced.
        self.scene_bind_group = create_scene_bind_group(
            &self.device,
            &self.scene_layout,
            &self.scene_sampler,
            self.graph.view(self.scene_color),
            self.graph.view(self.scene_depth),
        );
        self.present_bind_group = create_scene_bind_group(
            &self.device,
            &self.scene_layout,
            &self.scene_sampler,
            self.graph.view(self.scene_blend),
            self.graph.view(self.scene_depth),
        );
    }

    /// Draw the 3D scene into a rectangle of the window instead of all of it.
    ///
    /// `[x, y, w, h]` in **logical points**, top-left origin — the same units and
    /// the same orientation the UI toolkit lays out in, because that is where the
    /// rect comes from. `None` restores the default, which is the whole window.
    ///
    /// This is what makes the scene *a panel among panels* rather than a
    /// background with UI floating over it. The engine takes over three things
    /// the rect implies and a consumer should not have to redo: the size of every
    /// offscreen target the frame renders through, the camera's aspect ratio, and
    /// the mapping [`Renderer::pointer_ray`] unprojects through.
    ///
    /// The window around the pane is cleared to black. The UI draws over the
    /// whole window afterwards, so a consumer that wants a themed surround paints
    /// the regions outside this rect — which is arithmetic it already did to
    /// arrive at the rect.
    ///
    /// Note it is *points*, not pixels: the scale factor is the engine's business
    /// (see [`Renderer::ui`], which converts in the same direction), and a rect in
    /// points survives a move between displays.
    ///
    /// ```no_run
    /// # use slmsttaa::Renderer;
    /// # fn demo(renderer: &mut Renderer) {
    /// # let (panel_w, margin) = (240.0, 12.0);
    /// let [_, _, w, h] = renderer.scene_rect();
    /// // A pane filling everything to the right of a left-hand panel.
    /// let x = margin + panel_w + margin;
    /// renderer.set_scene_rect(Some([x, margin, w - x - margin, h - 2.0 * margin]));
    /// # }
    /// ```
    pub fn set_scene_rect(&mut self, rect: Option<[f32; 4]>) {
        self.scene_rect = rect;
        self.sync_scene_rect();
    }

    /// The rectangle the scene currently draws into, in logical points.
    ///
    /// Always concrete: with no rect set this is the whole window. It is the
    /// *resolved* rect, so it reflects the clamping and pixel rounding
    /// [`Renderer::set_scene_rect`] applied — read it back to find out where the
    /// scene actually is, rather than assuming the rect you passed survived.
    ///
    /// Do **not** compute the next rect from this one. It is an output, and
    /// feeding it back in is a loop that shrinks the pane a little every frame
    /// until it hits the one-pixel clamp — which is exactly what the first run of
    /// `examples/workspace.rs` did. Lay out against [`Renderer::window_size`].
    pub fn scene_rect(&self) -> [f32; 4] {
        let s = self.scale_factor();
        let [x, y, w, h] = self.scene_rect_px;
        [x as f32 / s, y as f32 / s, w as f32 / s, h as f32 / s]
    }

    /// The window's size in **logical points**, which is what to lay out against.
    ///
    /// The counterpart to [`Renderer::size`], which reports physical pixels. A
    /// consumer positioning panels and a scene pane needs points, because that is
    /// what the UI toolkit measures in and what a theme's margins are stated in —
    /// and converting between the two needs the display's scale factor, which the
    /// engine deliberately keeps to itself.
    ///
    /// This is the same rectangle the toolkit receives as `UiInput::viewport`.
    /// Unlike [`Renderer::scene_rect`] it does not move when the scene is inset,
    /// so it is safe to derive a layout from every frame.
    pub fn window_size(&self) -> [f32; 2] {
        let s = self.scale_factor();
        [self.size.width as f32 / s, self.size.height as f32 / s]
    }

    /// Reconfigure the surface after a window resize.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        // Every attachment must track the size of the scene's rectangle or the
        // render pass fails. The graph owns all of them, so this is one call
        // rather than one per texture — which is half of why it exists, since
        // Slice 16 took the count from one to three.
        //
        // Routed through the same method `set_scene_rect` uses, rather than
        // resizing to `new_size` here: the scene rect is the window only until a
        // consumer says otherwise, and two callers computing it separately is how
        // they come to disagree. A rect stated in points survives a resize
        // unchanged; one derived from the window is restated next frame.
        self.sync_scene_rect();
        // The overlay maps pixels to NDC using the surface size; keep it synced.
        self.overlay
            .resize(&self.queue, new_size.width, new_size.height);
    }

    /// Upload a [`Mesh`] to the GPU and return a handle to it.
    ///
    /// The consumer builds meshes CPU-side and hands them over; the engine owns
    /// the GPU buffers. Uploading is the *expensive* half, so do it once — in
    /// `Application::init`, usually — and then place the result as many times as
    /// you like with [`Renderer::set_instances`], which costs no vertex traffic
    /// at all.
    ///
    /// Handles are never invalidated; see [`MeshHandle`].
    pub fn upload_mesh(&mut self, mesh: &Mesh) -> MeshHandle {
        let gpu = GpuMesh::upload(&self.device, mesh);
        self.meshes.push(gpu);
        MeshHandle(self.meshes.len() - 1)
    }

    /// Replace the geometry behind `handle`, keeping the handle valid.
    ///
    /// For geometry that genuinely *changes shape* — the terrain demo rebuilding
    /// its heightmap when you drag a parameter — as opposed to geometry that only
    /// moves, which wants [`Renderer::set_instances`] instead. Every instance
    /// naming this handle draws the new mesh from the next frame on.
    ///
    /// This reallocates the mesh's buffers rather than writing into them, since
    /// the new mesh may be any size. It is a full upload: don't call it every
    /// frame for something a transform could express.
    ///
    /// # Panics
    ///
    /// If `handle` did not come from this renderer's [`Renderer::upload_mesh`].
    pub fn update_mesh(&mut self, handle: MeshHandle, mesh: &Mesh) {
        assert!(
            handle.0 < self.meshes.len(),
            "mesh handle {:?} is not from this renderer",
            handle,
        );
        self.meshes[handle.0] = GpuMesh::upload(&self.device, mesh);
    }

    /// Upload an RGBA8 image the UI can draw, and return its handle.
    ///
    /// `rgba` is `width * height` straight-alpha texels, row-major, top row
    /// first. Draw it with [`Painter::image`](slmsttaa_ui::Painter::image) or
    /// [`image_full`](slmsttaa_ui::Painter::image_full) from anywhere a
    /// [`Ui`](slmsttaa_ui::Ui) hands out its painter.
    ///
    /// The bytes are read the way a [`Color`] is — see
    /// [`Ui`](slmsttaa_ui::Painter::image) for what `uv` and `tint` mean, and
    /// note the one restriction that follows from the overlay being a single
    /// draw call: **one image per frame**.
    ///
    /// Images are never freed. A picture that changes wants
    /// [`Renderer::update_image`], not a second handle.
    ///
    /// # Panics
    ///
    /// If the size is zero, if `rgba` is not exactly `width * height * 4` bytes,
    /// or if either dimension exceeds this device's texture limit.
    pub fn create_image(&mut self, width: u32, height: u32, rgba: &[u8]) -> ImageId {
        self.overlay
            .create_image(&self.device, &self.queue, width, height, rgba)
    }

    /// Rewrite an image's pixels, keeping the handle valid.
    ///
    /// Same size as it was created at — a thumbnail that regenerates every pass
    /// wants one texture rewritten, not one per pass that nothing can free.
    ///
    /// # Panics
    ///
    /// If `id` did not come from this renderer, or `rgba` is the wrong length.
    pub fn update_image(&mut self, id: ImageId, rgba: &[u8]) {
        self.overlay.update_image(&self.queue, id, rgba);
    }

    /// Replace the draw-list: what to draw, and where.
    ///
    /// One [`MeshHandle`] may appear any number of times at different
    /// [`Transform`]s — that is the whole point, and the engine batches the
    /// repeats into a single instanced draw call per mesh. The list is *retained*
    /// until replaced, so static scenes set it once in `Application::init` and a
    /// moving one re-sends it each frame. Re-sending is cheap: it writes one
    /// matrix per instance, and touches no vertex or index buffer.
    ///
    /// # Panics
    ///
    /// If any instance names a handle this renderer didn't issue.
    pub fn set_instances(&mut self, instances: &[Instance]) {
        self.draws.clear();
        if instances.is_empty() {
            return;
        }

        // Group by mesh so each mesh becomes one instanced draw, and put every
        // opaque instance before every transparent one — blending only composites
        // correctly over a target that is already finished. Sorting is stable, so
        // a consumer's own ordering survives within a group.
        //
        // Transparent instances are *not* sorted against each other by depth. One
        // non-overlapping surface (terrain's water) doesn't need it, and a general
        // back-to-front sort should wait for a demo that actually does.
        let mut sorted: Vec<&Instance> = instances.iter().collect();
        sorted.sort_by_key(|i| (i.material.is_transparent(), i.mesh.0));

        let mut raw: Vec<InstanceRaw> = Vec::with_capacity(sorted.len());
        for instance in &sorted {
            assert!(
                instance.mesh.0 < self.meshes.len(),
                "instance names mesh handle {:?}, which is not from this renderer",
                instance.mesh,
            );
            let transparent = instance.material.is_transparent();
            // Extend the run in progress, or start a new one. A run needs one
            // mesh *and* one pipeline, so either changing breaks it — the same
            // mesh may legitimately appear in both an opaque and a blended run.
            match self.draws.last_mut() {
                Some(draw) if draw.mesh == instance.mesh.0 && draw.transparent == transparent => {
                    draw.count += 1
                }
                _ => self.draws.push(InstanceDraw {
                    mesh: instance.mesh.0,
                    byte_offset: (raw.len() * std::mem::size_of::<InstanceRaw>())
                        as wgpu::BufferAddress,
                    count: 1,
                    transparent,
                }),
            }
            raw.push(InstanceRaw::from_instance(instance));
        }

        // Grow the buffer if this frame needs more room than the last one did.
        // It is only ever reallocated upward, so a scene with a steady object
        // count settles after one frame and never allocates again.
        if raw.len() > self.instance_capacity {
            self.instance_capacity = raw.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&raw));
    }

    /// Choose whether meshes draw filled or as a wireframe (see [`RenderMode`]).
    ///
    /// Cheap to flip every frame: both pipelines and both index buffers already
    /// exist, so this only changes which are bound — no geometry is re-uploaded.
    pub fn set_render_mode(&mut self, mode: RenderMode) {
        self.render_mode = mode;
    }

    /// The current [`RenderMode`].
    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    /// Mutable access to the camera so the consumer can drive the viewpoint.
    ///
    /// Move `eye`/`target` (or change `fov_y`) from `Application::update`; the
    /// next [`Renderer::update`] re-uploads the view-projection matrix. The
    /// aspect ratio is owned by the engine and resynced on resize — leave it be.
    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// This frame's input snapshot, for reading from `Application::update`.
    ///
    /// Held keys/buttons persist across frames; mouse and scroll deltas cover
    /// only the current frame (see [`Input`]).
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// Ask the engine to close the window and end the run.
    ///
    /// The request is honoured at the *end* of the current frame, so the frame
    /// you ask on is still drawn. Calling it more than once is harmless.
    ///
    /// This exists because [`Application::quit_on_escape`] can be turned off: a
    /// consumer that takes Escape for its own UI would otherwise have no way to
    /// quit but the window's close button.
    ///
    /// [`Application::quit_on_escape`]: crate::Application::quit_on_escape
    pub fn request_exit(&mut self) {
        self.exit_requested = true;
    }

    /// Whether [`Renderer::request_exit`] has been called this frame.
    pub fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    /// A world-space [`Ray`] through the pointer, or `None` when the pointer is
    /// not over the window.
    ///
    /// The engine's whole contribution to picking. Turning a cursor into a ray
    /// needs three things a consumer cannot reach: the camera's inverse
    /// view-projection, the rectangle the scene occupies in pixels, and the
    /// display scale factor relating the two. Everything *after* the ray — what
    /// counts as a hit, which hit wins, what a click then means — is the
    /// consumer's, in the same way the erosion solver is the terrain demo's. The
    /// engine does not know what is in the scene, and this is the seam that lets
    /// it keep not knowing.
    ///
    /// It is cast through [`Renderer::scene_rect`], so an inset scene picks
    /// correctly; before Slice 19 this unprojected through the whole window,
    /// which was indistinguishable from correct while the two were the same
    /// rectangle.
    ///
    /// A cursor **outside** that rect still gets a ray, and deliberately so. The
    /// projection is perfectly well defined out there, and it is what keeps a
    /// drag continuous when the pointer strays over a panel — the same courtesy
    /// the toolkit extends to a slider dragged off itself. Whether a click that
    /// far out should count is a policy question, and policy is the consumer's
    /// here for the same reason the bounding box is: ask `Ui::wants_pointer`, or
    /// test the cursor against `scene_rect` yourself.
    ///
    /// It reads the camera as it stands *now*, so a consumer that moves the camera
    /// and then picks in the same frame gets the moved one.
    ///
    /// ```no_run
    /// # use slmsttaa::Renderer;
    /// # fn demo(renderer: &Renderer) {
    /// if let Some(ray) = renderer.pointer_ray() {
    ///     // Meet the ground plane at y = 0.
    ///     let t = -ray.origin[1] / ray.direction[1];
    ///     let ground = ray.at(t);
    /// }
    /// # }
    /// ```
    pub fn pointer_ray(&self) -> Option<Ray> {
        let cursor = self.input.cursor_position()?;
        let ndc = ndc_in_rect(cursor, self.scene_rect_px)?;
        Some(self.camera.ray_through_ndc(ndc))
    }

    /// Whether the pointer is over the rectangle the scene draws into.
    ///
    /// The companion to [`Renderer::pointer_ray`], and the reason that one does
    /// not decide for you: "is this click meant for the scene?" is a policy
    /// question, but *answering* it needs two things a consumer cannot reach —
    /// the cursor in physical pixels and the resolved rect in the same units, one
    /// scale factor apart from the points [`Renderer::scene_rect`] reports. So the
    /// engine supplies the fact and the consumer keeps the decision.
    ///
    /// False when the pointer has left the window entirely.
    ///
    /// ```no_run
    /// # use slmsttaa::Renderer;
    /// # fn demo(renderer: &Renderer, ui_wants_pointer: bool) {
    /// if renderer.pointer_in_scene() && !ui_wants_pointer {
    ///     if let Some(ray) = renderer.pointer_ray() {
    ///         // ...pick with it.
    ///         let _ = ray;
    ///     }
    /// }
    /// # }
    /// ```
    pub fn pointer_in_scene(&self) -> bool {
        let Some(ndc) = self
            .input
            .cursor_position()
            .and_then(|c| ndc_in_rect(c, self.scene_rect_px))
        else {
            return false;
        };
        // Inside the rect *is* inside NDC, which saves converting the cursor into
        // points and comparing against a rect that was rounded to pixels.
        (-1.0..=1.0).contains(&ndc[0]) && (-1.0..=1.0).contains(&ndc[1])
    }

    /// Seconds of **wall time** elapsed since the previous frame. Updated once
    /// per frame by the engine before `Application::update` runs; clamped to a
    /// sane maximum (see [`Clock`]).
    ///
    /// This is the clock for things that should keep moving while the simulation
    /// is stopped — an FPS readout, a UI hover fade. Simulation state should be
    /// advanced by the fixed step handed to `Application::fixed_update` instead;
    /// see [`Renderer::time`] for why.
    pub fn dt(&self) -> f32 {
        self.clock.dt()
    }

    /// The fixed-timestep simulation clock: elapsed simulation time, the
    /// interpolation alpha, and the step count the last frame ran.
    ///
    /// The counterpart to [`Renderer::dt`]. Where that reports how long the last
    /// frame took — a different number on every machine — this one is paid out in
    /// identical steps, which is what lets a run reproduce and what makes "pause"
    /// and "one step, please" expressible at all.
    pub fn time(&self) -> &Timeline {
        &self.timeline
    }

    /// Drive the simulation clock: pause, time scale, single-step, seek, and the
    /// step rate. See [`Timeline`].
    ///
    /// Mirrors [`Renderer::camera_mut`] — a handle rather than a spray of setters
    /// on this type, because these five controls are one subject.
    pub fn time_mut(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    /// Begin a UI frame and return the immediate-mode [`Ui`] builder.
    ///
    /// Call this from `Application::update`, declare your panels, then read
    /// [`Ui::changed`]. The widgets draw into the overlay (composited over the
    /// 3D scene by [`Renderer::render`]) and read this frame's [`Input`]. The
    /// returned `Ui` borrows the renderer mutably, so drive the camera first.
    ///
    /// This is where the two halves meet. The toolkit lives in its own crate and
    /// cannot see [`Input`] (that would be a dependency cycle — see
    /// `slmsttaa-ui/README.md`), so the engine copies this frame's host state
    /// into the toolkit's own [`UiInput`] snapshot. Seven assignments and one
    /// `match`, in exchange for a UI crate that has no dependencies at all.
    pub fn ui(&mut self) -> Ui<'_> {
        // Restate this frame's keyboard log in the toolkit's vocabulary. The two
        // `Key` enums are deliberately separate declarations rather than a shared
        // one — the toolkit imports nothing, including from us — and the `match`
        // in `ui_key` is exhaustive, so a variant added on one side is a compile
        // error rather than a key that silently stops working.
        self.ui_events.clear();
        for event in self.input.events() {
            self.ui_events.push(match *event {
                crate::input::Event::Text(ch) => slmsttaa_ui::Event::Text(ch),
                crate::input::Event::Key(key) => slmsttaa_ui::Event::Key(slmsttaa_ui::KeyEvent {
                    key: ui_key(key.key),
                    pressed: key.pressed,
                    repeat: key.repeat,
                    modifiers: ui_modifiers(key.modifiers),
                }),
            });
        }

        // The toolkit works in logical points, so the cursor is converted on the
        // way in and the overlay converts back on the way out. Without this the
        // UI is half-size (and mis-hit) on a 2× display.
        let scale = self.scale_factor();
        let input = UiInput {
            events: &self.ui_events,
            modifiers: ui_modifiers(self.input.modifiers()),
            cursor: self
                .input
                .cursor_position()
                .map(|(x, y)| (x / scale, y / scale)),
            primary_held: self.input.is_mouse_held(MouseButton::Left),
            primary_pressed: self.input.is_mouse_pressed(MouseButton::Left),
            // Scroll is shared with whatever else the consumer does with the
            // wheel; `Ui::wants_pointer` is how it decides who gets it.
            scroll_delta: self.input.scroll_delta(),
            // Points, for the same reason: a right-anchored panel measures from
            // the window's right edge, and that edge has to mean the same thing
            // as the metrics in `theme`.
            viewport: (
                self.size.width as f32 / scale,
                self.size.height as f32 / scale,
            ),
            // The toolkit owns no clock, so its hover fades and collapse
            // transitions run on ours. `begin_frame` has already ticked, so this
            // is the delta the consumer's `update` is seeing.
            dt: self.clock.dt(),
        };
        self.overlay.set_scale(scale);
        Ui::new(&mut self.overlay, input, &mut self.ui_state)
    }

    /// Advance both clocks, reset per-frame overlay geometry, and report how many
    /// fixed steps this frame owes the consumer.
    ///
    /// Engine-internal: the event loop calls this at the start of each frame, so
    /// the consumer's hooks see a fresh [`Renderer::dt`] and an empty overlay to
    /// rebuild its UI into. The returned count is how many times the loop should
    /// call `Application::fixed_update` before `Application::update`.
    ///
    /// The timeline is fed the *clamped* delta, so a stall that [`Clock`] already
    /// capped cannot arrive here as a hundred queued steps.
    pub(crate) fn begin_frame(&mut self) -> u32 {
        let dt = self.clock.tick();
        self.elapsed += dt;
        self.overlay.begin_frame();
        self.timeline.begin_frame(dt)
    }

    /// Mutable access to the input snapshot, for the event loop to feed events
    /// and to clear per-frame deltas. Engine-internal: consumers use
    /// [`Renderer::input`].
    pub(crate) fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    /// Advance per-frame state (camera animation, etc.).
    pub fn update(&mut self) {
        // Carry any copy or cut the UI asked for out to the operating system.
        // The toolkit has no dependencies and so cannot do this itself; it leaves
        // the text in `UiState` and the engine collects it once a frame. The
        // inbound direction needs no counterpart — a paste arrives as ordinary
        // typed characters (see `app::App::window_event`).
        if let Some(text) = self.ui_state.take_clipboard() {
            crate::clipboard::set(&text);
        }

        self.camera_uniform = CameraUniform::new(&self.camera, self.elapsed);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
    }

    /// Render one frame to the surface.
    ///
    /// Recoverable surface conditions (timeout, occlusion, outdated/lost) are
    /// handled here by skipping the frame and reconfiguring as needed, so the
    /// caller doesn't have to.
    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // Transient: just try again next frame.
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            // The surface needs reconfiguring; do it and skip this frame.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("surface acquire failed validation; skipping frame");
                return;
            }
        };

        // Rendered through `render_format`, which on a WebGPU canvas is an sRGB
        // re-view of a linear surface texture. `Default::default()` would take
        // the texture's own format and silently skip the encode.
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        // Read once, before the graph is borrowed. The offscreen targets are
        // already this size — this is only where on the swapchain they land.
        let [rx, ry, rw, rh] = self.scene_rect_px;

        // The graph resolved the order; this just records what it hands back.
        // Adding a pass means declaring what it touches over in `new`, not
        // finding the right place to slot it into this loop.
        for target in self.graph.record(&view) {
            // The overlay records its own pass (it owns its pipeline and
            // buffers), so it is dispatched before a pass is opened here.
            if target.kind == PassKind::Overlay {
                self.overlay
                    .flush(&self.device, &self.queue, &mut encoder, &view);
                continue;
            }

            let color = target
                .color
                .map(|(view, load)| wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match load {
                            Load::Clear(c) => wgpu::LoadOp::Clear(c),
                            _ => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                });
            let depth =
                target.depth.map(
                    |(view, load, write)| wgpu::RenderPassDepthStencilAttachment {
                        view,
                        // `None` is how wgpu spells a *read-only* depth attachment,
                        // and it is what makes the blended pass legal: a pass may
                        // sample the same depth texture it is testing against only
                        // if it cannot write to it.
                        depth_ops: write.then_some(wgpu::Operations {
                            load: match load {
                                Load::ClearDepth => wgpu::LoadOp::Clear(1.0),
                                _ => wgpu::LoadOp::Load,
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                );

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(target.label),
                color_attachments: &[color],
                depth_stencil_attachment: depth,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            match target.kind {
                PassKind::Sky => {
                    pass.set_pipeline(&self.sky_pipeline);
                    pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    // Three vertices, no buffers: the shader builds the triangle.
                    pass.draw(0..3, 0..1);
                }
                PassKind::Composite => {
                    pass.set_pipeline(&self.composite_pipeline);
                    pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    pass.set_bind_group(1, &self.scene_bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                PassKind::Present => {
                    // The frame's one viewport call, and the whole of "the scene
                    // is a panel". Everything upstream rendered into targets
                    // whose extent *is* the camera's frame; this decides where
                    // that frame lands on the swapchain. Clipping confines the
                    // oversized fullscreen triangle to the rect, so no scissor is
                    // needed — and the load-op clear that fills the surround
                    // happened before this pass, unaffected by either.
                    pass.set_viewport(rx as f32, ry as f32, rw as f32, rh as f32, 0.0, 1.0);
                    pass.set_pipeline(&self.composite_pipeline);
                    pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    pass.set_bind_group(1, &self.present_bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                PassKind::Opaque | PassKind::Blended => {
                    self.record_draws(&mut pass, target.kind);
                }
                PassKind::Overlay => unreachable!("handled above"),
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    /// Record the half of the draw-list that belongs to `kind`.
    ///
    /// Opaque and blended runs are already contiguous — `set_instances` sorts
    /// every opaque run ahead of every transparent one — so this is a filter over
    /// a sorted list rather than two lists.
    fn record_draws(&self, pass: &mut wgpu::RenderPass<'_>, kind: PassKind) {
        let wireframe = self.render_mode == RenderMode::Wireframe;
        // Wireframe draws *everything* as opaque lines in the opaque pass: an
        // edge list has no interior to see through, so blending it would only
        // make the inspection view harder to read, and refracting it would be
        // meaningless. The blended pass therefore has nothing to do.
        if wireframe && kind == PassKind::Blended {
            return;
        }

        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        if kind == PassKind::Blended {
            pass.set_bind_group(1, &self.scene_bind_group, &[]);
            pass.set_pipeline(&self.blend_pipeline);
        } else if wireframe {
            pass.set_pipeline(&self.line_pipeline);
        } else {
            pass.set_pipeline(&self.pipeline);
        }

        for draw in &self.draws {
            // In wireframe every run goes in the opaque pass; otherwise a run is
            // in exactly one of the two.
            if !wireframe && draw.transparent != (kind == PassKind::Blended) {
                continue;
            }
            let mesh = &self.meshes[draw.mesh];
            let (buffer, count) = if wireframe {
                (&mesh.line_index_buffer, mesh.line_index_count)
            } else {
                (&mesh.index_buffer, mesh.index_count)
            };
            // An empty mesh is a legal thing to hold a handle to; it just has
            // nothing to draw.
            if count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            // The run's offset rides on the *buffer binding*, and the draw always
            // starts at instance 0 — WebGL2 has no non-zero `first_instance`, so
            // the obvious `draw_indexed(.., first..last)` would work on native and
            // fail in the browser fallback.
            pass.set_vertex_buffer(1, self.instance_buffer.slice(draw.byte_offset..));
            pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..count, 0, 0..draw.count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mapping the whole of picking rests on, checked at the corners where
    /// an off-by-a-rectangle shows and the centre where it does not.
    #[test]
    fn a_fullscreen_rect_maps_the_window_to_ndc() {
        let rect = [0, 0, 800, 600];
        assert_eq!(ndc_in_rect((400.0, 300.0), rect), Some([0.0, 0.0]));
        // Top-left of the window is NDC (-1, +1): y flips.
        assert_eq!(ndc_in_rect((0.0, 0.0), rect), Some([-1.0, 1.0]));
        assert_eq!(ndc_in_rect((800.0, 600.0), rect), Some([1.0, -1.0]));
    }

    /// The Slice 19 bug, stated as a test: with the scene inset, the *rect's*
    /// centre is NDC zero and the *window's* centre is not.
    ///
    /// Before this slice `pointer_ray` divided by the surface, so the second
    /// assertion below would have been `[0.0, 0.0]` — a ray through the middle
    /// of the picture pointing somewhere else entirely.
    #[test]
    fn an_inset_rect_is_measured_from_its_own_corner() {
        // A 400×300 pane sitting at (200, 100) in an 800×600 window.
        let rect = [200, 100, 400, 300];
        assert_eq!(ndc_in_rect((400.0, 250.0), rect), Some([0.0, 0.0]));
        assert_eq!(ndc_in_rect((200.0, 100.0), rect), Some([-1.0, 1.0]));
        assert_eq!(ndc_in_rect((600.0, 400.0), rect), Some([1.0, -1.0]));
        // The window's centre is *not* the pane's centre, and this is the whole
        // difference between the old mapping and the new one.
        assert_ne!(ndc_in_rect((400.0, 300.0), rect), Some([0.0, 0.0]));
    }

    /// A cursor outside the pane still projects, because a drag that wanders
    /// over a panel should keep tracking rather than freeze.
    #[test]
    fn a_cursor_outside_the_rect_still_projects() {
        let rect = [200, 100, 400, 300];
        let ndc = ndc_in_rect((0.0, 250.0), rect).expect("outside is not degenerate");
        assert!(ndc[0] < -1.0, "left of the pane is left of NDC: {ndc:?}");
        assert_eq!(ndc[1], 0.0, "and vertically unchanged");
    }

    /// A collapsed pane has no sensible centre, so there is no ray to give.
    #[test]
    fn a_degenerate_rect_has_no_ray() {
        assert_eq!(ndc_in_rect((10.0, 10.0), [0, 0, 0, 600]), None);
        assert_eq!(ndc_in_rect((10.0, 10.0), [0, 0, 800, 0]), None);
    }
}
