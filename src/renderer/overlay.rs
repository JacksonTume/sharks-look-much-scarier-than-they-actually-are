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
//! [`Ui`](slmsttaa_ui::Ui) immediate-mode layer is built on top of it, but a
//! consumer could also drive it directly. Primitives are accumulated into CPU
//! vectors each frame (cleared by [`Overlay::begin_frame`]) and uploaded in one
//! shot at flush time.

use wgpu::util::DeviceExt;

use super::font;
use slmsttaa_ui::{Color, Layer, Painter, Rect};

/// A 2D overlay vertex: pixel position, atlas UV, RGBA tint, and the shape
/// parameters the fragment shader needs for rounded corners, borders, and
/// clipping.
///
/// The last three fields are per-vertex rather than per-draw on purpose: it is
/// what lets a panel with rounded corners, a hairline border, and a clipped
/// scroll region inside it all still be **one** draw call. The cost is 80 bytes
/// a vertex, which for a few thousand UI vertices is nothing.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex2D {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// The shape being drawn: `[centre.x, centre.y, half.x, half.y]`, in pixels.
    shape: [f32; 4],
    /// `[radius, border width, mode, unused]`; mode 0 skips the SDF.
    params: [f32; 4],
    /// `[min.x, min.y, max.x, max.y]`, in pixels.
    clip: [f32; 4],
}

/// Shader mode: a plain textured quad — text, and square-cornered rectangles.
const MODE_PLAIN: f32 = 0.0;
/// Shader mode: a filled rounded box, evaluated as an SDF.
const MODE_FILL: f32 = 1.0;
/// Shader mode: the outline of a rounded box.
const MODE_STROKE: f32 = 2.0;

/// A clip rectangle large enough to never clip anything.
const NO_CLIP: [f32; 4] = [-1.0e9, -1.0e9, 1.0e9, 1.0e9];

/// How far a rounded/stroked quad is inflated past its shape so the antialiased
/// edge has room to fade out. One pixel each side is exactly the smoothstep band.
const AA_PAD: f32 = 1.0;

impl Vertex2D {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x4,
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4
    ];

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

    /// CPU-side accumulation, rebuilt every frame. Vertices are shared across
    /// layers — only *index* order decides what covers what — so layering costs
    /// one index vector per [`Layer`] and still resolves to a single draw call.
    vertices: Vec<Vertex2D>,
    indices: [Vec<u32>; Layer::COUNT],
    /// Concatenation of `indices` in layer order, rebuilt at flush time.
    flat_indices: Vec<u32>,
    /// Which bucket the painter methods currently fill.
    layer: usize,
    /// The clip stack, in pixels, each entry already intersected with the one
    /// below it. Empty means "no clipping".
    clips: Vec<[f32; 4]>,

    /// Physical pixels per logical point. The UI lays out in points; this is
    /// where they become pixels, which is the only place the display's scale
    /// factor is allowed to matter.
    scale: f32,
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
            indices: Default::default(),
            flat_indices: Vec::new(),
            layer: Layer::default().index(),
            clips: Vec::new(),
            scale: 1.0,
        }
    }

    /// Set how many physical pixels one logical point is worth.
    ///
    /// Called by the renderer from the window's scale factor. Without this the
    /// UI is laid out in points but drawn as though they were pixels, which is
    /// why it rendered at half size on a 2× display.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = if scale > 0.0 { scale } else { 1.0 };
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
        for bucket in &mut self.indices {
            bucket.clear();
        }
        self.flat_indices.clear();
        self.layer = Layer::default().index();
        self.clips.clear();
    }

    /// The clip rectangle currently in force, in pixels.
    fn clip(&self) -> [f32; 4] {
        self.clips.last().copied().unwrap_or(NO_CLIP)
    }

    /// Push one quad (two triangles) with a per-vertex UV rectangle.
    ///
    /// Takes logical points and emits physical pixels; the indices go into the
    /// current layer's bucket, which is what decides draw order at flush time.
    fn push_quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: Color) {
        self.push_shape(x, y, w, h, uv, color, 0.0, 0.0, MODE_PLAIN);
    }

    /// Push one quad, carrying the shape parameters the fragment shader needs.
    ///
    /// For the SDF modes the emitted quad is inflated by [`AA_PAD`] beyond the
    /// shape it describes, so the antialiased edge fades out inside the geometry
    /// instead of being cut off by it.
    #[allow(clippy::too_many_arguments)]
    fn push_shape(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: [f32; 4],
        color: Color,
        radius: f32,
        border: f32,
        mode: f32,
    ) {
        let s = self.scale;
        let (sx, sy, sw, sh) = (x * s, y * s, w * s, h * s);

        // The shape, in pixels — described independently of the quad drawn.
        let shape = [sx + sw * 0.5, sy + sh * 0.5, sw * 0.5, sh * 0.5];
        let params = [radius * s, border * s, mode, 0.0];
        let clip = self.clip();

        // Inflate the quad for the antialiasing band (plain quads need none).
        let pad = if mode == MODE_PLAIN { 0.0 } else { AA_PAD };
        let (x, y, w, h) = (sx - pad, sy - pad, sw + 2.0 * pad, sh + 2.0 * pad);

        let base = self.vertices.len() as u32;
        let [u0, v0, u1, v1] = uv;
        self.vertices.extend_from_slice(&[
            Vertex2D {
                pos: [x, y],
                uv: [u0, v0],
                color,
                shape,
                params,
                clip,
            },
            Vertex2D {
                pos: [x + w, y],
                uv: [u1, v0],
                color,
                shape,
                params,
                clip,
            },
            Vertex2D {
                pos: [x + w, y + h],
                uv: [u1, v1],
                color,
                shape,
                params,
                clip,
            },
            Vertex2D {
                pos: [x, y + h],
                uv: [u0, v1],
                color,
                shape,
                params,
                clip,
            },
        ]);
        self.indices[self.layer].extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
        ]);
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
        // Flatten the per-layer buckets back-to-front. Because every layer
        // indexes the same vertex vector, the whole overlay — background,
        // widgets, and any popup above them — is still one draw call; only the
        // order the triangles are listed in changed.
        self.flat_indices.clear();
        for layer in Layer::ALL {
            self.flat_indices
                .extend_from_slice(&self.indices[layer.index()]);
        }
        if self.flat_indices.is_empty() {
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
        if self.flat_indices.len() > self.index_capacity {
            self.index_capacity = self.flat_indices.len().next_power_of_two();
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay index buffer"),
                size: (self.index_capacity * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(
            &self.index_buffer,
            0,
            bytemuck::cast_slice(&self.flat_indices),
        );

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
        pass.draw_indexed(0..self.flat_indices.len() as u32, 0, 0..1);
    }
}

impl Painter for Overlay {
    fn fill_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        let uv = cell_center_uv(WHITE_CELL);
        let uv = [uv[0], uv[1], uv[0], uv[1]];
        // A zero radius keeps the old cheap path: no SDF, no inflated quad, and
        // pixel-identical to what square rectangles drew before.
        let mode = if radius > 0.0 { MODE_FILL } else { MODE_PLAIN };
        self.push_shape(rect.x, rect.y, rect.w, rect.h, uv, color, radius, 0.0, mode);
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        if width <= 0.0 {
            return;
        }
        let uv = cell_center_uv(WHITE_CELL);
        let uv = [uv[0], uv[1], uv[0], uv[1]];
        self.push_shape(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            uv,
            color,
            radius,
            width,
            MODE_STROKE,
        );
    }

    fn push_clip(&mut self, rect: Rect) {
        let s = self.scale;
        let want = [rect.x * s, rect.y * s, rect.max_x() * s, rect.max_y() * s];
        // Intersect with whatever is already in force, so nesting can only ever
        // shrink the visible region.
        let current = self.clip();
        self.clips.push([
            want[0].max(current[0]),
            want[1].max(current[1]),
            want[2].min(current[2]),
            want[3].min(current[3]),
        ]);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, color: Color) {
        // Snap the run origin to whole *physical* pixels so the bitmap font
        // stays crisp — rounding in points would still land mid-pixel at 1.5×.
        let mut pen_x = (x * self.scale).round() / self.scale;
        let pen_y = (y * self.scale).round() / self.scale;
        for ch in text.chars() {
            if let Some(uv) = glyph_uv(ch) {
                self.push_quad(pen_x, pen_y, px, px, uv, color);
            }
            // Spaces and unknown glyphs still advance (monospace).
            pen_x += px;
        }
    }

    fn text_size(&self, text: &str, px: f32) -> [f32; 2] {
        // In points: the UI lays out in points and never learns the scale.
        [text.chars().count() as f32 * px, px]
    }

    fn set_layer(&mut self, layer: Layer) {
        self.layer = layer.index();
    }
}
