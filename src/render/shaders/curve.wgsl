// Polyline shader: transform 2D data-space vertices to clip space via the
// shared data->NDC ortho matrix and fill with a single color.
//
// Drawn as a line strip with wgpu's fixed 1px line width. Thick lines (quad
// expansion) and per-vertex color are later steps (doc/design.md §7).

struct Params {
    ortho: mat4x4<f32>,
    // Linear, premultiplied RGBA (already alpha-multiplied on the CPU side).
    color: vec4<f32>,
    axis_log: vec2<f32>, // 1.0 if that axis is log10, else 0.0
};

@group(0) @binding(0) var<uniform> params: Params;

// 1 / ln(10), to turn the natural log into log10.
const INV_LN10: f32 = 0.4342944819032518;

// Map a data coordinate to the affine (transformed) space the ortho matrix
// expects: identity for a linear axis, log10 for a log axis. Must match
// core::transform::Axis::norm so chrome and shader agree (doc/design.md §4).
fn apply_scale(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        select(p.x, log(p.x) * INV_LN10, params.axis_log.x > 0.5),
        select(p.y, log(p.y) * INV_LN10, params.axis_log.y > 0.5),
    );
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    return params.ortho * vec4<f32>(apply_scale(pos), 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.color;
}
