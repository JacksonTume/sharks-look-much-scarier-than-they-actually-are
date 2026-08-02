// The shared WGSL prelude: the camera uniform, the sun, and the sky.
//
// Textually prepended to both `shader.wgsl` and `fullscreen.wgsl` by
// `renderer/mod.rs`, because WGSL has no `#include` and these declarations have
// two callers that must agree exactly.
//
// The sky is the reason this file exists. It has two consumers:
//
//   - the **sky pass**, which draws it as the background, and
//   - the **water shader**, which falls back to it when a reflected ray hits
//     nothing.
//
// If those disagreed, a lake would reflect a different sky than the one above
// it — a bug that presents as "the water colour is slightly off" and would be
// miserable to trace back to two copies of a gradient. Concatenating one source
// into both modules makes that unrepresentable, which is the same argument
// `slmsttaa_ui::font` makes about text metrics having exactly one home.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    // Screen -> world. The sky pass has no geometry and therefore no world
    // position to shade from; it unprojects a screen point into a ray instead.
    inv_view_proj: mat4x4<f32>,
    // World-space eye. Needed for every view-dependent term; `w` is padding.
    eye: vec4<f32>,
    // x = wall-clock seconds since start. Lets surface detail animate without
    // the consumer rebuilding a mesh to express it.
    frame: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// One directional light plus ambient. Deliberately fixed rather than exposed: no
// demo has asked to move the sun, and a setter with no caller is the speculative
// build the roadmap forbids. These are the exact constants the terrain demo used
// to bake in by hand, so retiring that bake changed nothing on screen.
const LIGHT_DIR: vec3<f32> = vec3<f32>(0.45, 0.85, 0.35);
const AMBIENT: f32 = 0.35;
const DIFFUSE: f32 = 0.65;

// The gradient. Deliberately a *clear day*: a bright, slightly desaturated
// horizon under a deeper zenith is the condition in which water reads most
// obviously as water, because it gives the Fresnel edge something much brighter
// than the lake bed to tend toward.
//
// **These are linear values, and that is the whole trick to reading them.** The
// surface is `Bgra8UnormSrgb`, so the GPU encodes whatever the shader returns —
// 0.34 linear lands on screen at about 0.62. Picked by eye they come out
// washed-out and grey, which is exactly what the first attempt at this file did:
// a horizon of 0.55 displayed at 0.77 and the sky read as fog.
const SKY_ZENITH: vec3<f32> = vec3<f32>(0.025, 0.11, 0.42);
const SKY_HORIZON: vec3<f32> = vec3<f32>(0.30, 0.46, 0.70);
// Below the horizon the "sky" is really the ground haze a downward ray would
// see. It matters more than it sounds: a reflection ray off a steep wave can
// point downward, and without this it would sample black and read as a hole.
const SKY_GROUND: vec3<f32> = vec3<f32>(0.035, 0.038, 0.042);
const SUN_TINT: vec3<f32> = vec3<f32>(1.0, 0.94, 0.80);

// The colour of the sky in direction `dir` (normalized, +Y up).
fn sky_color(dir: vec3<f32>) -> vec3<f32> {
    // Up: horizon -> zenith. The exponent biases the blend toward the horizon so
    // the bright band is thin, which is what stops it reading as a flat wash.
    let up = clamp(dir.y, 0.0, 1.0);
    var col = mix(SKY_HORIZON, SKY_ZENITH, pow(up, 0.42));

    // Down: horizon -> ground haze, over a short angular range so the join at
    // y = 0 is continuous rather than a visible seam.
    let down = clamp(-dir.y, 0.0, 1.0);
    col = mix(col, SKY_GROUND, pow(down, 0.35));

    // The sun: a small hot disc plus a broad glow. Two powers rather than one
    // because a single exponent cannot be both tight enough to read as a disc
    // and wide enough to bloom around it.
    let s = max(dot(dir, normalize(LIGHT_DIR)), 0.0);
    col += SUN_TINT * pow(s, 900.0) * 12.0;
    col += SUN_TINT * pow(s, 14.0) * 0.22;

    return col;
}

// The world-space view ray through a clip-space XY, for a fullscreen pass.
fn view_ray(clip_xy: vec2<f32>) -> vec3<f32> {
    let far = camera.inv_view_proj * vec4<f32>(clip_xy, 1.0, 1.0);
    return normalize(far.xyz / far.w - camera.eye.xyz);
}
