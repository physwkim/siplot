//! The plot model.
//!
//! Holds the identifier, data-area background, data limits, margins, and the
//! optional colormap used to draw the colorbar. The item list, log/inverted
//! axis flags, and dirty tracking are added in later steps
//! (`doc/design.md` §1·§4·§11).

use egui::{Color32, Rect};

use crate::core::colormap::Colormap;
use crate::core::marker::Marker;
use crate::core::roi::Roi;
use crate::core::shape::Shape;
use crate::core::transform::{Axis, Margins, Scale, Transform, keep_aspect_limits};
use crate::core::triangles::Triangles;

/// Per-axis pan/zoom range constraints mirroring silx
/// `Axis.setRangeConstraints` / `Axis.setLimitsConstraints`.
///
/// All fields are optional; `None` means unconstrained. Applied by the
/// interaction helpers after every pan/zoom so the display range always
/// satisfies all set constraints.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisConstraints {
    /// Minimum allowed span (display range). Prevents zooming in past this.
    pub min_range: Option<f64>,
    /// Maximum allowed span (display range). Prevents zooming out past this.
    pub max_range: Option<f64>,
    /// Minimum allowed lower bound. Prevents panning the view below this value.
    pub min_pos: Option<f64>,
    /// Maximum allowed upper bound. Prevents panning the view above this value.
    pub max_pos: Option<f64>,
}

impl AxisConstraints {
    /// Return `(lo, hi)` clamped so all set constraints are satisfied. The
    /// span is corrected first (centered on the current midpoint), then the
    /// position window is clamped (shifting both ends equally).
    pub fn apply(self, lo: f64, hi: f64) -> (f64, f64) {
        let mut span = hi - lo;
        if span <= 0.0 {
            return (lo, hi);
        }

        // 1. Clamp the span.
        if let Some(min) = self.min_range
            && span < min
        {
            span = min;
        }
        if let Some(max) = self.max_range
            && span > max
        {
            span = max;
        }

        // 2. Re-center the clamped span on the original midpoint.
        let mid = (lo + hi) * 0.5;
        let mut new_lo = mid - span * 0.5;
        let mut new_hi = mid + span * 0.5;

        // 3. Clamp the position window (shift both ends to stay inside bounds).
        if let Some(min_pos) = self.min_pos
            && new_lo < min_pos
        {
            let shift = min_pos - new_lo;
            new_lo += shift;
            new_hi += shift;
        }
        if let Some(max_pos) = self.max_pos
            && new_hi > max_pos
        {
            let shift = new_hi - max_pos;
            new_lo -= shift;
            new_hi -= shift;
        }

        // 4. Final sanity — keep lo < hi even if constraints are contradictory.
        if new_lo >= new_hi {
            return (lo, hi);
        }

        (new_lo, new_hi)
    }

    /// `true` when all fields are `None` (no constraints set).
    pub fn is_unconstrained(self) -> bool {
        self.min_range.is_none()
            && self.max_range.is_none()
            && self.min_pos.is_none()
            && self.max_pos.is_none()
    }
}

/// Identifier for a single `Plot` instance.
///
/// `egui_wgpu`'s `callback_resources` is a global type map, so multi-plot keeps
/// per-plot GPU state separated by `PlotId` (`doc/design.md` §3.1·§12). The
/// current steps handle a single plot, so no separation map exists yet.
pub type PlotId = u64;

/// Grid lines drawn in the plot data area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphGrid {
    /// No grid lines.
    None,
    /// Major tick grid lines only.
    #[default]
    Major,
    /// Major and minor tick grid lines.
    MajorAndMinor,
}

impl GraphGrid {
    /// Whether major grid lines are drawn.
    pub fn major(self) -> bool {
        matches!(self, Self::Major | Self::MajorAndMinor)
    }

    /// Whether minor grid lines are drawn.
    pub fn minor(self) -> bool {
        matches!(self, Self::MajorAndMinor)
    }
}

/// One plot.
pub struct Plot {
    /// Instance identifier.
    pub id: PlotId,
    /// Data-area background color (maps to silx `setBackgroundColors`' data background).
    pub data_background: Color32,
    /// Data-space limits `(x_min, x_max, y_min, y_max)`.
    pub limits: (f64, f64, f64, f64),
    /// Margins reserving extra space inside the chrome gutters. Zero by default.
    pub margins: Margins,
    /// Colormap drawn as the colorbar (mirrors the displayed image's colormap).
    /// `None` hides the colorbar (`doc/design.md` §5·§8).
    pub colormap: Option<Colormap>,
    /// Limits to restore on a double-click "reset". The widget captures the
    /// first observed `limits` here so the home view survives pan/zoom
    /// (`doc/design.md` §8·§11.6). `None` until the first frame.
    pub home_limits: Option<(f64, f64, f64, f64)>,
    /// X-axis scale (linear or log10) (`doc/design.md` §13 A3).
    pub x_scale: Scale,
    /// Y-axis scale (linear or log10).
    pub y_scale: Scale,
    /// Reverse the X-axis on-screen direction (`doc/design.md` §13 A2).
    pub x_inverted: bool,
    /// Reverse the Y-axis on-screen direction.
    pub y_inverted: bool,
    /// Keep data square on screen by expanding the tighter axis' display range
    /// (silx `setKeepDataAspectRatio`). Only honored when both axes are linear
    /// (`doc/design.md` §13 A4).
    pub keep_aspect: bool,
    /// Secondary right Y axis limits `(y2_min, y2_max)`, or `None` for no y2
    /// axis. Curves bound to [`crate::YAxis::Right`] are plotted against it and
    /// its ticks are drawn in the right gutter (linear, `doc/design.md` §13 A5).
    pub y2: Option<(f64, f64)>,
    /// Draw a crosshair + coordinate readout following the pointer when it is
    /// over the data area (silx `setGraphCursor`, `doc/design.md` §13 C1).
    pub crosshair: bool,
    /// Regions of interest drawn over the data area with draggable edge
    /// handles. Dragging an edge updates that ROI's bounds in place and the
    /// widget reports the changed index (`doc/design.md` §13 C3).
    pub rois: Vec<Roi>,
    /// Point / line markers drawn over the data area (silx `addMarker`). Each is
    /// a static overlay; the widget draws the list every frame.
    pub markers: Vec<Marker>,
    /// Polygon / rectangle / polyline / line shapes drawn over the data area
    /// (silx `addShape`). Static overlays drawn every frame.
    pub shapes: Vec<Shape>,
    /// Per-vertex-colored filled triangle meshes drawn in the data layer (silx
    /// `addTriangles`). Drawn every frame under the chrome.
    pub triangles: Vec<Triangles>,
    /// Graph title, drawn centered above the data area (silx `setGraphTitle`,
    /// `BackendBase.setGraphTitle`). `None` reserves no top space for it.
    pub title: Option<String>,
    /// X-axis label, drawn centered below the X tick labels (silx
    /// `setGraphXLabel`). `None` reserves no extra bottom space.
    pub x_label: Option<String>,
    /// Left Y-axis label, drawn rotated at the far left (silx `setGraphYLabel`).
    /// `None` reserves no extra left space.
    pub y_label: Option<String>,
    /// Right (y2) Y-axis label, drawn rotated at the far right; only shown when
    /// a [`Self::y2`] axis exists. `None` reserves no extra right space.
    pub y2_label: Option<String>,
    /// Foreground color override for axes/frame/ticks/labels (silx
    /// `setForegroundColor`). `None` follows the egui theme's text color.
    pub foreground: Option<Color32>,
    /// Grid-line color override (silx `setGridColor`). `None` uses a faint tint
    /// of the foreground color.
    pub grid_color: Option<Color32>,
    /// Grid lines drawn in the data area (`setGraphGrid`).
    pub grid: GraphGrid,
    /// Pan/zoom constraints for the X axis (silx `getXAxis().setRangeConstraints`).
    pub x_constraints: AxisConstraints,
    /// Pan/zoom constraints for the left Y axis (silx `getYAxis().setRangeConstraints`).
    pub y_constraints: AxisConstraints,
    /// Maximum number of major ticks on the X axis.  `None` uses the default
    /// (8).  The chrome calls `nice_ticks` with this value, so the actual count
    /// may be slightly lower to keep round step sizes.
    pub x_max_ticks: Option<usize>,
    /// Maximum number of major ticks on the left Y axis.  `None` uses the
    /// default (6).
    pub y_max_ticks: Option<usize>,
}

impl Plot {
    /// Create a plot with the given id, a default dark background, unit limits,
    /// no margins, and no colorbar.
    pub fn new(id: PlotId) -> Self {
        Self {
            id,
            data_background: Color32::from_rgb(16, 16, 24),
            limits: (0.0, 1.0, 0.0, 1.0),
            margins: Margins::ZERO,
            colormap: None,
            home_limits: None,
            x_scale: Scale::Linear,
            y_scale: Scale::Linear,
            x_inverted: false,
            y_inverted: false,
            keep_aspect: false,
            y2: None,
            crosshair: false,
            rois: Vec::new(),
            markers: Vec::new(),
            shapes: Vec::new(),
            triangles: Vec::new(),
            title: None,
            x_label: None,
            y_label: None,
            y2_label: None,
            foreground: None,
            grid_color: None,
            grid: GraphGrid::Major,
            x_constraints: AxisConstraints::default(),
            y_constraints: AxisConstraints::default(),
            x_max_ticks: None,
            y_max_ticks: None,
        }
    }

    /// Build the data↔screen transform for the given data-area rect, honoring
    /// the per-axis scale, inversion, and (linear-only) aspect-ratio lock.
    ///
    /// Aspect correction is derived here from the stable requested `limits`, so
    /// it is the same view used for rendering, chrome, and pointer mapping —
    /// and resizing never compounds the expansion (`doc/design.md` §13 A4).
    pub fn transform(&self, area: Rect) -> Transform {
        let linear = self.x_scale == Scale::Linear && self.y_scale == Scale::Linear;
        let (x_min, x_max, y_min, y_max) = if self.keep_aspect && linear {
            keep_aspect_limits(self.limits, area)
        } else {
            self.limits
        };
        let x = Axis {
            min: x_min,
            max: x_max,
            scale: self.x_scale,
            inverted: self.x_inverted,
        };
        let y = Axis {
            min: y_min,
            max: y_max,
            scale: self.y_scale,
            inverted: self.y_inverted,
        };
        Transform::with_axes(x, y, area)
    }

    /// Build the transform for the secondary right (y2) axis, sharing the left
    /// transform's X axis exactly (including any aspect expansion) so curves on
    /// both axes stay aligned in X. `None` when the plot has no y2 axis. The y2
    /// axis is linear, non-inverted (`doc/design.md` §13 A5).
    pub fn transform_y2(&self, area: Rect) -> Option<Transform> {
        let (y2_min, y2_max) = self.y2?;
        let left = self.transform(area);
        let y2 = Axis::linear(y2_min, y2_max);
        Some(Transform::with_axes(left.x, y2, area))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn area() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0))
    }

    #[test]
    fn axis_constraints_unconstrained_is_passthrough() {
        let c = AxisConstraints::default();
        assert_eq!(c.apply(0.0, 10.0), (0.0, 10.0));
        assert!(c.is_unconstrained());
    }

    #[test]
    fn axis_constraints_min_range_widens_span() {
        let c = AxisConstraints {
            min_range: Some(5.0),
            ..Default::default()
        };
        // Current span is 2.0, below min; should be widened to 5.0 centered on 1.0.
        let (lo, hi) = c.apply(0.0, 2.0);
        assert!((hi - lo - 5.0).abs() < 1e-10, "span={}", hi - lo);
        assert!(((lo + hi) / 2.0 - 1.0).abs() < 1e-10); // centered on original mid
    }

    #[test]
    fn axis_constraints_max_range_narrows_span() {
        let c = AxisConstraints {
            max_range: Some(5.0),
            ..Default::default()
        };
        // Current span is 10.0, above max; should be narrowed to 5.0 centered on 5.0.
        let (lo, hi) = c.apply(0.0, 10.0);
        assert!((hi - lo - 5.0).abs() < 1e-10, "span={}", hi - lo);
        assert!(((lo + hi) / 2.0 - 5.0).abs() < 1e-10);
    }

    #[test]
    fn axis_constraints_min_pos_shifts_window_right() {
        let c = AxisConstraints {
            min_pos: Some(2.0),
            ..Default::default()
        };
        // View [0, 4] would place lo below min_pos=2; shift right so lo=2.
        let (lo, hi) = c.apply(0.0, 4.0);
        assert!((lo - 2.0).abs() < 1e-10, "lo={lo}");
        assert!((hi - 6.0).abs() < 1e-10, "hi={hi}");
    }

    #[test]
    fn axis_constraints_max_pos_shifts_window_left() {
        let c = AxisConstraints {
            max_pos: Some(8.0),
            ..Default::default()
        };
        // View [6, 12] places hi above max_pos=8; shift left so hi=8.
        let (lo, hi) = c.apply(6.0, 12.0);
        assert!((hi - 8.0).abs() < 1e-10, "hi={hi}");
        assert!((lo - 2.0).abs() < 1e-10, "lo={lo}");
    }

    #[test]
    fn axis_constraints_degenerate_span_is_passthrough() {
        let c = AxisConstraints {
            min_range: Some(1.0),
            ..Default::default()
        };
        // Already-invalid spans return unchanged (guard against further corruption).
        assert_eq!(c.apply(5.0, 3.0), (5.0, 3.0));
    }

    #[test]
    fn transform_y2_is_none_without_y2_axis() {
        let plot = Plot::new(0);
        assert!(plot.transform_y2(area()).is_none());
    }

    #[test]
    fn transform_y2_shares_left_x_and_maps_its_own_y() {
        let mut plot = Plot::new(0);
        plot.limits = (0.0, 10.0, 0.0, 100.0);
        plot.y2 = Some((-1.0, 1.0));
        let left = plot.transform(area());
        let right = plot.transform_y2(area()).expect("y2 transform");

        // X axis is shared exactly, so curves on both axes align in X.
        assert_eq!(left.x, right.x);
        // The right axis maps its own y2 range: y2_min at the bottom edge, y2_max
        // at the top edge of the same area.
        let bottom = right.data_to_pixel(0.0, -1.0).y;
        let top = right.data_to_pixel(0.0, 1.0).y;
        assert!((bottom - area().bottom()).abs() <= 1e-3, "{bottom}");
        assert!((top - area().top()).abs() <= 1e-3, "{top}");
    }

    #[test]
    fn transform_y2_shares_aspect_expanded_x() {
        // With the aspect lock on, the left transform's X is expanded; the y2
        // transform must inherit that same expanded X (not the raw limits).
        let mut plot = Plot::new(0);
        plot.limits = (0.0, 10.0, 0.0, 10.0);
        plot.keep_aspect = true;
        plot.y2 = Some((0.0, 5.0));
        let left = plot.transform(area());
        let right = plot.transform_y2(area()).expect("y2 transform");
        assert_eq!(left.x, right.x);
        // Sanity: the lock actually widened X beyond the raw [0, 10].
        assert!(left.x.min < 0.0 && left.x.max > 10.0, "{:?}", left.x);
    }
}
