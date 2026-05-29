// Colormapped 2D scalar image (slice 1, step 3).
//
// A quad covering the image's data-space rect is generated from the vertex
// index, transformed to NDC by the ortho matrix (data -> NDC, the single
// source of truth in core::transform). The fragment samples the scalar data
// texture (nearest), normalizes against clim, and looks up the color in a
// 256x1 sRGB LUT texture (linear). Linear normalization only for now.

struct Params {
    ortho: mat4x4<f32>,
    rect: vec4<f32>,   // data-space extent: (x0, y0, x1, y1)
    clim: vec2<f32>,   // (vmin, vmax)
    alpha: f32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var data_tex: texture_2d<f32>;   // R32Float, unfilterable
@group(0) @binding(2) var data_samp: sampler;          // non-filtering (nearest)
@group(0) @binding(3) var lut_tex: texture_2d<f32>;    // 256x1 sRGB
@group(0) @binding(4) var lut_samp: sampler;           // filtering (linear)

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Two triangles forming the unit quad in [0,1]^2.
    var verts = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let t = verts[vid];

    // t -> data space (rect.xy = lower-left, rect.zw = upper-right), then NDC.
    let dx = mix(params.rect.x, params.rect.z, t.x);
    let dy = mix(params.rect.y, params.rect.w, t.y);

    var out: VsOut;
    out.pos = params.ortho * vec4<f32>(dx, dy, 0.0, 1.0);
    // uv.y = 0 at the bottom vertex, so texture row 0 (data[0]) is at the
    // bottom: y increases upward (matplotlib origin='lower' / silx convention).
    out.uv = t;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let v = textureSample(data_tex, data_samp, in.uv).r;
    let tnorm = clamp((v - params.clim.x) / (params.clim.y - params.clim.x), 0.0, 1.0);
    let rgb = textureSample(lut_tex, lut_samp, vec2<f32>(tnorm, 0.5)).rgb;
    return vec4<f32>(rgb, params.alpha);
}
