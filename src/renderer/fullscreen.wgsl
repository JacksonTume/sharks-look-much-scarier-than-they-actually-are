// The two fullscreen passes: the sky behind the scene, and the composite that
// moves the offscreen scene colour onto the swapchain.
//
// `common.wgsl` is prepended to this file, supplying the camera uniform at
// @group(0) and `sky_color` / `view_ray`.

// The offscreen scene, at @group(1) so it does not collide with the camera. The
// same group layout the water shader binds, so one bind group serves both.
@group(1) @binding(0) var scene_color: texture_2d<f32>;
@group(1) @binding(1) var scene_sampler: sampler;

struct FullscreenOut {
    @builtin(position) clip_position: vec4<f32>,
    // Clip-space XY, for unprojecting to a view ray in the sky pass.
    @location(0) clip_xy: vec2<f32>,
    // Texture coordinates, y already flipped (clip space is +Y up, textures are
    // +Y down).
    @location(1) uv: vec2<f32>,
};

// One oversized triangle rather than two triangles forming a quad.
//
// A quad has a diagonal seam down the middle where the two triangles meet, and
// fragments along it get shaded by both — a measurable waste, and on some
// hardware a visible line when the shader is not perfectly continuous. A single
// triangle covering the whole viewport has no interior edge at all. The three
// vertices land at (-1,-1), (3,-1) and (-1,3), and the rasterizer clips away
// everything outside the screen for free.
@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOut {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: FullscreenOut;
    out.clip_xy = corner * 2.0 - 1.0;
    out.clip_position = vec4<f32>(out.clip_xy, 1.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

// The background. Replaces what used to be a flat clear colour, so the scene
// has a horizon and the water has something to reflect.
@fragment
fn fs_sky(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(sky_color(view_ray(in.clip_xy)), 1.0);
}

// Copy the offscreen scene colour to the swapchain.
//
// This pass exists for one reason, and it is a hard constraint rather than a
// preference: a texture cannot be a render target and a sampled input at the
// same time. The water needs to *read* the opaque scene to refract it, so the
// opaque scene cannot already be on the surface we are drawing the water onto.
// The cost is one fullscreen copy a frame; the alternative is no refraction.
@fragment
fn fs_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    return textureSample(scene_color, scene_sampler, in.uv);
}
