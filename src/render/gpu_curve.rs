//! GPU-side curve: the shared curve pipeline and uploaded polylines.
//!
//! [`CurveData`] is the CPU spec (mirrors silx `addCurve`: x/y arrays + color +
//! width + Y-axis binding). [`CurvePipeline`] holds the thick-line pipeline
//! shared across curves. [`GpuCurve`] owns one curve's point storage buffer +
//! uniform + bind group and persists across frames in `WgpuResources`.
//!
//! Each segment of the polyline is expanded in the vertex shader into a
//! screen-space quad (two triangles) of the curve's pixel width, so the line is
//! a uniform thickness regardless of the data aspect ratio. The points are read
//! from a read-only storage buffer; the draw is `6 × segment count` vertices,
//! no vertex buffer. In-place re-upload ([`GpuCurve::update`]) reuses the buffer
//! for live updates. Round joins/caps, anti-aliasing, per-vertex color, and
//! markers are later steps (`doc/design.md` §7·§13 B1).

use std::num::NonZeroU64;

use egui::Color32;
use egui_wgpu::wgpu;

use crate::core::transform::YAxis;

/// Identity ortho matrix; replaced every frame by the widget's transform.
const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Uniform block for the curve shader. Layout matches `Params` in `curve.wgsl`
/// (std140: mat4 @0, vec4 @64, vec2 @80, vec2 @88, f32 @96; padded to 112).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CurveParams {
    ortho: [[f32; 4]; 4],
    color: [f32; 4],
    /// 1.0 if that axis is log10, else 0.0 (x, y).
    axis_log: [f32; 2],
    /// Data-area size in physical pixels (for the pixel-space quad expansion).
    viewport_px: [f32; 2],
    /// Half the line width, in physical pixels.
    half_width_px: f32,
    _pad: [f32; 3],
}

/// A polyline to draw, in data coordinates. `x[i], y[i]` is vertex `i`; the
/// vertices are connected in order.
#[derive(Clone, Debug)]
pub struct CurveData {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub color: Color32,
    /// Line width in physical pixels (`doc/design.md` §12·§13 B1).
    pub width: f32,
    /// Which Y axis this curve is plotted against (left by default).
    pub y_axis: YAxis,
}

impl CurveData {
    /// Build a curve from equal-length x/y arrays with the given line color, a
    /// 1px width, plotted against the main left Y axis.
    pub fn new(x: Vec<f64>, y: Vec<f64>, color: Color32) -> Self {
        assert_eq!(x.len(), y.len(), "x and y must have equal length");
        Self {
            x,
            y,
            color,
            width: 1.0,
            y_axis: YAxis::Left,
        }
    }

    /// Set the line width in physical pixels (clamped to ≥ 0).
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(0.0);
        self
    }

    /// Bind this curve to the given Y axis (left or right/y2).
    pub fn with_y_axis(mut self, y_axis: YAxis) -> Self {
        self.y_axis = y_axis;
        self
    }
}

/// The render pipeline shared by all curves.
pub struct CurvePipeline {
    pub(crate) pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl CurvePipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("egui-silx curve"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/curve.wgsl").into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("egui-silx curve bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<CurveParams>() as u64
                            ),
                        },
                        count: None,
                    },
                    // The polyline points, read in the vertex shader for quad expansion.
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<[f32; 2]>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui-silx curve layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui-silx curve pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                // No vertex buffers: positions come from the storage buffer and
                // each vertex is derived from its index.
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            // Triangle list: two triangles (6 vertices) per polyline segment.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}

/// One uploaded curve's GPU resources, persisting across frames.
pub struct GpuCurve {
    points: wgpu::Buffer,
    count: u32,
    /// Points the buffer can hold; an in-place [`Self::update`] up to this many
    /// points reuses the buffer instead of reallocating.
    capacity: u32,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    color: [f32; 4],
    /// Line width in physical pixels.
    width: f32,
    /// Which Y axis this curve is bound to; selects the per-frame ortho matrix.
    pub(crate) y_axis: YAxis,
}

impl GpuCurve {
    /// Upload `curve`'s vertices and build its uniform + bind group.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &CurvePipeline,
        curve: &CurveData,
    ) -> Self {
        let positions: Vec<[f32; 2]> = curve
            .x
            .iter()
            .zip(&curve.y)
            .map(|(&x, &y)| [x as f32, y as f32])
            .collect();

        // max(1) keeps a zero-point curve from creating a zero-size buffer (also
        // satisfies the storage binding's nonzero min size); the draw is still
        // skipped (count < 2) so nothing is rendered.
        let capacity = positions.len().max(1) as u32;
        let points = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui-silx curve points"),
            size: (capacity as usize * std::mem::size_of::<[f32; 2]>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !positions.is_empty() {
            queue.write_buffer(&points, 0, bytemuck::cast_slice(&positions));
        }

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui-silx curve params"),
            size: std::mem::size_of::<CurveParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui-silx curve bg"),
            layout: &pipeline.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: points.as_entire_binding(),
                },
            ],
        });

        // sRGB Color32 -> linear, premultiplied RGBA (matches the alpha-blend target).
        let color = egui::Rgba::from(curve.color).to_array();

        let gpu = Self {
            points,
            count: positions.len() as u32,
            capacity,
            params,
            bind_group,
            color,
            width: curve.width,
            y_axis: curve.y_axis,
        };
        // Seed the uniform; the per-frame transform/viewport overwrite it.
        gpu.write_uniforms(queue, IDENTITY, [0.0, 0.0], [1.0, 1.0]);
        gpu
    }

    /// Re-upload `curve`'s vertices into the existing buffer in place (dirty
    /// update), reusing all GPU resources. Returns `false` if the new vertex
    /// count exceeds the allocated [`Self::capacity`], in which case the caller
    /// must reallocate (build a fresh [`GpuCurve`]).
    pub(crate) fn update(&mut self, queue: &wgpu::Queue, curve: &CurveData) -> bool {
        assert_eq!(
            curve.x.len(),
            curve.y.len(),
            "x and y must have equal length"
        );
        if curve.x.len() as u32 > self.capacity {
            return false;
        }
        let positions: Vec<[f32; 2]> = curve
            .x
            .iter()
            .zip(&curve.y)
            .map(|(&x, &y)| [x as f32, y as f32])
            .collect();
        if !positions.is_empty() {
            queue.write_buffer(&self.points, 0, bytemuck::cast_slice(&positions));
        }
        self.count = positions.len() as u32;
        self.color = egui::Rgba::from(curve.color).to_array();
        self.width = curve.width;
        self.y_axis = curve.y_axis;
        true
    }

    /// Update the per-frame data->NDC transform, axis-scale flags, and data-area
    /// pixel size (re-stamping the color and width too). `axis_log` is `[x, y]`
    /// with 1.0 for a log10 axis; `viewport_px` is the data area in physical
    /// pixels, used to keep the line width uniform in pixel space.
    pub(crate) fn write_uniforms(
        &self,
        queue: &wgpu::Queue,
        ortho: [[f32; 4]; 4],
        axis_log: [f32; 2],
        viewport_px: [f32; 2],
    ) {
        let params = CurveParams {
            ortho,
            color: self.color,
            axis_log,
            viewport_px,
            half_width_px: 0.5 * self.width,
            _pad: [0.0; 3],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
    }

    pub(crate) fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>, pipeline: &CurvePipeline) {
        // Need at least two points (one segment) to draw anything.
        if self.count < 2 {
            return;
        }
        // 6 vertices (two triangles) per segment; segment count = points - 1.
        let vertices = 6 * (self.count - 1);
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..vertices, 0..1);
    }
}
