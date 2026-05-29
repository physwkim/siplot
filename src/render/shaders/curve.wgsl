// Polyline shader: transform 2D data-space vertices to clip space via the
// shared data->NDC ortho matrix and fill with a single color.
//
// Drawn as a line strip with wgpu's fixed 1px line width. Thick lines (quad
// expansion) and per-vertex color are later steps (doc/design.md §7).

struct Params {
    ortho: mat4x4<f32>,
    // Linear, premultiplied RGBA (already alpha-multiplied on the CPU side).
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    return params.ortho * vec4<f32>(pos, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.color;
}
