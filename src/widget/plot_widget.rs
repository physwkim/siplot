//! The plot widget.
//!
//! Lays out the chrome gutters, clears the data area to the background, draws
//! the image and curve on top via wgpu paint callbacks, then draws the axes and
//! (optional) colorbar with egui's painter. The wgpu layer and the chrome share
//! a single [`crate::core::transform::Transform`] derived from the plot's limits
//! so they stay aligned (`doc/design.md` §4·§8). Interaction lands in a later
//! step (`doc/design.md` §11).

use egui::{Sense, Ui};

use crate::core::plot::Plot;
use crate::render::backend_wgpu::{ClearCallback, CurveCallback, ImageCallback};
use crate::widget::chrome;

/// Widget that renders a [`Plot`] into an egui `Ui`.
#[derive(Default)]
pub struct PlotWidget;

impl PlotWidget {
    /// Create a new plot widget.
    pub fn new() -> Self {
        Self
    }

    /// Render the plot, filling the available space.
    pub fn show(self, ui: &mut Ui, plot: &mut Plot) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());

        // Reserve chrome gutters (and a colorbar slot if the plot has a
        // colormap), then inset by the plot margins to get the data area.
        let with_colorbar = plot.colormap.is_some();
        let chrome_layout = chrome::layout(rect, with_colorbar);
        let area = plot.margins.data_area(chrome_layout.data_area);
        let transform = plot.transform(area);
        let ortho = transform.ortho_matrix();

        // Convert sRGB Color32 to linear, premultiplied RGBA expected by the shader.
        let bg = egui::Rgba::from(plot.data_background).to_array();
        let style = chrome::Style::from_visuals(ui.visuals());

        let painter = ui.painter();
        // Data layer (wgpu), clipped to the data area: clear, image, then curve.
        painter.add(egui_wgpu::Callback::new_paint_callback(
            area,
            ClearCallback { color: bg },
        ));
        painter.add(egui_wgpu::Callback::new_paint_callback(
            area,
            ImageCallback { ortho },
        ));
        painter.add(egui_wgpu::Callback::new_paint_callback(
            area,
            CurveCallback { ortho },
        ));

        // Chrome (egui), drawn on top of / in the gutters around the data layer.
        chrome::draw_axes(painter, &transform, &style);
        if let (Some(cbar), Some(cmap)) = (chrome_layout.colorbar, plot.colormap.as_ref()) {
            chrome::draw_colorbar(painter, cbar, cmap, &style);
        }

        response
    }
}
