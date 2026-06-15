//! 3D data items — the `silx.gui.plot3d.items` port.
//!
//! Items hold data plus presentation state (colormap, marker, size) and emit
//! their geometry into a [`Scene3dGeometry`] via [`append_to`](Scatter3D::append_to),
//! the analogue of silx's scene-primitive build. The GPU primitives themselves
//! live in [`crate::render::gpu_scene3d`]; this module is the headless item layer
//! (color mapping + bounds), unit-tested without a GPU.

use egui::Color32;

use crate::core::colormap::{AutoscaleMode, Colormap, ColormapName};
use crate::core::scene3d::mat4::Vec3;
use crate::render::gpu_scene3d::{PointMarker, Scene3dGeometry};

/// silx's default plot symbol size in pixels (`_config.DEFAULT_PLOT_SYMBOL_SIZE`).
pub const DEFAULT_SCATTER3D_SIZE: f32 = 6.0;

/// A 3D scatter plot: per-point `(x, y, z)` positions coloured by a per-point
/// `value` through a [`Colormap`], drawn as [`PointMarker`] sprites of one size.
///
/// Port of silx `plot3d.items.Scatter3D` (`DataItem3D` + `ColormapMixIn` +
/// `SymbolMixIn`). silx colours points on the GPU from a colormap texture; here
/// the mapping is done on the CPU via [`Colormap::color_at`] when building the
/// geometry — simpler, and points are few relative to image rasters.
#[derive(Clone, Debug)]
pub struct Scatter3D {
    x: Vec<f32>,
    y: Vec<f32>,
    z: Vec<f32>,
    values: Vec<f64>,
    colormap: Colormap,
    marker: PointMarker,
    size: f32,
}

impl Default for Scatter3D {
    fn default() -> Self {
        Self::new()
    }
}

impl Scatter3D {
    /// An empty scatter with silx defaults: the viridis colormap over `[0, 1]`,
    /// circle markers at [`DEFAULT_SCATTER3D_SIZE`].
    pub fn new() -> Self {
        Self {
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            values: Vec::new(),
            colormap: Colormap::new(ColormapName::Viridis, 0.0, 1.0),
            marker: PointMarker::Circle,
            size: DEFAULT_SCATTER3D_SIZE,
        }
    }

    /// Replace the point data (silx `Scatter3D.setData`). The four arrays must be
    /// the same length; on a length mismatch the data is left unchanged and
    /// `false` is returned (silx asserts equal lengths).
    pub fn set_data(&mut self, x: &[f32], y: &[f32], z: &[f32], values: &[f64]) -> bool {
        let n = x.len();
        if y.len() != n || z.len() != n || values.len() != n {
            return false;
        }
        self.x = x.to_vec();
        self.y = y.to_vec();
        self.z = z.to_vec();
        self.values = values.to_vec();
        true
    }

    /// Builder form of [`set_data`](Self::set_data); a length mismatch leaves the
    /// data empty.
    pub fn with_data(mut self, x: &[f32], y: &[f32], z: &[f32], values: &[f64]) -> Self {
        self.set_data(x, y, z, values);
        self
    }

    /// Set the colormap (silx `ColormapMixIn.setColormap`).
    pub fn set_colormap(&mut self, colormap: Colormap) {
        self.colormap = colormap;
    }

    /// Builder form of [`set_colormap`](Self::set_colormap).
    pub fn with_colormap(mut self, colormap: Colormap) -> Self {
        self.colormap = colormap;
        self
    }

    /// Read-only access to the colormap.
    pub fn colormap(&self) -> &Colormap {
        &self.colormap
    }

    /// Mutable access to the colormap (e.g. to set the value range directly).
    pub fn colormap_mut(&mut self) -> &mut Colormap {
        &mut self.colormap
    }

    /// Fit the colormap's value range to the current data with `mode` (silx's
    /// colormap autoscale over the value array), returning the new `(vmin, vmax)`.
    /// With no data the range falls back to the autoscale default, matching
    /// [`AutoscaleMode::range`].
    pub fn autoscale_colormap(&mut self, mode: AutoscaleMode) -> (f64, f64) {
        let (vmin, vmax) = mode.range(&self.values, self.colormap.autoscale_percentiles);
        self.colormap.vmin = vmin;
        self.colormap.vmax = vmax;
        (vmin, vmax)
    }

    /// Set the marker shape (silx `SymbolMixIn.setSymbol`).
    pub fn set_marker(&mut self, marker: PointMarker) {
        self.marker = marker;
    }

    /// Builder form of [`set_marker`](Self::set_marker).
    pub fn with_marker(mut self, marker: PointMarker) -> Self {
        self.marker = marker;
        self
    }

    /// Set the marker size in pixels (silx `SymbolMixIn.setSymbolSize`), clamped
    /// to be non-negative.
    pub fn set_size(&mut self, size: f32) {
        self.size = size.max(0.0);
    }

    /// Builder form of [`set_size`](Self::set_size).
    pub fn with_size(mut self, size: f32) -> Self {
        self.set_size(size);
        self
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// True when there are no points.
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Axis-aligned data bounds `(min, max)` over the points (silx
    /// `DataItem3D.getBounds`), or `None` when empty. Useful to frame a
    /// [`crate::widget::scene_widget::SceneWidget`].
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        if self.is_empty() {
            return None;
        }
        let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for i in 0..self.len() {
            let (px, py, pz) = (self.x[i], self.y[i], self.z[i]);
            min.x = min.x.min(px);
            min.y = min.y.min(py);
            min.z = min.z.min(pz);
            max.x = max.x.max(px);
            max.y = max.y.max(py);
            max.z = max.z.max(pz);
        }
        Some((min, max))
    }

    /// Append this scatter's points (coloured through the colormap) to
    /// `geometry`, ready to upload via [`crate::render::gpu_scene3d::set_scene3d`].
    pub fn append_to(&self, geometry: &mut Scene3dGeometry) {
        for i in 0..self.len() {
            let [r, g, b, a] = self.colormap.color_at(self.values[i]);
            geometry.add_point(
                [self.x[i], self.y[i], self.z[i]],
                Color32::from_rgba_unmultiplied(r, g, b, a),
                self.size,
                self.marker,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_data_rejects_length_mismatch() {
        let mut s = Scatter3D::new();
        assert!(!s.set_data(&[0.0, 1.0], &[0.0], &[0.0, 1.0], &[0.0, 1.0]));
        assert!(s.is_empty(), "rejected data must not be partially stored");
        assert!(s.set_data(&[0.0, 1.0], &[2.0, 3.0], &[4.0, 5.0], &[6.0, 7.0]));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn append_to_colours_each_point_through_the_colormap() {
        // A ramp colormap over [0, 4]: value 0 → LUT index 0, value 4 → index 255.
        let cmap = Colormap::new(ColormapName::Viridis, 0.0, 4.0);
        let s = Scatter3D::new()
            .with_colormap(cmap.clone())
            .with_marker(PointMarker::Square)
            .with_size(8.0)
            .with_data(
                &[0.0, 1.0, 2.0],
                &[0.0, 0.0, 0.0],
                &[0.0, 0.0, 0.0],
                &[0.0, 2.0, 4.0],
            );

        let mut g = Scene3dGeometry::new();
        s.append_to(&mut g);

        // One point per datum, each at its position, all square, all size 8.
        assert_eq!(g.points.len(), 3);
        assert_eq!(g.points[1].pos, [1.0, 0.0, 0.0]);
        for p in &g.points {
            assert_eq!(p.size, 8.0);
            assert_eq!(p.marker, PointMarker::Square.id());
        }

        // Colors match the colormap LUT lookup (premultiplied at upload).
        let expect = |v: f64| {
            let [r, gg, b, a] = cmap.color_at(v);
            egui::Rgba::from(Color32::from_rgba_unmultiplied(r, gg, b, a)).to_array()
        };
        assert_eq!(g.points[0].color, expect(0.0));
        assert_eq!(g.points[2].color, expect(4.0));
        // The endpoints differ (the value actually drives the color).
        assert_ne!(g.points[0].color, g.points[2].color);
    }

    #[test]
    fn autoscale_colormap_fits_value_range() {
        let mut s =
            Scatter3D::new().with_data(&[0.0, 1.0, 2.0], &[0.0; 3], &[0.0; 3], &[-5.0, 0.0, 10.0]);
        let (vmin, vmax) = s.autoscale_colormap(AutoscaleMode::MinMax);
        assert_eq!((vmin, vmax), (-5.0, 10.0));
        assert_eq!(s.colormap().vmin, -5.0);
        assert_eq!(s.colormap().vmax, 10.0);
    }

    #[test]
    fn bounds_brackets_the_points() {
        assert!(Scatter3D::new().bounds().is_none());
        let s = Scatter3D::new().with_data(
            &[-1.0, 2.0, 0.5],
            &[3.0, -2.0, 1.0],
            &[0.0, 4.0, -1.0],
            &[0.0; 3],
        );
        let (min, max) = s.bounds().expect("non-empty bounds");
        assert_eq!((min.x, min.y, min.z), (-1.0, -2.0, -1.0));
        assert_eq!((max.x, max.y, max.z), (2.0, 3.0, 4.0));
    }
}
