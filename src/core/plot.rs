//! The plot model.
//!
//! Holds the identifier, data-area background, data limits, margins, and the
//! optional colormap used to draw the colorbar. The item list, log/inverted
//! axis flags, and dirty tracking are added in later steps
//! (`doc/design.md` §1·§4·§11).

use egui::{Color32, Rect};

use crate::core::colormap::Colormap;
use crate::core::transform::{Margins, Transform};

/// Identifier for a single `Plot` instance.
///
/// `egui_wgpu`'s `callback_resources` is a global type map, so multi-plot keeps
/// per-plot GPU state separated by `PlotId` (`doc/design.md` §3.1·§12). The
/// current steps handle a single plot, so no separation map exists yet.
pub type PlotId = u64;

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
        }
    }

    /// Build the data↔screen transform for the given data-area rect.
    pub fn transform(&self, area: Rect) -> Transform {
        let (x_min, x_max, y_min, y_max) = self.limits;
        Transform::new(x_min, x_max, y_min, y_max, area)
    }
}
