// Scene shader: transforms vertices by the instance's model matrix and then the
// camera's view-projection matrix, and lights them in world space.
//
// Lighting lives here rather than in the consumer because a demo that bakes
// shading into its vertex colors is only correct while the mesh never moves —
// rotate such an object and the highlight turns with it. Evaluating the light
// *after* the model transform is the whole point.
//
// The model is one directional light with Lambert diffuse (which is all there was
// for a long time), a Blinn-Phong specular highlight, and a Schlick Fresnel edge.
// The last two are *view-dependent* and are what a surface needs to read as wet
// rather than as colored plastic — under diffuse alone a rippling water surface
// and a flat one are very nearly the same picture, because diffuse shading does
// not care where the viewer is.
//
// Slice 16 added the two terms that need more than this fragment: **refraction**
// (what is behind the surface, displaced) and **screen-space reflection** (what
// is in front of it, mirrored). Both read the opaque scene as a texture, which is
// why this shader now has a second bind group and why the frame has an offscreen
// pass in front of it. See `graph.rs`.
//
// Every one of these terms defaults to zero strength, so a material that asks for
// none of them shades exactly as it did before they existed.
//
// `common.wgsl` is prepended to this file, supplying the camera uniform at
// @group(0), the light constants, and `sky_color`.

// The opaque scene, rendered before this pass. Only the blended pipeline binds
// it — the opaque pipeline *writes* `scene_color`, and a texture cannot be an
// attachment and a sampled input at once, so it gets a layout without this group.
@group(1) @binding(0) var scene_color: texture_2d<f32>;
@group(1) @binding(1) var scene_sampler: sampler;
// Non-linear depth, exactly as the depth test wrote it. Sampled with
// `textureLoad` rather than through a sampler: depth must not be filtered, since
// the average of a near and a far surface is a distance where nothing is.
@group(1) @binding(2) var scene_depth: texture_depth_2d;

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
    // [refraction strength, absorption density, reflection strength, unused] —
    // the terms that read the scene texture. The fourth channel is the only spare
    // per-instance float left anywhere in this buffer.
    @location(13) water: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) shading: vec4<f32>,
    @location(4) fresnel_tint: vec3<f32>,
    @location(5) ripple_scale: f32,
    @location(6) water: vec3<f32>,
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
    out.water = instance.water.xyz;
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

// Reconstruct a world-space position from a screen UV and the depth stored there.
//
// The inverse view-projection does all the work. Note the Y flip: UV space runs
// downward from the top-left, clip space runs upward from the centre.
fn world_from_depth(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec3<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
    let p = camera.inv_view_proj * vec4<f32>(ndc, 1.0);
    return p.xyz / p.w;
}

// Non-linear depth at a screen UV, read straight out of the depth attachment.
fn depth_at(uv: vec2<f32>) -> f32 {
    let dims = vec2<f32>(textureDimensions(scene_depth));
    let texel = vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * dims);
    return textureLoad(scene_depth, texel, 0);
}

// How far outside the screen a UV is, as a 0..1 fade. Used to dissolve
// screen-space effects at the frame edge instead of clipping them off.
fn edge_fade(uv: vec2<f32>) -> f32 {
    let d = min(uv, vec2<f32>(1.0) - uv);
    return clamp(min(d.x, d.y) / 0.08, 0.0, 1.0);
}

// Where a world point lands on screen. `w <= 0` means behind the camera.
struct Projected {
    uv: vec2<f32>,
    depth: f32,
    valid: bool,
};

fn project(p: vec3<f32>) -> Projected {
    var out: Projected;
    let clip = camera.view_proj * vec4<f32>(p, 1.0);
    out.valid = clip.w > 0.0;
    let ndc = clip.xyz / max(clip.w, 1e-6);
    out.uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    out.depth = ndc.z;
    return out;
}

// March the depth buffer along a reflected ray and return what it hits.
//
// `w` is zero when nothing was hit, which is the common case rather than the
// exceptional one — **screen-space reflection can only reflect what is already
// on the screen**, and a reflection ray off a near-horizontal surface travels
// almost parallel to the view, so it leaves the frame quickly. That is the
// technique's defining limitation, not a bug in this implementation, and it is
// exactly why the caller has a sky to fall back to. Without one, every miss
// would read as a black smear and SSR would look worse than no reflection at all.
//
// Three details, each of which was a visible artifact before it was a line of
// code:
//
//   - **The step is a fraction of the viewing distance, not a world constant.**
//     The engine has no idea how big a consumer's scene is: terrain's lakes are
//     0.004 units deep while its map is a few units across, and a step sized for
//     one is meaningless for the other. A first attempt used fixed world steps
//     and marched straight past the entire landscape, which read as noise.
//   - **The crossing is refined by bisection.** Taking the first step *past* a
//     surface as the hit point quantizes every reflection to the step size, and
//     because the steps grow geometrically the quantization grows with them —
//     which draws exactly the horizontal banding this replaced.
//   - **A hit is only trusted if the ray is near the surface it crossed.** A ray
//     that dives behind a mountain reappears "hit" on the far side otherwise,
//     mirroring geometry that is nowhere near the water.
fn trace_reflection(origin: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    // Scale-free: everything below is relative to how far away this fragment is.
    let unit = distance(camera.eye.xyz, origin);
    var step = unit * 0.012;
    var t = step;
    var prev_t = 0.0;

    for (var i = 0; i < 24; i = i + 1) {
        let p = origin + dir * t;
        let s = project(p);
        if (!s.valid) {
            return vec4<f32>(0.0);
        }
        if (s.uv.x < 0.0 || s.uv.x > 1.0 || s.uv.y < 0.0 || s.uv.y > 1.0) {
            return vec4<f32>(0.0);
        }
        let scene = depth_at(s.uv);
        // Smaller depth is nearer (the test is `Less`), so the ray has gone
        // behind something when the scene is in front of the sample.
        if (s.depth > scene) {
            // Bisect between the last miss and this hit to find where the ray
            // actually crossed, rather than accepting a whole step of error.
            var lo = prev_t;
            var hi = t;
            for (var k = 0; k < 6; k = k + 1) {
                let mid = (lo + hi) * 0.5;
                let m = project(origin + dir * mid);
                if (m.depth > depth_at(m.uv)) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let hit = project(origin + dir * hi);
            let hit_world = world_from_depth(hit.uv, depth_at(hit.uv));
            // Reject a crossing that happened far from any surface: the ray flew
            // behind something rather than landing on it.
            if (distance(hit_world, origin + dir * hi) > unit * 0.05) {
                return vec4<f32>(0.0);
            }
            let color = textureSampleLevel(scene_color, scene_sampler, hit.uv, 0.0).rgb;
            // Confidence falls with how far the ray had to go. A long ray is a
            // near-grazing one, it crosses terrain at a shallow angle where a
            // half-step of error moves the hit a long way, and its neighbours
            // resolve differently — which is what draws streaks. Fading those
            // back into the sky costs the least believable reflections and keeps
            // the short, near-vertical ones that read correctly.
            let confidence = clamp(1.0 - hi / (unit * 0.6), 0.0, 1.0);
            return vec4<f32>(color, edge_fade(hit.uv) * confidence);
        }
        prev_t = t;
        step = step * 1.35;
        t = t + step;
    }
    return vec4<f32>(0.0);
}

// ---------------------------------------------------------------------------
// Two entry points, and why
//
// `fs_main` shades everything in the opaque pass; `fs_water` shades the blended
// pass and is the only one that touches @group(1). This is not an optimization —
// it is forced. The opaque pass *renders into* `scene_color`, and a pipeline
// whose shader statically references a texture must have it in its layout, so an
// opaque pipeline sharing an entry point with the water would have to bind the
// very texture it is drawing to. The split is what lets the opaque pipeline have
// a camera-only layout and the conflict simply not exist.
//
// Everything they share lives in the helpers between here and there, so the two
// cannot drift apart in how they light a surface.
// ---------------------------------------------------------------------------

// The shading normal: the geometric one, plus animated ripples if asked for.
fn shading_normal(in: VertexOutput) -> vec3<f32> {
    var normal = normalize(in.world_normal);
    let ripple_strength = in.shading.w;
    if (ripple_strength > 0.0) {
        let slope = ripple_slope(in.world_position.xz, camera.frame.x, in.ripple_scale);
        // The perturbation is a horizontal tilt of an up-facing surface, which is
        // what a ripple on water is. Adding rather than replacing keeps whatever
        // the geometry itself was doing.
        normal = normalize(normal + vec3<f32>(-slope.x, 0.0, -slope.y) * ripple_strength);
    }
    return normal;
}

// Lambert diffuse: the base colour every surface gets.
fn diffuse_rgb(in: VertexOutput, normal: vec3<f32>) -> vec3<f32> {
    let lambert = max(dot(normal, normalize(LIGHT_DIR)), 0.0);
    return in.color.rgb * (AMBIENT + DIFFUSE * lambert);
}

// Apply the view-dependent terms over an already-diffuse-shaded colour.
//
// `reflected` is what the surface mirrors, supplied by the caller because that
// is the one thing the two entry points genuinely disagree about: the opaque
// pass can only offer the sky, the water pass can trace the scene first.
fn view_terms(
    in: VertexOutput,
    normal: vec3<f32>,
    rgb_in: vec3<f32>,
    alpha_in: f32,
    reflected: vec3<f32>,
) -> vec4<f32> {
    let specular = in.shading.x;
    let shininess = in.shading.y;
    let f0 = in.shading.z;

    var rgb = rgb_in;
    var alpha = alpha_in;

    // A material with neither term set shades as pure Lambert, exactly as it did
    // before any of this existed.
    if (specular <= 0.0 && f0 <= 0.0) {
        return vec4<f32>(rgb, alpha);
    }

    let view = normalize(camera.eye.xyz - in.world_position);
    // Two-sided: a water surface seen from underneath, or a normal perturbed past
    // grazing by a wave, should still catch the light rather than turn matte
    // black.
    let facing = select(-normal, normal, dot(normal, view) >= 0.0);

    if (f0 > 0.0) {
        // Schlick's approximation. A dielectric is barely reflective face-on and
        // almost a mirror at grazing incidence, which is why a lake shows its bed
        // at your feet and the sky at the far shore.
        let cos_theta = max(dot(facing, view), 0.0);
        let fresnel = f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
        // The tint multiplies rather than replaces, now that there is a real
        // image to tint; `[1, 1, 1]` (the default) leaves it untouched.
        rgb = mix(rgb, reflected * in.fresnel_tint, fresnel);
        // A reflective surface is also a less transparent one: what you see at a
        // grazing angle is the reflection, not what lies beneath.
        alpha = mix(alpha, 1.0, fresnel);
    }

    // Specular last, so the sun glint sits *on* the surface rather than being
    // mixed away by the reflection above it.
    if (specular > 0.0) {
        let light = normalize(LIGHT_DIR);
        // Blinn-Phong: the half-vector formulation, which stays stable at grazing
        // angles where the reflect() form collapses.
        let half_vec = normalize(light + view);
        // Gated on the light actually reaching this face, or a surface turned
        // away from the sun still gets a highlight through its own back.
        let lit = step(0.0, dot(facing, light));
        rgb += specular * lit * pow(max(dot(facing, half_vec), 0.0), shininess);
    }

    return vec4<f32>(rgb, alpha);
}

// The direction a surface mirrors, given the shading normal.
fn mirror_dir(in: VertexOutput, normal: vec3<f32>) -> vec3<f32> {
    let view = normalize(camera.eye.xyz - in.world_position);
    let facing = select(-normal, normal, dot(normal, view) >= 0.0);
    return reflect(-view, facing);
}

// Opaque geometry. Reflects the sky and nothing else, which costs one function
// call and means a polished floor or a wet rock still gets a believable edge
// without the scene texture the water needs.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = shading_normal(in);
    let rgb = diffuse_rgb(in, normal);
    return view_terms(in, normal, rgb, in.color.a, sky_color(mirror_dir(in, normal)));
}

// Transparent geometry, with the two terms that read the opaque scene behind it.
@fragment
fn fs_water(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = shading_normal(in);
    var rgb = diffuse_rgb(in, normal);
    var alpha = in.color.a;

    let refraction = in.water.x;
    let density = in.water.y;
    let reflection = in.water.z;

    // Where this fragment is on screen, which is what indexes the scene texture.
    // `clip_position` in the fragment stage is already in pixels.
    let screen_uv = in.clip_position.xy / vec2<f32>(textureDimensions(scene_color));

    // --- Refraction: what is behind the surface, displaced -------------------
    //
    // The cue that reads at *every* camera angle, unlike the reflection below.
    // Measured at the terrain demo's default view the surface is only ~3%
    // reflective, so this is the term doing most of the work of looking wet.
    if (refraction > 0.0) {
        // Displace the sample by the surface's horizontal tilt — the ripples,
        // mostly. Real refraction bends by Snell's law along the view ray; this
        // is the screen-space stand-in, and at these angles the difference is not
        // visible.
        //
        // **Scaled by how much water is actually here**, and that factor is a fix
        // rather than a refinement. This branch ends by forcing `alpha = 1.0` and
        // compositing the scene itself, so a fragment with almost no water on it
        // is still an *opaque* fragment — and at full displacement it paints a
        // copy of the scene fetched from tens of pixels away. Where a surface
        // fades out over a few pixels that is invisible; where it is *mostly*
        // fade it is the whole surface, and the terrain demo's rivers are exactly
        // that (a channel a cell wide is feathered edge nearly all the way
        // across). They came out as bundles of fine smeared streaks with dry
        // ground showing between them, which is what a screen-space offset
        // dragging a displaced copy of the ground looks like. Displacing in
        // proportion to coverage means a surface that is not there does not move
        // anything, and a lake still refracts at full strength.
        var uv = clamp(
            screen_uv + vec2<f32>(normal.x, normal.z) * refraction * in.color.a,
            vec2<f32>(0.0),
            vec2<f32>(1.0),
        );
        var behind = depth_at(uv);
        // A displaced sample can land on geometry *in front of* the water — a
        // rock standing in the lake — and smear it across the surface before it.
        // Falling back to the undisplaced sample is the standard fix and costs
        // only the ripple distortion in a thin band around such objects.
        if (behind < in.clip_position.z) {
            uv = screen_uv;
            behind = depth_at(uv);
        }
        let refracted = textureSampleLevel(scene_color, scene_sampler, uv, 0.0).rgb;
        // Beer-Lambert: how much water the light came through decides how much of
        // the water's own colour it picked up. This is what makes a deep basin
        // read as deep rather than as the same blue everywhere.
        let thickness = distance(world_from_depth(uv, behind), in.world_position);
        let absorbed = 1.0 - exp(-thickness * density);
        // ...and this `max` is what stops absorption being the *only* thing
        // deciding, which was a bug before it was a line of code. Terrain's lakes
        // are around 0.004 world units deep by the time the demo is interesting,
        // so Beer-Lambert alone returns ~2% water and 98% "exactly the scene
        // behind" — a surface that composites to the pixel already there, which
        // is to say an invisible one. The authored alpha is the *other* measure
        // of how much water is present (terrain's is a wetness field sized
        // against the shallowest water worth seeing), and taking whichever is
        // larger means a deep basin is carried by its depth and a shallow lake by
        // its coverage.
        let cover = clamp(max(in.color.a, absorbed), 0.0, 1.0);
        rgb = mix(refracted, rgb, cover);
        // Compositing happened here rather than in the blend unit, so the surface
        // is now opaque: `refracted` already *is* what is behind it. Leaving alpha
        // below 1.0 would blend that against the undisplaced scene a second time
        // and quietly halve the refraction.
        alpha = 1.0;
    }

    // --- Reflection: what is in front of the surface, mirrored ---------------
    //
    // The sky is the floor of this, not a fallback of last resort: it is what a
    // ray leaving the screen genuinely should see, and most rays off a near-flat
    // surface do exactly that. The screen-space trace then *upgrades* the rays
    // that find geometry — a far bank mirrored in the lake below it — and fades
    // back to sky at the frame edge, so there is no seam where it gives up.
    let dir = mirror_dir(in, normal);
    var reflected = sky_color(dir);
    if (reflection > 0.0) {
        // Trace along a *calmer* normal than the one that shades the surface.
        // The two want opposite things from the ripples: the specular highlight
        // needs the full tilt or the glints vanish, while a traced ray needs
        // coherence with its neighbours — a fully rippled normal sends adjacent
        // pixels to unrelated parts of the scene, and since each independently
        // either finds geometry or does not, the result is binary speckle rather
        // than a reflection. Real water does distort what it mirrors, so this
        // keeps some; it just cannot keep all of it and stay coherent.
        let calm = normalize(mix(normalize(in.world_normal), normal, 0.25));
        let trace_dir = mirror_dir(in, calm);
        let hit = trace_reflection(in.world_position, trace_dir);
        reflected = mix(reflected, hit.rgb, hit.w * reflection);
    }

    return view_terms(in, normal, rgb, alpha, reflected);
}
