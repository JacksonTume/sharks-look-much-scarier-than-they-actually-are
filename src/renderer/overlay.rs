//! The screen-space overlay pass: 2D rectangles and text drawn on top of the
//! 3D scene.
//!
//! This is the engine's first *second pass* (roadmap Slice 5). After the main
//! 3D pass renders the world, [`Overlay::flush`] records a second render pass
//! that draws accumulated 2D primitives in pixel coordinates — orthographic,
//! depth-test off, alpha-blended over the scene. It renders identically on native
//! and web, which is the whole point: it gives both targets real on-screen UI,
//! unlike the gallery's DOM-button hack which only exists in a browser.
//!
//! The overlay owns a glyph atlas baked from the embedded [`font`](super::font)
//! bitmap and exposes a tiny CPU draw API ([`Painter`]); the
//! [`Ui`](crate::ui::Ui) immediate-mode layer is built on top of it, but a
//! consumer could also drive it directly. Primitives are accumulated into CPU
//! vectors each frame (cleared by [`Overlay::begin_frame`]) and uploaded in one
//! shot at flush time.

use wgpu::util::DeviceExt;

use super::font;
use crate::ui::{Color, Painter};

/// A 2D overlay vertex: pixel position, atlas UV, and RGBA tint.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex2D {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Vertex2D {
    const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex2D>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// The screen-size uniform (pixels), padded to 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    _pad: [f32; 2],
}

// --- Atlas layout ----------------------------------------------------------

/// Glyph cells per row in the atlas.
const ATLAS_COLS: usize = 16;
/// Rows needed to hold every glyph.
const ATLAS_ROWS: usize = font::COUNT.div_ceil(ATLAS_COLS);
const ATLAS_W: usize = ATLAS_COLS * font::SIZE;
const ATLAS_H: usize = ATLAS_ROWS * font::SIZE;
/// The cell repurposed as a fully-opaque white texel for solid rectangles. The
/// last glyph (`0x7F`, DEL) is never printed, so we overwrite it.
const WHITE_CELL: usize = font::COUNT - 1;

/// Build the R8 coverage atlas: every printable glyph stamped into its cell,
/// plus a white [`WHITE_CELL`] for rectangle fills.
fn build_atlas() -> Vec<u8> {
    let mut pixels = vec![0u8; ATLAS_W * ATLAS_H];
    for (idx, glyph) in font::GLYPHS.iter().enumerate() {
        let (cx, cy) = (
            (idx % ATLAS_COLS) * font::SIZE,
            (idx / ATLAS_COLS) * font::SIZE,
        );
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font::SIZE {
                // font8x8: bit 0 (LSB) is the leftmost pixel.
                if (bits >> col) & 1 == 1 {
                    pixels[(cy + row) * ATLAS_W + (cx + col)] = 0xFF;
                }
            }
        }
    }
    // Solid white block for rect fills.
    let (wx, wy) = (
        (WHITE_CELL % ATLAS_COLS) * font::SIZE,
        (WHITE_CELL / ATLAS_COLS) * font::SIZE,
    );
    for row in 0..font::SIZE {
        for col in 0..font::SIZE {
            pixels[(wy + row) * ATLAS_W + (wx + col)] = 0xFF;
        }
    }
    pixels
}

/// UV of a single texel at the centre of `cell` — used for solid rectangles so
/// every fragment samples full coverage.
fn cell_center_uv(cell: usize) -> [f32; 2] {
    let cx = (cell % ATLAS_COLS) * font::SIZE + font::SIZE / 2;
    let cy = (cell / ATLAS_COLS) * font::SIZE + font::SIZE / 2;
    [cx as f32 / ATLAS_W as f32, cy as f32 / ATLAS_H as f32]
}

/// UV rectangle `[u0, v0, u1, v1]` covering the glyph cell for `ch`.
fn glyph_uv(ch: char) -> Option<[f32; 4]> {
    let code = ch as u32;
    if code < font::FIRST as u32 || code >= font::FIRST as u32 + (font::COUNT as u32 - 1) {
        // Out of range, or the white/DEL cell which isn't a printable glyph.
        return None;
    }
    let idx = (code - font::FIRST as u32) as usize;
    let (cx, cy) = (
        (idx % ATLAS_COLS) * font::SIZE,
        (idx / ATLAS_COLS) * font::SIZE,
    );
    Some([
        cx as f32 / ATLAS_W as f32,
        cy as f32 / ATLAS_H as f32,
        (cx + font::SIZE) as f32 / ATLAS_W as f32,
        (cy + font::SIZE) as f32 / ATLAS_H as f32,
    ])
}

/// All GPU state for the overlay pass plus this frame's accumulated geometry.
pub struct Overlay {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,

    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// Capacities in *elements*, so we only reallocate when geometry grows.
    vertex_capacity: usize,
    index_capacity: usize,

    /// CPU-side accumulation, rebuilt every frame.
    vertices: Vec<Vertex2D>,
    indices: Vec<u32>,
}

impl Overlay {
    /// Build the overlay pipeline, atlas, and initial dynamic buffers.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        // --- Screen-size uniform ---
        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay screen uniform"),
            contents: bytemuck::cast_slice(&[ScreenUniform {
                size: [width.max(1) as f32, height.max(1) as f32],
                _pad: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay screen layout"),
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
        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay screen bind group"),
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });

        // --- Glyph atlas texture ---
        let atlas_pixels = build_atlas();
        let atlas_size = wgpu::Extent3d {
            width: ATLAS_W as u32,
            height: ATLAS_H as u32,
            depth_or_array_layers: 1,
        };
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay glyph atlas"),
            size: atlas_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_W as u32),
                rows_per_image: Some(ATLAS_H as u32),
            },
            atlas_size,
        );
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Nearest filtering keeps the bitmap font crisp at any scale.
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay atlas layout"),
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
            ],
        });
        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay atlas bind group"),
            layout: &atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        // --- Pipeline ---
        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay pipeline layout"),
            bind_group_layouts: &[Some(&screen_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex2D::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // Standard straight-alpha blending so the UI composites over
                    // the 3D scene.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // 2D UI has no meaningful winding; draw both sides.
                cull_mode: None,
                ..Default::default()
            },
            // The overlay draws last and ignores depth (it's pure 2D).
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Modest initial capacity; grows on demand.
        let vertex_capacity = 1024;
        let index_capacity = 1536;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay vertex buffer"),
            size: (vertex_capacity * std::mem::size_of::<Vertex2D>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay index buffer"),
            size: (index_capacity * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            screen_buffer,
            screen_bind_group,
            atlas_bind_group,
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Update the screen-size uniform after a surface resize.
    pub fn resize(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::cast_slice(&[ScreenUniform {
                size: [width.max(1) as f32, height.max(1) as f32],
                _pad: [0.0, 0.0],
            }]),
        );
    }

    /// Clear last frame's accumulated geometry. Called at the start of each
    /// frame, before the consumer rebuilds the UI.
    pub fn begin_frame(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Push one quad (two triangles) with a per-vertex UV rectangle.
    fn push_quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: Color) {
        let base = self.vertices.len() as u32;
        let [u0, v0, u1, v1] = uv;
        self.vertices.extend_from_slice(&[
            Vertex2D {
                pos: [x, y],
                uv: [u0, v0],
                color,
            },
            Vertex2D {
                pos: [x + w, y],
                uv: [u1, v0],
                color,
            },
            Vertex2D {
                pos: [x + w, y + h],
                uv: [u1, v1],
                color,
            },
            Vertex2D {
                pos: [x, y + h],
                uv: [u0, v1],
                color,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Upload this frame's geometry and record the overlay render pass on top of
    /// `view`. Skips entirely if nothing was drawn.
    pub fn flush(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if self.indices.is_empty() {
            return;
        }

        // Grow the GPU buffers if this frame outgrew them.
        if self.vertices.len() > self.vertex_capacity {
            self.vertex_capacity = self.vertices.len().next_power_of_two();
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay vertex buffer"),
                size: (self.vertex_capacity * std::mem::size_of::<Vertex2D>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if self.indices.len() > self.index_capacity {
            self.index_capacity = self.indices.len().next_power_of_two();
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay index buffer"),
                size: (self.index_capacity * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&self.indices));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Composite over the already-rendered 3D scene.
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
    }
}

impl Painter for Overlay {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let uv = cell_center_uv(WHITE_CELL);
        self.push_quad(x, y, w, h, [uv[0], uv[1], uv[0], uv[1]], color);
    }

    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color) {
        // Snap the run origin to whole pixels so the bitmap font stays crisp.
        let mut pen_x = x.round();
        let pen_y = y.round();
        for ch in text.chars() {
            if let Some(uv) = glyph_uv(ch) {
                self.push_quad(pen_x, pen_y, px, px, uv, color);
            }
            // Spaces and unknown glyphs still advance (monospace).
            pen_x += px;
        }
    }

    fn text_size(&self, text: &str, px: f32) -> [f32; 2] {
        [text.chars().count() as f32 * px, px]
    }
}
