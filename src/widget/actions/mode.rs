//! Plot interaction-mode actions, mirroring silx `silx.gui.plot.actions.mode`.
//!
//! silx exposes `ZoomModeAction` (`mode.py:45`) and `PanModeAction`
//! (`mode.py:108`) as checkable `QAction`s that call
//! `plot.setInteractiveMode("zoom" | "pan")`. egui is immediate-mode, so each
//! action here is a plain function that performs the one state transition the
//! corresponding silx `QAction._actionTriggered` does — setting the
//! [`PlotWidget`]'s [`PlotInteractionMode`] — without the `QAction`,
//! `checkable`, or signal machinery.
//!
//! [`zoom_mode`] and [`pan_mode`] mirror silx directly. silx has no
//! `MaskModeAction` — its `MaskToolsWidget` owns its pencil draw mode rather
//! than exposing it as a plot mode action — so [`mask_draw_mode`] is a
//! port-specific mode setter for [`PlotInteractionMode::MaskDraw`], grouped here
//! with the other mode setters because it sets a plot interaction mode.
//! [`select_mode`] sets the port's [`PlotInteractionMode::Select`] (item / ROI
//! handle editing), which has no standalone silx action either.
//!
//! These are thin setters over [`PlotWidget::set_interaction_mode`]: a single
//! state transition each, named after the silx actions so they group with the
//! other `actions/*` ports. The load-bearing per-mode gating lives in
//! `apply_interaction` (tested there); the only logic here is the mode each
//! setter maps to, exercised by `mode_for_*` pure helpers below.

use crate::widget::high_level::PlotWidget;
use crate::widget::plot_widget::PlotInteractionMode;

/// The interaction mode [`zoom_mode`] sets (silx `ZoomModeAction`,
/// `mode.py:45`). Pure, so the mapping is unit-testable without a GPU backend.
pub fn mode_for_zoom() -> PlotInteractionMode {
    PlotInteractionMode::Zoom
}

/// The interaction mode [`pan_mode`] sets (silx `PanModeAction`,
/// `mode.py:108`). Pure, so the mapping is unit-testable without a GPU backend.
pub fn mode_for_pan() -> PlotInteractionMode {
    PlotInteractionMode::Pan
}

/// The interaction mode [`mask_draw_mode`] sets (port-specific pencil/mask draw
/// mode; silx's `MaskToolsWidget` owns this). Pure, so the mapping is
/// unit-testable without a GPU backend.
pub fn mode_for_mask_draw() -> PlotInteractionMode {
    PlotInteractionMode::MaskDraw
}

/// The interaction mode [`select_mode`] sets (port-specific item / ROI-handle
/// select mode). Pure, so the mapping is unit-testable without a GPU backend.
pub fn mode_for_select() -> PlotInteractionMode {
    PlotInteractionMode::Select
}

/// Put `plot` into box-zoom mode (silx `ZoomModeAction._actionTriggered` →
/// `plot.setInteractiveMode("zoom")`, `mode.py:45`).
pub fn zoom_mode(plot: &mut PlotWidget) {
    plot.set_interaction_mode(mode_for_zoom());
}

/// Put `plot` into pan mode (silx `PanModeAction._actionTriggered` →
/// `plot.setInteractiveMode("pan")`, `mode.py:108`).
pub fn pan_mode(plot: &mut PlotWidget) {
    plot.set_interaction_mode(mode_for_pan());
}

/// Put `plot` into pencil / mask-draw mode ([`PlotInteractionMode::MaskDraw`]),
/// where the primary drag is reserved for mask painting. Mirrors silx
/// `MaskToolsWidget` activating the plot's pencil draw interaction
/// (`MaskToolsWidget.py:849-876`); silx has no standalone `MaskModeAction`, so
/// this is the port's mode setter for that state.
pub fn mask_draw_mode(plot: &mut PlotWidget) {
    plot.set_interaction_mode(mode_for_mask_draw());
}

/// Put `plot` into select mode ([`PlotInteractionMode::Select`]), where primary
/// clicks select items and primary drags edit ROI handles without starting a
/// box zoom. Port-specific (no standalone silx action).
pub fn select_mode(plot: &mut PlotWidget) {
    plot.set_interaction_mode(mode_for_select());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_setter_maps_to_its_variant() {
        // Each named setter mirrors a silx mode action by mapping to exactly one
        // PlotInteractionMode. PlotWidget needs a RenderState/GPU to construct,
        // so assert the pure mode-for-* mapping the setter applies (the setter
        // body is `set_interaction_mode(mode_for_*())`).
        assert_eq!(mode_for_zoom(), PlotInteractionMode::Zoom);
        assert_eq!(mode_for_pan(), PlotInteractionMode::Pan);
        assert_eq!(mode_for_mask_draw(), PlotInteractionMode::MaskDraw);
        assert_eq!(mode_for_select(), PlotInteractionMode::Select);

        // The four setters target four distinct modes.
        let modes = [
            mode_for_zoom(),
            mode_for_pan(),
            mode_for_mask_draw(),
            mode_for_select(),
        ];
        for (i, a) in modes.iter().enumerate() {
            for b in &modes[i + 1..] {
                assert_ne!(a, b, "mode setters must map to distinct variants");
            }
        }
    }
}
