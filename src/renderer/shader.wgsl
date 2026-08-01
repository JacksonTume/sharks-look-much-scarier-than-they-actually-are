// Scene shader: transforms vertices by the instance's model matrix and then the
// camera's view-projection matrix, and lights them in world space.
//
// Lighting lives here rather than in the consumer because a demo that bakes
// shading into its vertex colors is only correct while the mesh never moves —
// rotate such an object and the highlight turns with it. Evaluating the light
// *after* the model transform is the whole point.
//
// The model is one directional light with three terms: Lambert diffuse (which is
// all there was for a long time), a Blinn-Phong specular highlight, and a Schlick
// Fresnel edge. The last two are *view-dependent* and are what a surface needs to
// read as wet rather than as colored plastic — under diffuse alone a rippling
// water surface and a flat one are very nearly the same picture, because diffuse
// shading does not care where the viewer is.
//
// Both default to zero strength, so a material that asks for neither shades
// exactly as it did before they existed.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    // World-space eye. Needed for the two view-dependent terms; `w` is padding.
    eye: vec4<f32>,
    // x = wall-clock seconds since start. Lets surface detail animate without the
    // consumer rebuilding a mesh to express it.
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

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // RGBA. The alpha is the *shape* of the transparency across a surface; the
    // instance tint's alpha is its overall strength.
    @location(2) color: vec4<f32>,
};

// Per-instance data, stepped once per object rather than once per vertex. WGSL
// has no matrix vertex attribute, so both matrices arrive as loose columns and
// are reassembled here.
struct InstanceInput {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    // The 3x3 inverse-transpose of the model's rotation/scale block. Normals
    // cannot ride the model matrix: a non-uniform scale stretches them out of
    // perpendicular with the surface they describe.
    @location(7) normal_0: vec3<f32>,
    @location(8) normal_1: vec3<f32>,
    @location(9) normal_2: vec3<f32>,
    @location(10) tint: vec4<f32>,
    // [specular strength, shininess, fresnel f0, ripple strength] — packed into
    // one slot because attribute slots are scarcer than bytes.
    @location(11) shading: vec4<f32>,
    // [fresnel tint rgb, ripple scale].
    @location(12) fresnel_tint: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) shading: vec4<f32>,
    @location(4) fresnel_tint: vec3<f32>,
    @location(5) ripple_scale: f32,
};

@vertex
fn vs_main(in: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );
    let normal_matrix = mat3x3<f32>(
        instance.normal_0,
        instance.normal_1,
        instance.normal_2,
    );

    var out: VertexOutput;
    // The tint multiplies rather than replaces, so a mesh keeps whatever shading
    // was authored into its corners and gets recolored on top. That now includes
    // alpha: a surface that fades at its edges keeps its shape and gets scaled.
    out.color = in.color * instance.tint;
    // Normalized in the fragment stage instead, since interpolating across a
    // triangle shortens it.
    out.world_normal = normal_matrix * in.normal;
    // Object space -> world space, kept for the view direction.
    let world = model * vec4<f32>(in.position, 1.0);
    out.world_position = world.xyz;
    out.shading = instance.shading;
    out.fresnel_tint = instance.fresnel_tint.xyz;
    out.ripple_scale = instance.fresnel_tint.w;
    out.clip_position = camera.view_proj * world;
    return out;
}

// The slope `(dh/dx, dh/dz)` of an animated ripple field at a world position.
//
// Six octaves of directional waves. Three things stop it looking like the stripes
// a naive sum of sines produces, and all three are necessary:
//
//   - **Every octave points somewhere else.** The direction is rotated by ~113
//     degrees each time, so no two octaves share an axis and none lines up with
//     the grid the geometry was built on.
//   - **The frequency ratio is not an integer.** At 1.87 the octaves never share
//     a period, so the pattern does not repeat at any scale you can see.
//   - **Longer waves travel faster**, which is what real deep water does
//     (phase speed goes as the square root of wavelength). Octaves marching in
//     lockstep is what turns a wave field into one sliding moiré band.
//
// Amplitude falls by 0.55 while frequency rises by 1.87, so each octave
// contributes roughly the same *slope* — the surface is equally rough at every
// scale, which is the property that reads as water rather than as a wobble.
//
// Evaluated per fragment, so the detail is per pixel and does not depend on how
// finely the surface happens to be tessellated.
fn ripple_slope(p: vec2<f32>, t: f32, scale: f32) -> vec2<f32> {
    var slope = vec2<f32>(0.0, 0.0);
    var dir = vec2<f32>(0.8, 0.6);
    var freq = max(scale, 0.001);
    var amp = 1.0;
    for (var i = 0; i < 6; i = i + 1) {
        let speed = inverseSqrt(freq) * 2.4;
        let phase = dot(dir, p) * freq + t * speed * freq;
        slope = slope + dir * (cos(phase) * amp * freq);
        // Rotate the next octave off this one's axis.
        let rc = -0.39;
        let rs = 0.92;
        dir = vec2<f32>(dir.x * rc - dir.y * rs, dir.x * rs + dir.y * rc);
        freq = freq * 1.87;
        amp = amp * 0.55;
    }
    // Normalized so `ripple_strength` means the same thing whatever `scale` is.
    return slope / max(scale, 0.001) * 0.18;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var normal = normalize(in.world_normal);

    // Animated surface detail, before any lighting reads the normal.
    let ripple_strength = in.shading.w;
    if (ripple_strength > 0.0) {
        let slope = ripple_slope(in.world_position.xz, camera.frame.x, in.ripple_scale);
        // The perturbation is a horizontal tilt of an up-facing surface, which is
        // what a ripple on water is. Adding rather than replacing keeps whatever
        // the geometry itself was doing.
        normal = normalize(normal + vec3<f32>(-slope.x, 0.0, -slope.y) * ripple_strength);
    }

    let light = normalize(LIGHT_DIR);
    let lambert = max(dot(normal, light), 0.0);
    var rgb = in.color.rgb * (AMBIENT + DIFFUSE * lambert);
    var alpha = in.color.a;

    let specular = in.shading.x;
    let shininess = in.shading.y;
    let f0 = in.shading.z;

    // Everything below is view-dependent, so it needs the eye. A material with
    // neither term set skips it entirely and shades as pure Lambert.
    if (specular > 0.0 || f0 > 0.0) {
        let view = normalize(camera.eye.xyz - in.world_position);
        // Two-sided: a water surface seen from underneath, or a normal perturbed
        // past grazing by a wave, should still catch the light rather than turn
        // matte black.
        let facing = select(-normal, normal, dot(normal, view) >= 0.0);

        if (specular > 0.0) {
            // Blinn-Phong: the half-vector formulation, which stays stable at
            // grazing angles where the reflect() form collapses.
            let half_vec = normalize(light + view);
            // Gated on the light actually reaching this face, or a surface turned
            // away from the sun still gets a highlight through its own back.
            let lit = step(0.0, dot(facing, light));
            rgb += specular * lit * pow(max(dot(facing, half_vec), 0.0), shininess);
        }

        if (f0 > 0.0) {
            // Schlick's approximation. A dielectric is barely reflective face-on
            // and almost a mirror at grazing incidence, which is why a lake shows
            // its bed at your feet and the sky at the far shore.
            let cos_theta = max(dot(facing, view), 0.0);
            let fresnel = f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
            // No second render pass exists, so this goes toward a flat color
            // rather than toward an image of the scene — a stand-in for a
            // reflection, and the honest ceiling until an offscreen target lands.
            rgb = mix(rgb, in.fresnel_tint, fresnel);
            // A reflective surface is also a less transparent one: what you see
            // at a grazing angle is the reflection, not what lies beneath.
            alpha = mix(alpha, 1.0, fresnel);
        }
    }

    return vec4<f32>(rgb, alpha);
}
