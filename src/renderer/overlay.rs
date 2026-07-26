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
//! The overlay uploads the toolkit's baked glyph atlas
//! ([`slmsttaa_ui::font::ATLAS`]) and exposes a tiny CPU draw API ([`Painter`]);
//! the [`Ui`](slmsttaa_ui::Ui) immediate-mode layer is built on top of it, but a
//! consumer could also drive it directly. Primitives are accumulated into CPU
//! vectors each frame (cleared by [`Overlay::begin_frame`]) and uploaded in one
//! shot at flush time.
//!
//! The atlas is a **signed distance field**, which is why this module no longer
//! rasterizes a font at startup and no longer owns one. It belongs to
//! `slmsttaa-ui` so that the metrics used to *lay text out* and the glyphs used
//! to *draw it* cannot come from two different fonts — see
//! [`slmsttaa_ui::font`] for why that was worth a narrower seam.

use wgpu::util::DeviceExt;

use slmsttaa_ui::font::{self, Weight};
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
    /// `[radius, border width, mode, glyph AA band]`.
    ///
    /// The last slot was unused through Slice 4 and is now what makes distance
    /// field text work without derivatives — see [`Overlay::text`].
    params: [f32; 4],
    /// `[min.x, min.y, max.x, max.y]`, in pixels.
    clip: [f32; 4],
}

/// Shader mode: a flat, square-cornered rectangle. No SDF, no texture.
const MODE_RECT: f32 = 0.0;
/// Shader mode: a filled rounded box, evaluated as an SDF.
const MODE_FILL: f32 = 1.0;
/// Shader mode: the outline of a rounded box.
const MODE_STROKE: f32 = 2.0;
/// Shader mode: a glyph, sampled from the distance field atlas.
const MODE_TEXT: f32 = 3.0;

/// A clip rectangle large enough to never clip anything.
const NO_CLIP: [f32; 4] = [-1.0e9, -1.0e9, 1.0e9, 1.0e9];

/// UVs for a primitive that isn't textured. Non-text modes ignore the sample, so
/// the value only has to be in range — the fetch still happens because WGSL
/// requires texture sampling in uniform control flow.
const NO_UV: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// How far a rounded/stroked quad is inflated past its shape so the antialiased
/// edge has room to fade out. One pixel each side is exactly the smoothstep band.
///
/// Glyph quads are **not** inflated: the baked field already carries its own
/// padding, so the quad the toolkit hands over is exactly the region the field
/// covers.
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

// --- Atlas -----------------------------------------------------------------
//
// There is nothing to build any more. Through Slice 4 this module rasterized an
// 8x8 bitmap font into a coverage atlas at startup and reserved one cell as an
// opaque white texel so that solid rectangles could share the text pipeline.
// Both are gone: the atlas is a baked distance field that arrives from
// `slmsttaa_ui::font::ATLAS` as bytes, and rectangles no longer sample a texture
// at all — the shader gives them full coverage by mode instead. The engine's
// `renderer/font.rs` went with it.
//
// The atlas is *the toolkit's*, deliberately, so that the metrics used to lay
// text out and the glyphs used to draw it cannot come from different fonts. See
// `slmsttaa_ui::font` for that argument.

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
        let atlas_size = wgpu::Extent3d {
            width: font::metrics::ATLAS_W,
            height: font::metrics::ATLAS_H,
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
            font::ATLAS,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(font::metrics::ATLAS_W),
                rows_per_image: Some(font::metrics::ATLAS_H),
            },
            atlas_size,
        );
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // **Linear**, where the bitmap font wanted nearest. A distance field has
        // to be interpolated — the whole reason it scales to any size is that a
        // fragment reads a smoothly varying distance, and point-sampling it would
        // reintroduce exactly the stair-stepped edges it exists to avoid.
        //
        // Safe against bleeding between neighbours: the baker leaves a one-texel
        // gutter, and a zero texel decodes as "fully outside", which is the
        // correct thing for a filter tap to pick up.
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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

    /// Push one quad (two triangles), carrying the shape parameters the fragment
    /// shader needs.
    ///
    /// Takes logical points and emits physical pixels; the indices go into the
    /// current layer's bucket, which is what decides draw order at flush time.
    ///
    /// For the rounded-box modes the emitted quad is inflated by [`AA_PAD`]
    /// beyond the shape it describes, so the antialiased edge fades out inside
    /// the geometry instead of being cut off by it. Rectangles and glyphs are
    /// not inflated — the former have hard edges, the latter carry their own
    /// padding in the baked field.
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
        aa: f32,
    ) {
        let s = self.scale;
        let (sx, sy, sw, sh) = (x * s, y * s, w * s, h * s);

        // The shape, in pixels — described independently of the quad drawn.
        let shape = [sx + sw * 0.5, sy + sh * 0.5, sw * 0.5, sh * 0.5];
        let params = [radius * s, border * s, mode, aa];
        let clip = self.clip();

        // Inflate the quad for the antialiasing band.
        let pad = if mode == MODE_FILL || mode == MODE_STROKE {
            AA_PAD
        } else {
            0.0
        };
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
        // A zero radius keeps the cheap path: no SDF, no inflated quad, no
        // texture fetch that matters.
        let mode = if radius > 0.0 { MODE_FILL } else { MODE_RECT };
        self.push_shape(
            rect.x, rect.y, rect.w, rect.h, NO_UV, color, radius, 0.0, mode, 0.0,
        );
    }

    fn stroke_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        if width <= 0.0 {
            return;
        }
        self.push_shape(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            NO_UV,
            color,
            radius,
            width,
            MODE_STROKE,
            0.0,
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

    fn text(&mut self, x: f32, y: f32, text: &str, px: f32, weight: Weight, color: Color) {
        // Snap the **baseline** to a whole physical pixel. The horizontal strokes
        // — baseline, x-height, cap line — are what the eye reads as crisp, and
        // they all key off it. Snapping x is pointless now that advances are
        // proportional and fractional: it would distort spacing to no benefit,
        // which is a change from the bitmap font, where every advance was a whole
        // number of points and snapping the origin snapped every glyph.
        let baseline = font::baseline(y, px);
        let baseline = (baseline * self.scale).round() / self.scale;

        // The antialiasing width, computed here because it depends on the display
        // scale factor, which the toolkit is never told. `overlay.wgsl` uses no
        // derivatives (so the WebGL2 fallback matches WebGPU), so `fwidth` isn't
        // available to estimate it in the shader — and the CPU knows it exactly.
        let aa = font::aa_band(px * self.scale);

        let mut pen = x;
        for ch in text.chars() {
            let glyph = font::glyph(ch, weight);
            if glyph.has_ink() {
                self.push_shape(
                    pen + glyph.x * px,
                    baseline + glyph.y * px,
                    glyph.w * px,
                    glyph.h * px,
                    glyph.uv,
                    color,
                    0.0,
                    0.0,
                    MODE_TEXT,
                    aa,
                );
            }
            // A space has no ink but still advances, and an unbaked character
            // resolves to a visible tofu box rather than being skipped.
            pen += glyph.advance * px;
        }
    }

    fn set_layer(&mut self, layer: Layer) {
        self.layer = layer.index();
    }
}
