//! Markers: point, vertical-line, and horizontal-line annotations drawn over the
//! data area (silx `BackendBase.addMarker`).
//!
//! A marker is a lightweight overlay like an [`crate::core::roi::Roi`]: its
//! geometry is data-space and the screen-placement math is pure (no egui input),
//! so it is unit-testable; the widget's chrome draws the list each frame via
//! [`crate::widget::chrome::draw_markers`]. silx selects the marker kind by which
//! coordinate is `None` — `x` `None` ⇒ horizontal line, `y` `None` ⇒ vertical
//! line, both set ⇒ a point marker drawn with a symbol (`doc/design.md` §8).

use egui::{Color32, Pos2};

use crate::core::items::LineStyle;
use crate::core::transform::{Transform, YAxis};

/// Default point-marker symbol size (full extent) in logical points.
pub const DEFAULT_MARKER_SIZE: f32 = 8.0;

/// Symbol drawn at a point marker (silx `addMarker` `symbol`). The catalog
/// matches silx's marker symbols, which differ from the GPU curve's scatter
/// symbols ([`crate::Symbol`]) — markers add the diamond, point, and pixel
/// glyphs and are drawn with egui's painter rather than an SDF shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerSymbol {
    /// Filled circle. silx `'o'`.
    Circle,
    /// Small filled dot. silx `'.'`.
    Point,
    /// A single pixel. silx `','`.
    Pixel,
    /// Upright "+". silx `'+'`.
    Plus,
    /// Diagonal "×". silx `'x'`.
    Cross,
    /// Filled diamond. silx `'d'`.
    Diamond,
    /// Filled square. silx `'s'`.
    Square,
}

/// What a marker is and where it sits. The illegal combinations (a line with a
/// symbol, a point without one) are unrepresentable: only [`MarkerKind::Point`]
/// carries a symbol/size, only the lines span an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkerKind {
    /// A point marker at data `(x, y)`, drawn with `symbol` at `size` (logical
    /// points). silx: both `x` and `y` set.
    Point {
        x: f64,
        y: f64,
        symbol: MarkerSymbol,
        size: f32,
    },
    /// A vertical line at data `x`, spanning the data-area height. silx: `y` `None`.
    VLine { x: f64 },
    /// A horizontal line at data `y`, spanning the data-area width. silx: `x` `None`.
    HLine { y: f64 },
}

/// A point / vertical-line / horizontal-line marker drawn over the data area
/// (silx `BackendBase.addMarker`).
///
/// `line_style`/`line_width` apply to the line kinds only (silx: "Only relevant
/// for line markers where X or Y is None"); a point ignores them. The silx
/// `constraint` (drag filter) and `font` (a `QFont`) parameters are interaction /
/// Qt-specific and not part of the marker's drawn geometry, so they are not
/// modeled here.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// What the marker is and where it sits.
    pub kind: MarkerKind,
    /// Line / symbol color (silx `color`).
    pub color: Color32,
    /// Optional label text drawn beside the marker (silx `text`).
    pub text: Option<String>,
    /// Background fill behind the label text (silx `bgcolor`); `None` draws no box.
    pub bgcolor: Option<Color32>,
    /// Stroke style for the line kinds (silx `linestyle`); ignored for points.
    pub line_style: LineStyle,
    /// Stroke width in logical points for the line kinds (silx `linewidth`);
    /// ignored for points.
    pub line_width: f32,
    /// Which Y axis the marker's data Y is measured against (silx `yaxis`).
    pub y_axis: YAxis,
}

impl Marker {
    /// A point marker at data `(x, y)`: a filled circle at the default size, white,
    /// no text, bound to the left axis.
    pub fn point(x: f64, y: f64) -> Self {
        Self::with_kind(MarkerKind::Point {
            x,
            y,
            symbol: MarkerSymbol::Circle,
            size: DEFAULT_MARKER_SIZE,
        })
    }

    /// A vertical-line marker at data `x` (silx `y` `None`).
    pub fn vline(x: f64) -> Self {
        Self::with_kind(MarkerKind::VLine { x })
    }

    /// A horizontal-line marker at data `y` (silx `x` `None`).
    pub fn hline(y: f64) -> Self {
        Self::with_kind(MarkerKind::HLine { y })
    }

    fn with_kind(kind: MarkerKind) -> Self {
        Self {
            kind,
            color: Color32::WHITE,
            text: None,
            bgcolor: None,
            line_style: LineStyle::Solid,
            line_width: 1.0,
            y_axis: YAxis::Left,
        }
    }

    /// Set the marker color.
    pub fn with_color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    /// Attach label text.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set a background color for the label text.
    pub fn with_bgcolor(mut self, color: Color32) -> Self {
        self.bgcolor = Some(color);
        self
    }

    /// Set the symbol of a point marker. No-op on the line kinds, which have no
    /// symbol.
    pub fn with_symbol(mut self, symbol: MarkerSymbol) -> Self {
        if let MarkerKind::Point { symbol: s, .. } = &mut self.kind {
            *s = symbol;
        }
        self
    }

    /// Set the symbol size (logical points) of a point marker. No-op on the line
    /// kinds.
    pub fn with_symbol_size(mut self, size: f32) -> Self {
        if let MarkerKind::Point { size: s, .. } = &mut self.kind {
            *s = size;
        }
        self
    }

    /// Set the line style of a line marker (silx `linestyle`).
    pub fn with_line_style(mut self, style: LineStyle) -> Self {
        self.line_style = style;
        self
    }

    /// Set the line width of a line marker (silx `linewidth`).
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.line_width = width;
        self
    }

    /// Bind the marker's data Y to a Y axis (silx `yaxis`).
    pub fn with_y_axis(mut self, axis: YAxis) -> Self {
        self.y_axis = axis;
        self
    }

    /// Screen position of a point marker, or `None` for a line marker.
    pub fn screen_point(&self, t: &Transform) -> Option<Pos2> {
        match self.kind {
            MarkerKind::Point { x, y, .. } => Some(t.data_to_pixel(x, y)),
            _ => None,
        }
    }

    /// Screen x of a vertical-line marker, or `None` otherwise. Independent of y,
    /// so it is evaluated at the axis minimum.
    pub fn screen_x(&self, t: &Transform) -> Option<f32> {
        match self.kind {
            MarkerKind::VLine { x } => Some(t.data_to_pixel(x, t.y.min).x),
            _ => None,
        }
    }

    /// Screen y of a horizontal-line marker, or `None` otherwise. Independent of
    /// x, so it is evaluated at the axis minimum.
    pub fn screen_y(&self, t: &Transform) -> Option<f32> {
        match self.kind {
            MarkerKind::HLine { y } => Some(t.data_to_pixel(t.x.min, y).y),
            _ => None,
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
    fn point_defaults_are_circle_white_left_axis() {
        let m = Marker::point(1.0, 2.0);
        assert_eq!(
            m.kind,
            MarkerKind::Point {
                x: 1.0,
                y: 2.0,
                symbol: MarkerSymbol::Circle,
                size: DEFAULT_MARKER_SIZE,
            }
        );
        assert_eq!(m.color, Color32::WHITE);
        assert_eq!(m.y_axis, YAxis::Left);
        assert_eq!(m.line_style, LineStyle::Solid);
        assert!(m.text.is_none() && m.bgcolor.is_none());
    }

    #[test]
    fn builders_set_fields() {
        let m = Marker::point(0.0, 0.0)
            .with_color(Color32::RED)
            .with_text("peak")
            .with_bgcolor(Color32::BLACK)
            .with_symbol(MarkerSymbol::Diamond)
            .with_symbol_size(12.0)
            .with_y_axis(YAxis::Right);
        assert_eq!(m.color, Color32::RED);
        assert_eq!(m.text.as_deref(), Some("peak"));
        assert_eq!(m.bgcolor, Some(Color32::BLACK));
        assert_eq!(m.y_axis, YAxis::Right);
        assert_eq!(
            m.kind,
            MarkerKind::Point {
                x: 0.0,
                y: 0.0,
                symbol: MarkerSymbol::Diamond,
                size: 12.0,
            }
        );
    }

    #[test]
    fn symbol_builders_are_noops_on_line_markers() {
        // A vertical line has no symbol/size; the builders leave the kind intact.
        let v = Marker::vline(3.0)
            .with_symbol(MarkerSymbol::Square)
            .with_symbol_size(20.0);
        assert_eq!(v.kind, MarkerKind::VLine { x: 3.0 });
        // Line style/width builders apply.
        let v = v.with_line_style(LineStyle::Dashed).with_line_width(2.0);
        assert_eq!(v.line_style, LineStyle::Dashed);
        assert_eq!(v.line_width, 2.0);
    }

    #[test]
    fn screen_helpers_select_by_kind() {
        // Point at data (2, 3): x 2->20px, y 3 (bottom-ish) -> 70px (y flipped).
        let p = Marker::point(2.0, 3.0);
        let pos = p.screen_point(&t()).expect("point pos");
        assert!((pos.x - 20.0).abs() < 1e-3 && (pos.y - 70.0).abs() < 1e-3);
        assert!(p.screen_x(&t()).is_none() && p.screen_y(&t()).is_none());

        // Vertical line at x=4 -> 40px; no point/y.
        let v = Marker::vline(4.0);
        assert!((v.screen_x(&t()).expect("vline x") - 40.0).abs() < 1e-3);
        assert!(v.screen_point(&t()).is_none() && v.screen_y(&t()).is_none());

        // Horizontal line at y=8 -> screen 20px (y flipped); no point/x.
        let h = Marker::hline(8.0);
        assert!((h.screen_y(&t()).expect("hline y") - 20.0).abs() < 1e-3);
        assert!(h.screen_point(&t()).is_none() && h.screen_x(&t()).is_none());
    }
}
