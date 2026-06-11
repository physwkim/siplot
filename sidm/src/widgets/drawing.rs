//! `SidmDrawing` — a static shape with a fill, border, rotation, and optional
//! alarm-driven recolouring.
//!
//! Ports the core of `pydm/widgets/drawing.py` (`PyDMDrawing` + the
//! `PyDMDrawingRectangle`/`Ellipse`/`Circle`/`Triangle`/`Line` subclasses): a
//! widget that paints one shape with a brush (fill) and pen (border), rotated by
//! `rotation`, with the alarm severity optionally overriding the fill
//! (`alarmSensitiveContent`) or the border (`alarmSensitiveBorder`, default
//! *off* for drawings). PyDM reduces the drawing bounds by the pen width so the
//! border stays inside the widget (`get_bounds`); the same inset is applied here.
//!
//! The colour decision ([`effective_colors`]) and the rotation maths are pure
//! and unit-tested; the shapes are unified through one polygon path
//! (ellipse/circle sampled as a polygon — which also rotates for free), verified
//! by a headless wgpu readback.
//!
//! **Deviation:** the `PyDMDrawingPolyline`/`Polygon`/`Arc`/`Pie`/`Chord`/`Image`
//! subclasses are not ported (arcs/images need extra primitives); the common
//! filled shapes and the line are.

use siplot::egui::{self, Color32, Pos2, Stroke, Vec2};

use crate::channel::{AlarmSeverity, Channel};
use crate::engine::{Engine, EngineError};
use crate::widgets::base::{ChannelBase, severity_color};

/// The shape drawn by a [`SidmDrawing`] (PyDM `PyDMDrawing*` subclasses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrawingShape {
    /// A filled rectangle (PyDM `PyDMDrawingRectangle`).
    #[default]
    Rectangle,
    /// A filled ellipse filling the bounds (PyDM `PyDMDrawingEllipse`).
    Ellipse,
    /// A filled circle, the largest that fits the bounds (PyDM
    /// `PyDMDrawingCircle`).
    Circle,
    /// A filled apex-up triangle (PyDM `PyDMDrawingTriangle`).
    Triangle,
    /// A straight line across the bounds, drawn with the border pen (PyDM
    /// `PyDMDrawingLine`); needs a non-zero border width to be visible.
    Line,
}

/// Segments used to approximate an ellipse/circle as a polygon.
const ELLIPSE_SEGMENTS: usize = 48;
/// Default drawing size in points.
const DEFAULT_SIZE: Vec2 = Vec2::new(40.0, 40.0);

/// The effective `(fill, border)` colours after applying alarm sensitivity
/// (PyDM's stylesheet override): the fill follows the severity when
/// `sensitive_content`, the border follows it when `sensitive_border`; a
/// `NoAlarm` severity (or an insensitive flag) keeps the configured colour.
pub fn effective_colors(
    fill: Color32,
    border: Color32,
    severity: AlarmSeverity,
    sensitive_content: bool,
    sensitive_border: bool,
) -> (Color32, Color32) {
    let sev = severity_color(severity);
    let fill = if sensitive_content {
        sev.unwrap_or(fill)
    } else {
        fill
    };
    let border = if sensitive_border {
        sev.unwrap_or(border)
    } else {
        border
    };
    (fill, border)
}

/// Rotate the offset `(dx, dy)` from a centre by `angle_rad` (screen
/// convention: y points down, a positive angle rotates clockwise). Pure.
fn rotate(dx: f64, dy: f64, angle_rad: f64) -> (f64, f64) {
    let (sin, cos) = angle_rad.sin_cos();
    (dx * cos - dy * sin, dx * sin + dy * cos)
}

/// The shape's vertices (screen points), centred on `center`, fitting a `w × h`
/// box and rotated by `rotation_deg`. A `Line` returns its two endpoints.
///
/// Shared with [`crate::widgets::symbol`]: it is the one owner of the shape
/// geometry, so `SidmSymbol` fills its bounds with these points rather than
/// re-deriving the ellipse sampling.
pub(crate) fn shape_points(
    shape: DrawingShape,
    center: Pos2,
    w: f64,
    h: f64,
    rotation_deg: f64,
) -> Vec<Pos2> {
    let (hw, hh) = (w * 0.5, h * 0.5);
    let local: Vec<(f64, f64)> = match shape {
        DrawingShape::Rectangle => vec![(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)],
        DrawingShape::Triangle => vec![(0.0, -hh), (hw, hh), (-hw, hh)],
        DrawingShape::Line => vec![(-hw, 0.0), (hw, 0.0)],
        DrawingShape::Ellipse => ellipse_local(hw, hh),
        DrawingShape::Circle => {
            let r = hw.min(hh);
            ellipse_local(r, r)
        }
    };
    let angle = rotation_deg.to_radians();
    local
        .into_iter()
        .map(|(dx, dy)| {
            let (rx, ry) = rotate(dx, dy, angle);
            egui::pos2(center.x + rx as f32, center.y + ry as f32)
        })
        .collect()
}

/// Vertices of an axis-aligned ellipse with radii `(rw, rh)` about the origin.
fn ellipse_local(rw: f64, rh: f64) -> Vec<(f64, f64)> {
    (0..ELLIPSE_SEGMENTS)
        .map(|i| {
            let t = i as f64 * std::f64::consts::TAU / ELLIPSE_SEGMENTS as f64;
            (rw * t.cos(), rh * t.sin())
        })
        .collect()
}

/// A static shape driven by a channel only for its alarm/connection state (PyDM
/// `PyDMDrawing*`).
pub struct SidmDrawing {
    base: ChannelBase,
    shape: DrawingShape,
    fill: Color32,
    border_color: Color32,
    border_width: f32,
    rotation_deg: f64,
    size: Vec2,
}

impl SidmDrawing {
    /// Connect `address` and wrap it as a drawing of `shape`. The border is off
    /// by default (PyDM pen `NoPen`), as is the alarm border (PyDM
    /// `PyDMDrawing.alarmSensitiveBorder = False`).
    pub fn new(engine: &Engine, address: &str, shape: DrawingShape) -> Result<Self, EngineError> {
        let mut base = ChannelBase::new(engine.connect(address)?);
        base.alarm_sensitive_border = false;
        Ok(Self {
            base,
            shape,
            fill: Color32::BLACK,
            border_color: Color32::BLACK,
            border_width: 0.0,
            rotation_deg: 0.0,
            size: DEFAULT_SIZE,
        })
    }

    /// Set the fill (brush) colour (builder style; PyDM `brush`).
    pub fn with_fill(mut self, fill: Color32) -> Self {
        self.fill = fill;
        self
    }

    /// Set the border (pen) colour and width (builder style; PyDM `penColor` /
    /// `penWidth`). A width of 0 draws no border.
    pub fn with_border(mut self, color: Color32, width: f32) -> Self {
        self.border_color = color;
        self.border_width = width;
        self
    }

    /// Set the rotation in degrees (builder style; PyDM `rotation`).
    pub fn with_rotation(mut self, degrees: f64) -> Self {
        self.rotation_deg = degrees;
        self
    }

    /// Set the drawing size in points (builder style).
    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    /// Recolour the fill by alarm severity (builder style; PyDM
    /// `alarmSensitiveContent`).
    pub fn with_alarm_sensitive_content(mut self, on: bool) -> Self {
        self.base.alarm_sensitive_content = on;
        self
    }

    /// Recolour the border by alarm severity (builder style; PyDM
    /// `alarmSensitiveBorder`, off by default for drawings).
    pub fn with_alarm_sensitive_border(mut self, on: bool) -> Self {
        self.base.alarm_sensitive_border = on;
        self
    }

    /// The underlying channel.
    pub fn channel(&self) -> &Channel {
        self.base.channel()
    }

    /// Render the shape this frame.
    pub fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let state = self.base.channel().state();
        let (fill, border) = effective_colors(
            self.fill,
            self.border_color,
            state.effective_severity(),
            self.base.alarm_sensitive_content,
            self.base.alarm_sensitive_border,
        );

        let (rect, response) = ui.allocate_exact_size(self.size, egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint(ui.painter(), rect, fill, border);
        }
        response.on_hover_text(self.base.tooltip(&state))
    }

    /// Paint the shape into `rect`, insetting by the border width so the stroke
    /// stays inside the widget (PyDM `get_bounds`).
    fn paint(&self, painter: &egui::Painter, rect: egui::Rect, fill: Color32, border: Color32) {
        let stroke = if self.border_width > 0.0 {
            Stroke::new(self.border_width, border)
        } else {
            Stroke::NONE
        };
        let inset = 2.0 * f64::from(self.border_width);
        let w = (f64::from(rect.width()) - inset).max(0.0);
        let h = (f64::from(rect.height()) - inset).max(0.0);
        let pts = shape_points(self.shape, rect.center(), w, h, self.rotation_deg);

        if self.shape == DrawingShape::Line {
            if let [a, b] = pts[..] {
                painter.line_segment([a, b], stroke);
            }
        } else {
            painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_alarm_keeps_configured_colors() {
        let (fill, border) = effective_colors(
            Color32::BLUE,
            Color32::GREEN,
            AlarmSeverity::NoAlarm,
            true,
            true,
        );
        assert_eq!(fill, Color32::BLUE);
        assert_eq!(border, Color32::GREEN);
    }

    #[test]
    fn sensitive_content_recolors_only_the_fill() {
        let (fill, border) = effective_colors(
            Color32::BLUE,
            Color32::GREEN,
            AlarmSeverity::Major,
            true,
            false,
        );
        assert_eq!(fill, severity_color(AlarmSeverity::Major).unwrap());
        assert_eq!(border, Color32::GREEN);
    }

    #[test]
    fn sensitive_border_recolors_only_the_border() {
        let (fill, border) = effective_colors(
            Color32::BLUE,
            Color32::GREEN,
            AlarmSeverity::Minor,
            false,
            true,
        );
        assert_eq!(fill, Color32::BLUE);
        assert_eq!(border, severity_color(AlarmSeverity::Minor).unwrap());
    }

    #[test]
    fn rotate_quarter_turn() {
        let (x, y) = rotate(1.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!((x - 0.0).abs() < 1e-9, "x = {x}");
        assert!((y - 1.0).abs() < 1e-9, "y = {y}");
    }

    #[test]
    fn rectangle_has_four_corners_centered() {
        let center = egui::pos2(50.0, 50.0);
        let pts = shape_points(DrawingShape::Rectangle, center, 20.0, 10.0, 0.0);
        assert_eq!(pts.len(), 4);
        // Corners at center ± (10, 5).
        assert_eq!(pts[0], egui::pos2(40.0, 45.0));
        assert_eq!(pts[2], egui::pos2(60.0, 55.0));
    }

    #[test]
    fn circle_uses_the_smaller_half_extent() {
        // A 40×20 box → circle radius 10 (min half-extent), so points stay within
        // ±10 on both axes.
        let center = egui::pos2(0.0, 0.0);
        let pts = shape_points(DrawingShape::Circle, center, 40.0, 20.0, 0.0);
        assert_eq!(pts.len(), ELLIPSE_SEGMENTS);
        for p in pts {
            assert!(p.x.abs() <= 10.0 + 1e-3, "x out of range: {}", p.x);
            assert!(p.y.abs() <= 10.0 + 1e-3, "y out of range: {}", p.y);
        }
    }
}
