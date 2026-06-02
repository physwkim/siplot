//! Shapes: polygon / rectangle / polyline / horizontal-line / vertical-line
//! annotations drawn over the data area (silx `BackendBase.addShape`).
//!
//! Like [`crate::core::marker::Marker`], a shape is a data-space overlay with
//! pure screen-placement math (unit-testable); the widget's chrome draws the
//! list each frame via [`crate::widget::chrome::draw_shapes`]. silx's `overlay`
//! flag chooses between the data layer and a separate overlay layer; here every
//! shape draws in the single overlay pass (over the chrome, like an ROI), so the
//! flag is not modeled (`doc/design.md` §8).

use egui::{Color32, Pos2};

use crate::core::items::LineStyle;
use crate::core::transform::Transform;

/// The geometry a [`Shape`] draws (silx `addShape` `shape`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// A closed polygon through the points `(x[i], y[i])`. silx `'polygon'`.
    Polygon,
    /// An axis-aligned rectangle. Built from two corner points stored as
    /// `x = [x0, x1]`, `y = [y0, y1]`. silx `'rectangle'`.
    Rectangle,
    /// An open polyline through the points `(x[i], y[i])`. silx `'polylines'`.
    Polyline,
    /// One full-width horizontal line per entry in `y` (`x` unused). silx `'hline'`.
    HLine,
    /// One full-height vertical line per entry in `x` (`y` unused). silx `'vline'`.
    VLine,
}

/// A shape drawn over the data area (silx `BackendBase.addShape`).
///
/// `fill` is honored for [`ShapeKind::Polygon`] / [`ShapeKind::Rectangle`].
/// **Fill is convex-only**: egui's polygon fill (`Shape::convex_polygon`) is
/// correct for rectangles and convex polygons but renders a concave polygon's
/// fill as its convex interpretation. The outline (and all line kinds) honor
/// `line_style` / `line_width`, with `gap_color` filling dash gaps (silx
/// `gapcolor`).
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// What geometry this shape draws.
    pub kind: ShapeKind,
    /// Data X coordinates of the shape's points (see [`ShapeKind`] for which
    /// array each kind reads).
    pub x: Vec<f64>,
    /// Data Y coordinates of the shape's points.
    pub y: Vec<f64>,
    /// Outline and fill color (silx `color`).
    pub color: Color32,
    /// Fill the interior (silx `fill`); honored for `Polygon` / `Rectangle`.
    pub fill: bool,
    /// Outline stroke style (silx `linestyle`).
    pub line_style: LineStyle,
    /// Outline width in logical points (silx `linewidth`).
    pub line_width: f32,
    /// Second color filling dash gaps in the outline (silx `gapcolor`); `None`
    /// leaves the gaps empty.
    pub gap_color: Option<Color32>,
    /// Whether the shape draws in the overlay pass (silx `_OverlayItem.isOverlay`
    /// / `setOverlay`, `shape.py:54-73`).
    ///
    /// Defaults to `true`, the port's current behavior: every shape draws in the
    /// single overlay pass (over the chrome, like an ROI). This differs from
    /// silx's `_OverlayItem` default of `False` (the data layer); the port has no
    /// separate data layer for shapes, so the field is carried for parity and for
    /// a future renderer that honors it without changing today's draw path.
    pub is_overlay: bool,
}

impl Shape {
    /// A closed polygon through `(x[i], y[i])`. Panics if `x` and `y` differ in
    /// length.
    pub fn polygon(x: Vec<f64>, y: Vec<f64>) -> Self {
        assert_eq!(
            x.len(),
            y.len(),
            "polygon x and y must have the same length"
        );
        Self::with_points(ShapeKind::Polygon, x, y)
    }

    /// An axis-aligned rectangle between corners `(x0, y0)` and `(x1, y1)`.
    pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self::with_points(ShapeKind::Rectangle, vec![x0, x1], vec![y0, y1])
    }

    /// An open polyline through `(x[i], y[i])`. Panics if `x` and `y` differ in
    /// length.
    pub fn polyline(x: Vec<f64>, y: Vec<f64>) -> Self {
        assert_eq!(
            x.len(),
            y.len(),
            "polyline x and y must have the same length"
        );
        Self::with_points(ShapeKind::Polyline, x, y)
    }

    /// One full-width horizontal line at each y value (silx `'hline'`).
    pub fn hlines(y: Vec<f64>) -> Self {
        Self::with_points(ShapeKind::HLine, Vec::new(), y)
    }

    /// One full-height vertical line at each x value (silx `'vline'`).
    pub fn vlines(x: Vec<f64>) -> Self {
        Self::with_points(ShapeKind::VLine, x, Vec::new())
    }

    fn with_points(kind: ShapeKind, x: Vec<f64>, y: Vec<f64>) -> Self {
        Self {
            kind,
            x,
            y,
            color: Color32::WHITE,
            fill: false,
            line_style: LineStyle::Solid,
            line_width: 1.0,
            gap_color: None,
            is_overlay: true,
        }
    }

    /// Set the outline / fill color.
    pub fn with_color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    /// Fill the interior (`Polygon` / `Rectangle` only).
    pub fn with_fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Set the outline stroke style.
    pub fn with_line_style(mut self, style: LineStyle) -> Self {
        self.line_style = style;
        self
    }

    /// Set the outline width.
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.line_width = width;
        self
    }

    /// Set the dash-gap fill color (silx `gapcolor`).
    pub fn with_gap_color(mut self, color: Color32) -> Self {
        self.gap_color = Some(color);
        self
    }

    /// Set whether the shape draws in the overlay pass (silx
    /// `_OverlayItem.setOverlay`).
    pub fn with_overlay(mut self, overlay: bool) -> Self {
        self.is_overlay = overlay;
        self
    }

    /// Screen-space vertices for the area-shaped kinds: the four corners of a
    /// [`ShapeKind::Rectangle`], or each `(x[i], y[i])` of a
    /// [`ShapeKind::Polygon`] / [`ShapeKind::Polyline`]. Empty for the line kinds,
    /// whose lines span the data area and are placed at draw time.
    pub fn screen_points(&self, t: &Transform) -> Vec<Pos2> {
        match self.kind {
            ShapeKind::Rectangle => {
                if self.x.len() < 2 || self.y.len() < 2 {
                    return Vec::new();
                }
                let (x0, x1, y0, y1) = (self.x[0], self.x[1], self.y[0], self.y[1]);
                vec![
                    t.data_to_pixel(x0, y0),
                    t.data_to_pixel(x1, y0),
                    t.data_to_pixel(x1, y1),
                    t.data_to_pixel(x0, y1),
                ]
            }
            ShapeKind::Polygon | ShapeKind::Polyline => self
                .x
                .iter()
                .zip(&self.y)
                .map(|(&x, &y)| t.data_to_pixel(x, y))
                .collect(),
            ShapeKind::HLine | ShapeKind::VLine => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};

    // 100×100 px area mapping data [0,10]×[0,10]; 1 data unit = 10 px, y flipped.
    fn t() -> Transform {
        Transform::new(
            0.0,
            10.0,
            0.0,
            10.0,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
        )
    }

    #[test]
    fn constructors_set_kind_and_defaults() {
        let p = Shape::polygon(vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 0.0]);
        assert_eq!(p.kind, ShapeKind::Polygon);
        assert_eq!(p.color, Color32::WHITE);
        assert!(!p.fill);
        assert_eq!(p.line_style, LineStyle::Solid);
        assert_eq!(p.line_width, 1.0);
        assert!(p.gap_color.is_none());

        assert_eq!(
            Shape::rectangle(0.0, 0.0, 1.0, 1.0).kind,
            ShapeKind::Rectangle
        );
        assert_eq!(
            Shape::polyline(vec![0.0], vec![0.0]).kind,
            ShapeKind::Polyline
        );
        assert_eq!(Shape::hlines(vec![1.0, 2.0]).kind, ShapeKind::HLine);
        assert_eq!(Shape::vlines(vec![1.0, 2.0]).kind, ShapeKind::VLine);
    }

    #[test]
    #[should_panic(expected = "polygon x and y must have the same length")]
    fn polygon_rejects_length_mismatch() {
        Shape::polygon(vec![0.0, 1.0], vec![0.0]);
    }

    #[test]
    #[should_panic(expected = "polyline x and y must have the same length")]
    fn polyline_rejects_length_mismatch() {
        Shape::polyline(vec![0.0], vec![0.0, 1.0]);
    }

    #[test]
    fn builders_set_fields() {
        let s = Shape::rectangle(0.0, 0.0, 1.0, 1.0)
            .with_color(Color32::RED)
            .with_fill(true)
            .with_line_style(LineStyle::Dashed)
            .with_line_width(2.0)
            .with_gap_color(Color32::BLACK);
        assert_eq!(s.color, Color32::RED);
        assert!(s.fill);
        assert_eq!(s.line_style, LineStyle::Dashed);
        assert_eq!(s.line_width, 2.0);
        assert_eq!(s.gap_color, Some(Color32::BLACK));
    }

    #[test]
    fn overlay_defaults_true_and_builder_toggles() {
        // Default is the port's current behavior: shapes draw in the overlay pass.
        assert!(Shape::rectangle(0.0, 0.0, 1.0, 1.0).is_overlay);
        // The builder can opt out (silx setOverlay(False)).
        let s = Shape::rectangle(0.0, 0.0, 1.0, 1.0).with_overlay(false);
        assert!(!s.is_overlay);
    }

    #[test]
    fn rectangle_screen_points_are_the_four_corners() {
        // Rectangle data (2,3)-(8,7): x 2->20,8->80; y 3->70,7->30 (y flipped).
        let r = Shape::rectangle(2.0, 3.0, 8.0, 7.0);
        let pts = r.screen_points(&t());
        assert_eq!(
            pts,
            vec![
                pos2(20.0, 70.0), // (x0, y0)
                pos2(80.0, 70.0), // (x1, y0)
                pos2(80.0, 30.0), // (x1, y1)
                pos2(20.0, 30.0), // (x0, y1)
            ]
        );
    }

    #[test]
    fn polygon_screen_points_map_each_vertex_and_lines_are_empty() {
        let p = Shape::polygon(vec![1.0, 5.0], vec![2.0, 6.0]);
        assert_eq!(
            p.screen_points(&t()),
            vec![pos2(10.0, 80.0), pos2(50.0, 40.0)]
        );
        // Line kinds carry no fixed-extent vertices.
        assert!(Shape::hlines(vec![1.0]).screen_points(&t()).is_empty());
        assert!(Shape::vlines(vec![1.0]).screen_points(&t()).is_empty());
    }
}
