// Overlay shader: draws 2D screen-space quads (UI rectangles and text glyphs)
// composited on top of the 3D scene. Positions arrive in physical pixels with
// the origin at the top-left; we map them to clip space here.
//
// The glyph atlas is a single-channel (R8) **signed distance field**: 0.5 is the
// glyph edge and larger is further inside, so one bake serves every size in the
// type scale. Rectangles don't sample it at all — they take their coverage from
// the mode instead, which is why the old opaque-white-texel trick is gone.
//
// Rounded corners, borders, glyph antialiasing, and clipping are all evaluated
// per-fragment from per-vertex parameters, which is what keeps the whole UI a
// single draw call — no scissor-rect state changes, no pipeline switches.
// Everything here sticks to GLSL ES 3.0-compatible constructs so the WebGL2
// fallback behaves identically to WebGPU: no derivatives, no storage buffers.
//
// That "no derivatives" rule is why text carries its antialiasing width as a
// vertex attribute. The usual SDF trick is `fwidth(distance)` to recover how fast
// the field changes per pixel; here the CPU computes it from the render size and
// the display scale (`slmsttaa_ui::font::aa_band`), which is both portable and
// exact rather than estimated.

struct Screen {
    // Surface size in physical pixels; .zw is padding to a 16-byte uniform.
    size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: Screen;

@group(1) @binding(0)
var atlas_tex: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    // The shape this fragment belongs to: centre.xy, half-size.xy, in pixels.
    // Note this is the *shape*, not the quad — the quad is inflated slightly so
    // the antialiased edge has somewhere to fade out.
    @location(3) shape: vec4<f32>,
    // x: corner radius, y: border width (0 = filled), z: mode, w: glyph AA band.
    // Modes: 0 flat rect, 1 rounded fill, 2 rounded stroke, 3 distance-field glyph.
    @location(4) params: vec4<f32>,
    // Clip rectangle: min.xy, max.xy, in pixels.
    @location(5) clip: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) px: vec2<f32>,
    @location(3) shape: vec4<f32>,
    @location(4) params: vec4<f32>,
    @location(5) clip: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Pixel (top-left origin, y down) -> NDC (centre origin, y up).
    let ndc = vec2<f32>(
        in.pos.x / screen.size.x * 2.0 - 1.0,
        1.0 - in.pos.y / screen.size.y * 2.0,
    );
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    // Interpolated across the quad, so the fragment shader knows where it is in
    // pixel space without needing derivatives.
    out.px = in.pos;
    out.shape = in.shape;
    out.params = in.params;
    out.clip = in.clip;
    return out;
}

// Signed distance to a rounded box centred on the origin: negative inside,
// positive outside, and in pixel units — which is why a 1-pixel smoothstep band
// gives correct antialiasing without sampling derivatives.
fn sd_round_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sampled before any branching: WGSL requires texture sampling to happen in
    // uniform control flow, so this cannot move below the clip test or into the
    // text branch, even though only glyphs use the result.
    let field = textureSample(atlas_tex, atlas_sampler, in.uv).r;

    // Clipping is a discard rather than a scissor rect, so a clipped region
    // costs nothing in draw calls and nests by simple intersection on the CPU.
    if (in.px.x < in.clip.x || in.px.y < in.clip.y
        || in.px.x > in.clip.z || in.px.y > in.clip.w) {
        discard;
    }

    var alpha = in.color.a;

    let mode = in.params.z;
    if (mode > 2.5) {
        // A glyph. 0.5 is the outline; the band is how much of the field one
        // screen pixel spans, so the same atlas antialiases correctly at 15pt on
        // a 1x display and at 24pt on a 2x one.
        let band = in.params.w;
        alpha = alpha * smoothstep(0.5 - band, 0.5 + band, field);
    } else if (mode > 0.5) {
        let d = sd_round_box(in.px - in.shape.xy, in.shape.zw, in.params.x);
        // 1-pixel band centred on the edge.
        var cov = 1.0 - smoothstep(-0.5, 0.5, d);
        if (mode > 1.5) {
            // A border is the fill minus an inset copy of itself: keep only the
            // fragments within `border` pixels inside the edge.
            cov = cov * smoothstep(-0.5, 0.5, d + in.params.y);
        }
        alpha = alpha * cov;
    }

    return vec4<f32>(in.color.rgb, alpha);
}
