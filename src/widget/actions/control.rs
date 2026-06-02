//! Plot control actions, mirroring silx `silx.gui.plot.actions.control`.
//!
//! Functions here mutate a [`PlotWidget`] (or its underlying [`Plot`]) in
//! place, performing the same state transition the corresponding silx
//! `QAction._actionTriggered` does, without the `QAction` machinery.
//!
//! [`Plot`]: crate::core::plot::Plot

use crate::core::items::LineStyle;
use crate::widget::high_level::{ImageView, PlotWidget, ScatterView};

/// The line-style cycle used by [`curve_style_cycle`], in order. A drawn curve
/// steps Solid → Dashed → DashDot → Dotted → (wrap to Solid).
///
/// silx `CurveStyleAction` instead cycles the *default* `(plot lines, plot
/// points)` booleans through `(line) → (line+symbol) → (symbol) → (line)`,
/// applying to every curve. This port has no plot-wide default-style toggle, so
/// it cycles the active curve's concrete [`LineStyle`] through the drawn
/// patterns; [`LineStyle::None`] and [`LineStyle::Custom`] are not part of the
/// cycle (a curve in either maps to `Solid` on the next step via
/// [`next_line_style`]).
const LINE_STYLE_CYCLE: [LineStyle; 4] = [
    LineStyle::Solid,
    LineStyle::Dashed,
    LineStyle::DashDot,
    LineStyle::Dotted,
];

/// The next line style after `current` in [`LINE_STYLE_CYCLE`]. A style not in
/// the cycle (`None`, `Custom`) steps to the first entry (`Solid`). Pure and
/// deterministic so the cycle is unit-testable without a GPU backend.
pub fn next_line_style(current: &LineStyle) -> LineStyle {
    match LINE_STYLE_CYCLE.iter().position(|s| s == current) {
        Some(idx) => LINE_STYLE_CYCLE[(idx + 1) % LINE_STYLE_CYCLE.len()].clone(),
        None => LINE_STYLE_CYCLE[0].clone(),
    }
}

/// Toggle whether the plot axes (frame/ticks/labels) are displayed, mirroring
/// silx `ShowAxisAction` (`actions/control.py`): its `_actionTriggered(checked)`
/// calls `plot.setAxesDisplayed(checked)`, flipping the current state.
///
/// Returns the new `axes_displayed` value after the toggle.
pub fn show_axis_toggle(plot: &mut PlotWidget) -> bool {
    let next = !plot.plot().axes_displayed();
    plot.plot_mut().set_axes_displayed(next);
    next
}

/// Cycle the active curve's line style to the next style in
/// [`LINE_STYLE_CYCLE`], mirroring silx `CurveStyleAction` (which cycles the
/// plot-wide default line/points state). Returns the new [`LineStyle`], or
/// `None` when there is no active curve with a retained style.
pub fn curve_style_cycle(plot: &mut PlotWidget) -> Option<LineStyle> {
    plot.cycle_active_curve_style()
}

/// Toggle the [`ImageView`] side colorbar's visibility, mirroring silx
/// `ColorBarAction`: its `_actionTriggered(checked)` sets the `ColorBarWidget`'s
/// visibility. Returns the new `show_colorbar` value.
pub fn image_colorbar_toggle(view: &mut ImageView) -> bool {
    let next = !view.show_colorbar();
    view.set_show_colorbar(next);
    next
}

/// Toggle the [`ScatterView`] side colorbar's visibility, mirroring silx
/// `ColorBarAction`. Returns the new `show_colorbar` value.
pub fn scatter_colorbar_toggle(view: &mut ScatterView) -> bool {
    let next = !view.show_colorbar();
    view.set_show_colorbar(next);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_line_style_cycles_deterministically_and_wraps() {
        assert_eq!(next_line_style(&LineStyle::Solid), LineStyle::Dashed);
        assert_eq!(next_line_style(&LineStyle::Dashed), LineStyle::DashDot);
        assert_eq!(next_line_style(&LineStyle::DashDot), LineStyle::Dotted);
        // Wraps back to the first entry.
        assert_eq!(next_line_style(&LineStyle::Dotted), LineStyle::Solid);
        // Styles outside the cycle step to the first entry.
        assert_eq!(next_line_style(&LineStyle::None), LineStyle::Solid);
        assert_eq!(
            next_line_style(&LineStyle::Custom {
                offset: 0.0,
                pattern: vec![1.0, 2.0],
            }),
            LineStyle::Solid
        );
    }

    #[test]
    fn cycling_changes_stored_line_style_on_curve_data() {
        // Mirror cycle_active_curve_style's body on a bare CurveData (no GPU
        // backend): clone the retained curve, advance its stored line style.
        use crate::render::gpu_curve::CurveData;
        use egui::Color32;

        let mut data = CurveData::new(vec![0.0, 1.0], vec![0.0, 1.0], Color32::WHITE)
            .with_line_style(LineStyle::Solid);
        assert_eq!(data.line_style, LineStyle::Solid);

        data.line_style = next_line_style(&data.line_style);
        assert_eq!(data.line_style, LineStyle::Dashed, "first cycle");

        data.line_style = next_line_style(&data.line_style);
        assert_eq!(data.line_style, LineStyle::DashDot, "second cycle");
    }

    #[test]
    fn show_axis_toggle_flips_axes_displayed() {
        // A bare Plot model exercises the same set_axes_displayed transition the
        // action drives through PlotWidget, without a GPU backend.
        use crate::core::plot::Plot;

        let mut plot = Plot::new(0);
        assert!(plot.axes_displayed(), "default is displayed");

        // Mirror show_axis_toggle's body on the bare model.
        let next = !plot.axes_displayed();
        plot.set_axes_displayed(next);
        assert!(!plot.axes_displayed(), "first toggle hides");

        let next = !plot.axes_displayed();
        plot.set_axes_displayed(next);
        assert!(plot.axes_displayed(), "second toggle shows again");
    }
}
