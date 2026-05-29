//! Plot chrome drawn with egui's painter: frame, grid, ticks, tick labels, and
//! a vertical colorbar.
//!
//! Everything here derives from the same [`Transform`] that feeds the wgpu data
//! layer, so the axes and the image cannot drift apart (`doc/design.md` §4·§8).
//! Layout reserves fixed-pixel gutters for the labels and (optionally) the
//! colorbar; this is the chrome counterpart of silx's `_PlotWidget` margins.

use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, Visuals, pos2};

use crate::core::colormap::Colormap;
use crate::core::transform::Transform;

/// Colors used to draw the chrome, derived from the active egui visuals so the
/// chrome follows light/dark theme.
pub struct Style {
    /// Frame, tick marks, and colorbar border.
    pub axis: Color32,
    /// Grid lines inside the data area (faint).
    pub grid: Color32,
    /// Tick label text.
    pub text: Color32,
}

impl Style {
    /// Build a chrome style from egui visuals (axis/text = text color, grid = a
    /// faint tint of it).
    pub fn from_visuals(v: &Visuals) -> Self {
        let text = v.text_color();
        Self {
            axis: text,
            grid: Color32::from_rgba_unmultiplied(text.r(), text.g(), text.b(), 28),
            text,
        }
    }
}

/// Where the data area and (optional) colorbar sit inside the widget rect.
pub struct ChromeLayout {
    /// Rect the data layer (image/curve) and axes occupy.
    pub data_area: Rect,
    /// Colorbar strip rect, or `None` when the plot has no colormap.
    pub colorbar: Option<Rect>,
}

// Fixed-pixel gutters. Left holds Y tick labels; bottom holds X tick labels;
// top/right are breathing room. With a colorbar the right gutter also holds the
// strip and its value labels.
const GUTTER_LEFT: f32 = 52.0;
const GUTTER_BOTTOM: f32 = 30.0;
const GUTTER_TOP: f32 = 12.0;
const GUTTER_RIGHT: f32 = 12.0;
const CBAR_WIDTH: f32 = 16.0;
const CBAR_LABELS: f32 = 46.0;

/// Reserve gutters for axis labels (and a colorbar, if requested) and return
/// the resulting data area and colorbar rects.
pub fn layout(full: Rect, with_colorbar: bool) -> ChromeLayout {
    let right = if with_colorbar {
        GUTTER_RIGHT + CBAR_WIDTH + CBAR_LABELS
    } else {
        GUTTER_RIGHT
    };
    let data_area = Rect::from_min_max(
        pos2(full.left() + GUTTER_LEFT, full.top() + GUTTER_TOP),
        pos2(full.right() - right, full.bottom() - GUTTER_BOTTOM),
    );
    let colorbar = with_colorbar.then(|| {
        let x0 = data_area.right() + GUTTER_RIGHT;
        Rect::from_min_max(
            pos2(x0, data_area.top()),
            pos2(x0 + CBAR_WIDTH, data_area.bottom()),
        )
    });
    ChromeLayout {
        data_area,
        colorbar,
    }
}

/// "Nice" rounding of a span to {1, 2, 5} × 10ⁿ — the classic axis-tick
/// heuristic (Heckbert, *Graphics Gems*).
fn nice_num(range: f64, round: bool) -> f64 {
    if range <= 0.0 {
        return 1.0;
    }
    let exp = range.log10().floor();
    let frac = range / 10f64.powf(exp);
    let nice = if round {
        if frac < 1.5 {
            1.0
        } else if frac < 3.0 {
            2.0
        } else if frac < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * 10f64.powf(exp)
}

/// "Nice" tick values within `[min, max]` plus the step between them.
pub fn nice_ticks(min: f64, max: f64, max_ticks: usize) -> (Vec<f64>, f64) {
    // partial_cmp (not `max > min`) so NaN limits fall through to "no ticks".
    let ascending = matches!(max.partial_cmp(&min), Some(std::cmp::Ordering::Greater));
    if !ascending || max_ticks < 2 {
        return (Vec::new(), 1.0);
    }
    let range = nice_num(max - min, false);
    let step = nice_num(range / (max_ticks - 1) as f64, true);
    let start = (min / step).floor() * step;
    let end = (max / step).ceil() * step;
    let n = ((end - start) / step).round() as i64;
    let mut ticks = Vec::new();
    for i in 0..=n {
        let v = start + i as f64 * step;
        if v >= min - step * 1e-6 && v <= max + step * 1e-6 {
            ticks.push(v);
        }
    }
    (ticks, step)
}

/// Format a tick value with enough decimals for the step size.
fn format_tick(v: f64, step: f64) -> String {
    let decimals = (-step.log10().floor()).clamp(0.0, 6.0) as usize;
    format!("{v:.decimals$}")
}

/// Draw the frame, grid, ticks, and tick labels around the data area.
pub fn draw_axes(painter: &Painter, t: &Transform, style: &Style) {
    let area = t.area;
    let axis = Stroke::new(1.0, style.axis);
    let grid = Stroke::new(1.0, style.grid);
    let font = FontId::proportional(11.0);
    let tick_len = 4.0;

    let (xticks, xstep) = nice_ticks(t.x.min, t.x.max, 8);
    let (yticks, ystep) = nice_ticks(t.y.min, t.y.max, 6);

    // Grid lines first, so the frame and ticks sit on top of them.
    for &xv in &xticks {
        let px = t.data_to_pixel(xv, t.y.min).x;
        painter.vline(px, area.y_range(), grid);
    }
    for &yv in &yticks {
        let py = t.data_to_pixel(t.x.min, yv).y;
        painter.hline(area.x_range(), py, grid);
    }

    painter.rect_stroke(
        area,
        egui::CornerRadius::ZERO,
        axis,
        egui::StrokeKind::Inside,
    );

    // X ticks + labels below the bottom edge.
    for &xv in &xticks {
        let px = t.data_to_pixel(xv, t.y.min).x;
        painter.line_segment(
            [pos2(px, area.bottom()), pos2(px, area.bottom() + tick_len)],
            axis,
        );
        painter.text(
            pos2(px, area.bottom() + tick_len + 2.0),
            Align2::CENTER_TOP,
            format_tick(xv, xstep),
            font.clone(),
            style.text,
        );
    }
    // Y ticks + labels left of the left edge.
    for &yv in &yticks {
        let py = t.data_to_pixel(t.x.min, yv).y;
        painter.line_segment(
            [pos2(area.left() - tick_len, py), pos2(area.left(), py)],
            axis,
        );
        painter.text(
            pos2(area.left() - tick_len - 3.0, py),
            Align2::RIGHT_CENTER,
            format_tick(yv, ystep),
            font.clone(),
            style.text,
        );
    }
}

/// Draw a vertical colorbar matching `cmap` (top = vmax, bottom = vmin), with a
/// border and value labels on its right edge.
pub fn draw_colorbar(painter: &Painter, rect: Rect, cmap: &Colormap, style: &Style) {
    // Fill with horizontal strips top→bottom; the +0.5 height overlap avoids
    // hairline gaps from rounding strip boundaries to pixels.
    let n = 64usize;
    let strip_h = rect.height() / n as f32;
    for i in 0..n {
        // i = 0 at the top maps to LUT 255 (vmax); i = n-1 to LUT 0 (vmin).
        let lut_idx = (255 * (n - 1 - i) / (n - 1)).min(255);
        let c = cmap.lut[lut_idx];
        let y0 = rect.top() + i as f32 * strip_h;
        let strip = Rect::from_min_max(
            pos2(rect.left(), y0),
            pos2(rect.right(), y0 + strip_h + 0.5),
        );
        painter.rect_filled(
            strip,
            egui::CornerRadius::ZERO,
            Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    painter.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(1.0, style.axis),
        egui::StrokeKind::Inside,
    );

    let (ticks, step) = nice_ticks(cmap.vmin, cmap.vmax, 6);
    let font = FontId::proportional(11.0);
    let axis = Stroke::new(1.0, style.axis);
    let span = cmap.vmax - cmap.vmin;
    if span <= 0.0 {
        return;
    }
    for v in ticks {
        let frac = ((v - cmap.vmin) / span) as f32; // 0 at vmin, 1 at vmax
        let py = rect.bottom() - frac * rect.height(); // vmin at bottom
        painter.line_segment([pos2(rect.right(), py), pos2(rect.right() + 3.0, py)], axis);
        painter.text(
            pos2(rect.right() + 5.0, py),
            Align2::LEFT_CENTER,
            format_tick(v, step),
            font.clone(),
            style.text,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks_lie_within_range_and_are_evenly_spaced() {
        let (ticks, step) = nice_ticks(0.0, 256.0, 8);
        assert!(!ticks.is_empty());
        for &t in &ticks {
            assert!((-1e-6..=256.0 + 1e-6).contains(&t), "{t} out of range");
        }
        for w in ticks.windows(2) {
            assert!((w[1] - w[0] - step).abs() <= step * 1e-6, "uneven spacing");
        }
    }

    #[test]
    fn degenerate_or_inverted_range_yields_no_ticks() {
        assert!(nice_ticks(5.0, 5.0, 8).0.is_empty());
        assert!(nice_ticks(5.0, 1.0, 8).0.is_empty());
    }

    #[test]
    fn format_tick_uses_step_appropriate_decimals() {
        assert_eq!(format_tick(2.0, 1.0), "2");
        assert_eq!(format_tick(0.5, 0.5), "0.5");
        assert_eq!(format_tick(0.25, 0.05), "0.25");
    }

    #[test]
    fn layout_reserves_right_gutter_only_with_colorbar() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 300.0));
        let no_bar = layout(full, false);
        assert!(no_bar.colorbar.is_none());
        let with_bar = layout(full, true);
        let bar = with_bar.colorbar.expect("colorbar rect");
        // The colorbar sits to the right of the (narrower) data area.
        assert!(bar.left() >= with_bar.data_area.right());
        assert!(with_bar.data_area.right() < no_bar.data_area.right());
    }
}
