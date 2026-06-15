// Shaded triangle-mesh shader — the wgpu analogue of silx
// `scene/primitives.py` `Mesh3D` lit by `scene/function.py` `DirectionalLight`.
//
// silx shades plot3d meshes with a *headlight*: a directional Phong light fixed
// in camera space pointing into the screen (direction (0,0,-1)), ambient 0.3,
// diffuse 0.7, no specular (shininess 0) — the `Plot3DWidget` defaults. The
// shading therefore follows the camera as the scene is orbited, so it must be
// computed per-frame on the GPU. The normal is carried into camera space by the
// view matrix (`normal_mat`); positions are projected by the usual clip MVP.
//
// The light parameters are silx's viewport defaults, baked in as constants here
// (a lighting on/off / parameter API is a later enhancement).

struct MeshParams {
    // Clip-space MVP (proj × view × model), depth-corrected (as scene3d.wgsl).
    mvp: mat4x4<f32>,
    // Camera-space transform for normals: the view matrix (model is identity —
    // items bake world-space vertices — and the view is rigid, so its 3×3 is its
    // own inverse-transpose; w = 0 drops the translation).
    normal_mat: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> params: MeshParams;

// silx Plot3DWidget DirectionalLight defaults (camera space).
const LIGHT_DIR: vec3<f32> = vec3<f32>(0.0, 0.0, -1.0);
const AMBIENT: f32 = 0.3;
const DIFFUSE: f32 = 0.7;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal_cam: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) normal: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = params.mvp * vec4<f32>(pos, 1.0);
    out.normal_cam = (params.normal_mat * vec4<f32>(normal, 0.0)).xyz;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal_cam);
    // One-sided Lambert term (silx `max(0.0, dot(normal, -lightDir))`).
    let n_dot_l = max(0.0, dot(n, -LIGHT_DIR));
    let factor = AMBIENT + DIFFUSE * n_dot_l;
    // `color` is linear, premultiplied alpha; scaling rgb by the lighting factor
    // keeps it premultiplied (alpha unchanged), as silx leaves color.a untouched.
    return vec4<f32>(in.color.rgb * factor, in.color.a);
}
