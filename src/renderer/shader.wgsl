// Scene shader: transforms vertices by the instance's model matrix and then the
// camera's view-projection matrix, and lights them in world space.
//
// Lighting lives here rather than in the consumer because a demo that bakes
// shading into its vertex colors is only correct while the mesh never moves —
// rotate such an object and the highlight turns with it. Evaluating the light
// *after* the model transform is the whole point.

struct CameraUniform {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// One directional light plus ambient, Lambert diffuse. Deliberately fixed rather
// than exposed: no demo has asked to move the sun, and a setter with no caller is
// the speculative build the roadmap forbids. These are the exact constants the
// terrain demo used to bake in by hand, so retiring that bake changes nothing on
// screen.
const LIGHT_DIR: vec3<f32> = vec3<f32>(0.45, 0.85, 0.35);
const AMBIENT: f32 = 0.35;
const DIFFUSE: f32 = 0.65;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
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
    // was authored into its corners and gets recolored on top.
    out.color = vec4<f32>(in.color, 1.0) * instance.tint;
    // Normalized in the fragment stage instead, since interpolating across a
    // triangle shortens it.
    out.world_normal = normal_matrix * in.normal;
    // Object space -> world space -> clip space.
    out.clip_position = camera.view_proj * model * vec4<f32>(in.position, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    let lambert = max(dot(normal, normalize(LIGHT_DIR)), 0.0);
    let shade = AMBIENT + DIFFUSE * lambert;
    // Alpha is untouched by the light: a translucent surface does not become
    // more opaque because it faces the sun.
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}
