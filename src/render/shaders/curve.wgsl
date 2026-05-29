// Polyline shader: expand each segment of a data-space polyline into a
// screen-space quad of a given pixel width, transformed to clip space via the
// shared data->NDC ortho matrix and filled with a single color.
//
// The points live in a read-only storage buffer; a non-instanced draw of
// 6 * (segment count) vertices builds two triangles per segment. Offsetting in
// pixel space (using the data-area viewport size) keeps the width uniform
// regardless of the data aspect ratio. Butt caps, no joins — for finely sampled
// curves the per-segment gap at a turn is sub-pixel; round joins/caps and
// anti-aliasing are later steps (doc/design.md §7·§13 B1).

struct Params {
    ortho: mat4x4<f32>,
    // Linear, premultiplied RGBA (already alpha-multiplied on the CPU side).
    color: vec4<f32>,
    axis_log: vec2<f32>,      // 1.0 if that axis is log10, else 0.0
    viewport_px: vec2<f32>,   // data-area size in physical pixels
    half_width_px: f32,       // half the line width, in physical pixels
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> points: array<vec2<f32>>;

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

fn to_ndc(p: vec2<f32>) -> vec2<f32> {
    let clip = params.ortho * vec4<f32>(apply_scale(p), 0.0, 1.0);
    return clip.xy / clip.w;
}

// Per-corner endpoint selector (0 = segment start, 1 = segment end) and the
// perpendicular offset side, for the two triangles (start-, start+, end-) and
// (end-, start+, end+).
const ENDPOINT = array<u32, 6>(0u, 0u, 1u, 1u, 0u, 1u);
const SIDE = array<f32, 6>(-1.0, 1.0, -1.0, -1.0, 1.0, 1.0);

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    let seg = vid / 6u;
    let corner = vid % 6u;

    let half_vp = params.viewport_px * 0.5;
    // Endpoints in pixel space (NDC scaled by half the viewport).
    let px0 = to_ndc(points[seg]) * half_vp;
    let px1 = to_ndc(points[seg + 1u]) * half_vp;

    // Perpendicular unit vector in pixels; degenerate (zero-length) segments
    // collapse to a zero offset and draw nothing.
    let delta = px1 - px0;
    let len = length(delta);
    var normal = vec2<f32>(0.0, 0.0);
    if (len > 1e-6) {
        let dir = delta / len;
        normal = vec2<f32>(-dir.y, dir.x);
    }

    let base = select(px0, px1, ENDPOINT[corner] == 1u);
    let pos_px = base + normal * (params.half_width_px * SIDE[corner]);
    // Back to NDC.
    return vec4<f32>(pos_px / half_vp, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.color;
}
