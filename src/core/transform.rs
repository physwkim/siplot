//! The data↔screen coordinate transform — the single source of truth.
//!
//! Both consumers derive their mapping from one [`Transform`] built from the
//! same (limits, area): the wgpu shader gets an orthographic data→NDC matrix
//! ([`Transform::ortho_matrix`]), and the egui chrome gets a data→pixel
//! [`RectTransform`] plus [`Transform::data_to_pixel`] / [`Transform::pixel_to_data`]
//! (`doc/design.md` §4). Computing the mapping in two places is what makes the
//! image and the axes drift apart by a pixel, so it lives here once.
//!
//! Scope: linear, single Y axis, non-inverted. Log/inverted/skew axes arrive in
//! later steps (`doc/design.md` §11) as flags on this type.

use egui::{Pos2, Rect, emath::RectTransform, pos2};

/// Plot margins as fractions of the full widget rect, matching silx
/// `setAxesMargins`. Insetting the full widget rect by these yields the data
/// area that a [`Transform`] maps into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Margins {
    /// No margins: the data area is the whole widget rect.
    pub const ZERO: Margins = Margins {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// Inset `full` by these fractions to get the data area.
    pub fn data_area(&self, full: Rect) -> Rect {
        let w = full.width();
        let h = full.height();
        Rect::from_min_max(
            pos2(full.left() + self.left * w, full.top() + self.top * h),
            pos2(
                full.right() - self.right * w,
                full.bottom() - self.bottom * h,
            ),
        )
    }
}

/// Linear mapping between data space and the data area's screen pixels.
///
/// Preconditions: `x_max > x_min` and `y_max > y_min` (non-degenerate limits;
/// the widget is responsible for enforcing a minimum span).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    /// Data-area rectangle in egui points (screen space).
    pub area: Rect,
}

impl Transform {
    /// Build a transform mapping the given data limits into `area`.
    pub fn new(x_min: f64, x_max: f64, y_min: f64, y_max: f64, area: Rect) -> Self {
        Self {
            x_min,
            x_max,
            y_min,
            y_max,
            area,
        }
    }

    /// Map data coordinates to screen pixels (egui points). Y is flipped:
    /// data `y_max` is at the top of the area, `y_min` at the bottom.
    pub fn data_to_pixel(&self, x: f64, y: f64) -> Pos2 {
        let (px, py) = self.data_to_pixel_f64(x, y);
        pos2(px as f32, py as f32)
    }

    /// Map screen pixels (egui points) back to data coordinates.
    pub fn pixel_to_data(&self, p: Pos2) -> (f64, f64) {
        self.pixel_to_data_f64(p.x as f64, p.y as f64)
    }

    fn data_to_pixel_f64(&self, x: f64, y: f64) -> (f64, f64) {
        let (left, right) = (self.area.left() as f64, self.area.right() as f64);
        let (top, bottom) = (self.area.top() as f64, self.area.bottom() as f64);
        let px = left + (x - self.x_min) / (self.x_max - self.x_min) * (right - left);
        // y flip: y_max -> top, y_min -> bottom
        let py = top + (self.y_max - y) / (self.y_max - self.y_min) * (bottom - top);
        (px, py)
    }

    fn pixel_to_data_f64(&self, px: f64, py: f64) -> (f64, f64) {
        let (left, right) = (self.area.left() as f64, self.area.right() as f64);
        let (top, bottom) = (self.area.top() as f64, self.area.bottom() as f64);
        let x = self.x_min + (px - left) / (right - left) * (self.x_max - self.x_min);
        let y = self.y_max - (py - top) / (bottom - top) * (self.y_max - self.y_min);
        (x, y)
    }

    /// data→pixel [`RectTransform`] for drawing chrome with egui's painter. The
    /// `from` rect carries the Y flip (its `min.y` is `y_max`).
    pub fn rect_transform(&self) -> RectTransform {
        let from = Rect::from_min_max(
            pos2(self.x_min as f32, self.y_max as f32),
            pos2(self.x_max as f32, self.y_min as f32),
        );
        RectTransform::from_to(from, self.area)
    }

    /// Column-major data→NDC orthographic matrix for the wgpu shader. Maps
    /// `[x_min, x_max] × [y_min, y_max]` to NDC `[-1, 1]²` at `z = 0`, with
    /// `y_max → +1` (top) to match egui-wgpu's viewport. Equivalent to pygfx's
    /// `OrthographicCamera::show_rect` (`doc/design.md` §4).
    pub fn ortho_matrix(&self) -> [[f32; 4]; 4] {
        let sx = (2.0 / (self.x_max - self.x_min)) as f32;
        let sy = (2.0 / (self.y_max - self.y_min)) as f32;
        let tx = (-(self.x_max + self.x_min) / (self.x_max - self.x_min)) as f32;
        let ty = (-(self.y_max + self.y_min) / (self.y_max - self.y_min)) as f32;
        [
            [sx, 0.0, 0.0, 0.0],
            [0.0, sy, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [tx, ty, 0.0, 1.0],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A deliberately asymmetric, off-origin setup to catch sign/offset bugs.
    fn sample() -> Transform {
        let area = Rect::from_min_max(pos2(100.0, 20.0), pos2(420.0, 260.0));
        Transform::new(-3.0, 5.0, 10.0, 50.0, area)
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn corners_map_with_y_flip() {
        let t = sample();
        // (x_min, y_min) -> (left, bottom)
        let (px, py) = t.data_to_pixel_f64(-3.0, 10.0);
        assert!(
            close(px, 100.0, 1e-9) && close(py, 260.0, 1e-9),
            "{px},{py}"
        );
        // (x_max, y_max) -> (right, top)
        let (px, py) = t.data_to_pixel_f64(5.0, 50.0);
        assert!(close(px, 420.0, 1e-9) && close(py, 20.0, 1e-9), "{px},{py}");
        // center -> area center
        let (px, py) = t.data_to_pixel_f64(1.0, 30.0);
        assert!(
            close(px, 260.0, 1e-9) && close(py, 140.0, 1e-9),
            "{px},{py}"
        );
    }

    #[test]
    fn round_trip_is_identity_f64() {
        let t = sample();
        for &(x, y) in &[
            (-3.0, 10.0),
            (5.0, 50.0),
            (1.0, 30.0),
            (-1.25, 42.0),
            (4.9, 11.1),
        ] {
            let (px, py) = t.data_to_pixel_f64(x, y);
            let (rx, ry) = t.pixel_to_data_f64(px, py);
            assert!(
                close(rx, x, 1e-9) && close(ry, y, 1e-9),
                "{x},{y} -> {rx},{ry}"
            );
        }
    }

    #[test]
    fn round_trip_through_pos2_is_close() {
        // f32 Pos2 loses precision; tolerance is small relative to the data span.
        let t = sample();
        for &(x, y) in &[(-1.25, 42.0), (4.9, 11.1), (0.0, 25.0)] {
            let (rx, ry) = t.pixel_to_data(t.data_to_pixel(x, y));
            assert!(
                close(rx, x, 1e-3) && close(ry, y, 1e-3),
                "{x},{y} -> {rx},{ry}"
            );
        }
    }

    #[test]
    fn rect_transform_agrees_with_data_to_pixel() {
        let t = sample();
        let rt = t.rect_transform();
        for &(x, y) in &[(-3.0, 10.0), (5.0, 50.0), (1.0, 30.0)] {
            let a = t.data_to_pixel(x, y);
            let b = rt.transform_pos(pos2(x as f32, y as f32));
            assert!(
                (a.x - b.x).abs() <= 1e-3 && (a.y - b.y).abs() <= 1e-3,
                "{a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn ortho_matrix_maps_limits_to_ndc() {
        let t = sample();
        let m = t.ortho_matrix();
        // Apply column-major matrix to (x, y, 0, 1): clip = m * v.
        let apply = |x: f32, y: f32| -> (f32, f32) {
            let cx = m[0][0] * x + m[1][0] * y + m[3][0];
            let cy = m[0][1] * x + m[1][1] * y + m[3][1];
            (cx, cy)
        };
        let approx = |a: f32, b: f32| (a - b).abs() <= 1e-5;
        let (cx, cy) = apply(-3.0, 10.0);
        assert!(approx(cx, -1.0) && approx(cy, -1.0), "{cx},{cy}");
        let (cx, cy) = apply(5.0, 50.0);
        assert!(approx(cx, 1.0) && approx(cy, 1.0), "{cx},{cy}");
        let (cx, cy) = apply(1.0, 30.0);
        assert!(approx(cx, 0.0) && approx(cy, 0.0), "{cx},{cy}");
    }

    #[test]
    fn margins_inset_full_rect() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let m = Margins {
            left: 0.1,
            top: 0.2,
            right: 0.05,
            bottom: 0.0,
        };
        let area = m.data_area(full);
        assert_eq!(area.left(), 20.0);
        assert_eq!(area.top(), 20.0);
        assert_eq!(area.right(), 190.0);
        assert_eq!(area.bottom(), 100.0);
    }
}
