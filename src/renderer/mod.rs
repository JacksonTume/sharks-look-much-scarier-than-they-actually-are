//! The wgpu rendering backend.
//!
//! [`Renderer`] owns the GPU surface, device/queue, and the render pipeline. It
//! is deliberately small: enough to clear the screen and draw a
//! camera-transformed mesh supplied by the consumer, with clear seams where a
//! real engine would grow (material system, mesh registry, render graph, etc.).

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

use crate::camera::{Camera, CameraUniform};
use crate::input::{Input, MouseButton};
use crate::time::{Clock, Timeline};
use overlay::Overlay;
use slmsttaa_ui::{Ui, UiInput, UiState};

/// Format of the depth buffer used for depth testing.
///
/// `Depth32Float` is a render-attachment format on every backend we target,
/// including the WebGL2 fallback. (If a future GL adapter ever rejects it, switch
/// to `Depth24Plus` — both the texture and the pipeline read this one constant.)
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

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

/// Create an instance buffer with room for `capacity` instances.
fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance buffer"),
        size: (capacity * std::mem::size_of::<InstanceRaw>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Create a depth texture sized to the surface and return its default view.
///
/// Must be called whenever the surface is (re)configured: the depth attachment
/// has to match the color target's dimensions exactly or the render pass fails.
fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        // Must match the pipeline's 1-sample `MultisampleState`.
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
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
/// thing a see-through surface must not do.
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
            entry_point: Some("fs_main"),
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
    size: winit::dpi::PhysicalSize<u32>,

    pipeline: wgpu::RenderPipeline,
    /// Alpha-blended variant of [`Renderer::pipeline`], with depth writes off.
    /// Selected for draw-list runs whose material alpha is below `1.0`.
    blend_pipeline: wgpu::RenderPipeline,
    /// Wireframe variant of [`Renderer::pipeline`] (line topology, no culling),
    /// selected when the render mode is [`RenderMode::Wireframe`].
    line_pipeline: wgpu::RenderPipeline,
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
    /// Depth attachment for occlusion testing; resized with the surface.
    depth_view: wgpu::TextureView,

    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

    /// This frame's input snapshot. The event loop feeds it; the consumer reads
    /// it via [`Renderer::input`] from `Application::update`.
    input: Input,

    /// The screen-space overlay pass (2D UI/HUD), drawn after the 3D scene.
    overlay: Overlay,
    /// Persistent immediate-mode UI state (active widget, panel height).
    ui_state: UiState,
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
        // Prefer an sRGB surface format so colors look correct without manual
        // gamma handling in the shader.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            // `AutoVsync` avoids tearing and keeps the GPU from melting; switch
            // to `AutoNoVsync` to benchmark uncapped frame rates.
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
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

        // --- Pipeline ------------------------------------------------------
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slmsttaa pipeline layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout)],
            immediate_size: 0,
        });

        // Three pipelines sharing one shader/layout: solid (culled triangles),
        // blended (the same, over what's behind it, without writing depth), and
        // wireframe (lines, no culling). Render mode picks the wireframe one; the
        // material's alpha picks between the other two, per draw-list run.
        let pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            false,
        );
        let blend_pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            true,
        );
        let line_pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::LineList,
            None,
            false,
        );

        let depth_view = create_depth_view(&device, &config);

        let instance_buffer = create_instance_buffer(&device, INITIAL_INSTANCE_CAPACITY);

        let overlay = Overlay::new(&device, &queue, config.format, width, height);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            pipeline,
            blend_pipeline,
            line_pipeline,
            render_mode: RenderMode::default(),
            // No geometry yet — the consumer supplies it in `Application::init`.
            meshes: Vec::new(),
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            draws: Vec::new(),
            depth_view,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            input: Input::default(),
            overlay,
            ui_state: UiState::default(),
            clock: Clock::new(),
            elapsed: 0.0,
            timeline: Timeline::new(),
            window,
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

    /// Reconfigure the surface after a window resize.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        // The depth buffer must track the surface size or the render pass fails.
        self.depth_view = create_depth_view(&self.device, &self.config);
        self.camera.set_aspect(new_size.width, new_size.height);
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
    /// into the toolkit's own [`UiInput`] snapshot. Five assignments, in
    /// exchange for a UI crate that has no dependencies at all.
    pub fn ui(&mut self) -> Ui<'_> {
        // The toolkit works in logical points, so the cursor is converted on the
        // way in and the overlay converts back on the way out. Without this the
        // UI is half-size (and mis-hit) on a 2× display.
        let scale = self.scale_factor();
        let input = UiInput {
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

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.05,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // Clear to the far plane each frame before drawing.
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            // Draw the consumer's draw-list, one instanced call per mesh in it;
            // if it's empty the pass above still clears the screen.
            if !self.draws.is_empty() {
                let wireframe = self.render_mode == RenderMode::Wireframe;
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                // Which pipeline was bound last, so a run only rebinds when it
                // has to. `None` until the first run binds one.
                let mut bound: Option<bool> = None;
                for draw in &self.draws {
                    let mesh = &self.meshes[draw.mesh];
                    let (buffer, count) = if wireframe {
                        (&mesh.line_index_buffer, mesh.line_index_count)
                    } else {
                        (&mesh.index_buffer, mesh.index_count)
                    };
                    // An empty mesh is a legal thing to hold a handle to; it just
                    // has nothing to draw.
                    if count == 0 {
                        continue;
                    }
                    // Wireframe draws every run as opaque lines: an edge list has
                    // no interior to see through, so blending it would only make
                    // the inspection view harder to read.
                    let blended = draw.transparent && !wireframe;
                    if bound != Some(blended) {
                        pass.set_pipeline(match (wireframe, blended) {
                            (true, _) => &self.line_pipeline,
                            (false, true) => &self.blend_pipeline,
                            (false, false) => &self.pipeline,
                        });
                        bound = Some(blended);
                    }
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    // The run's offset rides on the *buffer binding*, and the draw
                    // always starts at instance 0 — WebGL2 has no non-zero
                    // `first_instance`, so the obvious `draw_indexed(.., first..last)`
                    // would work on native and fail in the browser fallback.
                    pass.set_vertex_buffer(1, self.instance_buffer.slice(draw.byte_offset..));
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, 0..draw.count);
                }
            }
        }

        // Second pass: composite the 2D overlay (UI/HUD) over the 3D scene. This
        // records its own render pass that loads (rather than clears) the color
        // target. It no-ops if the consumer drew no UI this frame.
        self.overlay
            .flush(&self.device, &self.queue, &mut encoder, &view);

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}
