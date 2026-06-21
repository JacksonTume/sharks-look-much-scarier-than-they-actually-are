//! The wgpu rendering backend.
//!
//! [`Renderer`] owns the GPU surface, device/queue, and the render pipeline. It
//! is deliberately small: enough to clear the screen and draw a
//! camera-transformed mesh supplied by the consumer, with clear seams where a
//! real engine would grow (material system, mesh registry, render graph, etc.).

mod font;
mod mesh;
mod overlay;
mod vertex;

pub use mesh::Mesh;
pub use vertex::Vertex;

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::{Camera, CameraUniform};
use crate::input::Input;
use crate::time::Clock;
use crate::ui::{Ui, UiState};
use overlay::Overlay;

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
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh index buffer"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        // Derive a deduplicated edge list once, so wireframe is a buffer swap.
        let line_indices = edge_indices(&mesh.indices);
        let line_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh line index buffer"),
            contents: bytemuck::cast_slice(&line_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
            line_index_buffer,
            line_index_count: line_indices.len() as u32,
        }
    }
}

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
/// The solid and wireframe pipelines are identical except for topology and
/// culling: triangles cull their back faces, lines never cull. Both share the
/// camera-transform shader, the color target, and depth testing, so wireframe
/// edges occlude correctly against solid geometry.
fn create_scene_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("slmsttaa scene pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
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
            depth_write_enabled: Some(true),
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
    /// Wireframe variant of [`Renderer::pipeline`] (line topology, no culling),
    /// selected when the render mode is [`RenderMode::Wireframe`].
    line_pipeline: wgpu::RenderPipeline,
    /// Whether meshes draw filled or as edges. Defaults to [`RenderMode::Solid`].
    render_mode: RenderMode,
    /// The draw-list: every mesh the consumer has handed over via
    /// [`Renderer::set_meshes`]. Empty until set; the engine just clears the
    /// screen until then.
    meshes: Vec<GpuMesh>,
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
    /// Frame clock for delta-time and the FPS readout.
    clock: Clock,

    /// Keep the window alive for as long as the surface borrows it.
    _window: Arc<Window>,
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
        let camera_uniform = CameraUniform::from_camera(&camera);

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
                    visibility: wgpu::ShaderStages::VERTEX,
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

        // Two pipelines sharing one shader/layout: solid (culled triangles) and
        // wireframe (lines, no culling). Render mode just selects between them.
        let pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
        );
        let line_pipeline = create_scene_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::LineList,
            None,
        );

        let depth_view = create_depth_view(&device, &config);

        let overlay = Overlay::new(&device, &queue, config.format, width, height);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            pipeline,
            line_pipeline,
            render_mode: RenderMode::default(),
            // No geometry yet — the consumer supplies it in `Application::init`.
            meshes: Vec::new(),
            depth_view,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            input: Input::default(),
            overlay,
            ui_state: UiState::default(),
            clock: Clock::new(),
            _window: window,
        }
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

    /// Replace the draw-list with `meshes`, uploading each to the GPU.
    ///
    /// The consumer builds [`Mesh`]es CPU-side and hands them over; the engine
    /// owns the GPU buffers. This *replaces* the previous draw-list, so calling
    /// it every frame (to animate geometry) re-uploads rather than accumulating.
    pub fn set_meshes(&mut self, meshes: &[Mesh]) {
        // Build into a local first so we aren't borrowing `self.device` while
        // assigning into `self.meshes`.
        let gpu: Vec<GpuMesh> = meshes
            .iter()
            .map(|mesh| GpuMesh::upload(&self.device, mesh))
            .collect();
        self.meshes = gpu;
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

    /// Seconds elapsed since the previous frame (delta-time), for frame-rate-
    /// independent animation. Updated once per frame by the engine before
    /// `Application::update` runs; clamped to a sane maximum (see [`Clock`]).
    pub fn dt(&self) -> f32 {
        self.clock.dt()
    }

    /// Begin a UI frame and return the immediate-mode [`Ui`] builder.
    ///
    /// Call this from `Application::update`, declare your panel's widgets, then
    /// read [`Ui::changed`]. The widgets draw into the overlay (composited over
    /// the 3D scene by [`Renderer::render`]) and read this frame's [`Input`].
    /// The returned `Ui` borrows the renderer mutably, so drive the camera first.
    pub fn ui(&mut self) -> Ui<'_> {
        Ui::new(&mut self.overlay, &self.input, &mut self.ui_state)
    }

    /// Advance the frame clock and reset per-frame overlay geometry.
    ///
    /// Engine-internal: the event loop calls this at the start of each frame,
    /// before `Application::update`, so the consumer's `update` sees a fresh
    /// [`Renderer::dt`] and an empty overlay to rebuild its UI into.
    pub(crate) fn begin_frame(&mut self) {
        self.clock.tick();
        self.overlay.begin_frame();
    }

    /// Mutable access to the input snapshot, for the event loop to feed events
    /// and to clear per-frame deltas. Engine-internal: consumers use
    /// [`Renderer::input`].
    pub(crate) fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    /// Advance per-frame state (camera animation, etc.).
    pub fn update(&mut self) {
        self.camera_uniform = CameraUniform::from_camera(&self.camera);
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

            // Draw every mesh in the consumer's draw-list; if it's empty the pass
            // above still clears the screen.
            if !self.meshes.is_empty() {
                let wireframe = self.render_mode == RenderMode::Wireframe;
                pass.set_pipeline(if wireframe {
                    &self.line_pipeline
                } else {
                    &self.pipeline
                });
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                for mesh in &self.meshes {
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    let (buffer, count) = if wireframe {
                        (&mesh.line_index_buffer, mesh.line_index_count)
                    } else {
                        (&mesh.index_buffer, mesh.index_count)
                    };
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, 0..1);
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
