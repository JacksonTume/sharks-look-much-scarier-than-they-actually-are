//! The wgpu rendering backend.
//!
//! [`Renderer`] owns the GPU surface, device/queue, and the demo render
//! pipeline. It is deliberately small: enough to clear the screen and draw a
//! camera-transformed mesh, with clear seams where a real engine would grow
//! (material system, mesh registry, render graph, etc.).

mod vertex;

pub use vertex::Vertex;

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::camera::{Camera, CameraUniform};

/// A simple colored triangle, defined in counter-clockwise winding order.
const DEMO_VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.5, 0.0],
        color: [1.0, 0.2, 0.3],
    },
    Vertex {
        position: [-0.5, -0.5, 0.0],
        color: [0.2, 1.0, 0.4],
    },
    Vertex {
        position: [0.5, -0.5, 0.0],
        color: [0.3, 0.4, 1.0],
    },
];

/// Holds all GPU state required to render a frame.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,

    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("slmsttaa render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // No culling for the demo: a single flat triangle should be
                // visible from either side regardless of projected winding.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("demo vertex buffer"),
            contents: bytemuck::cast_slice(DEMO_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            surface,
            device,
            queue,
            config,
            size,
            pipeline,
            vertex_buffer,
            vertex_count: DEMO_VERTICES.len() as u32,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
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
        self.camera.set_aspect(new_size.width, new_size.height);
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
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}
