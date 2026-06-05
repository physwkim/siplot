//! Mask Tools Example.
//!
//! Demonstrates the `MaskToolsWidget` allowing the user to draw a boolean mask over a `Plot2D`.
//!
//! Run with: `cargo run --example high_level_mask_tools`

use eframe::egui;
use siplot::{Colormap, MaskTool, MaskToolsWidget, Plot2D, PlotInteractionMode};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

struct MaskToolsApp {
    image_plot: Plot2D,
    mask_tools: MaskToolsWidget,
}

impl MaskToolsApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("eframe must use the wgpu renderer");

        let pixels = build_image();

        let mut image_plot = Plot2D::new(rs, 0);
        image_plot.set_graph_title("Interactive Masking (Hover and Drag)");
        image_plot.set_graph_cursor(true);
        image_plot.set_default_colormap(Colormap::viridis(0.0, 1.0));
        image_plot
            .try_add_default_image(WIDTH, HEIGHT, &pixels)
            .expect("image dimensions match");

        let mask_tools = MaskToolsWidget::new(WIDTH, HEIGHT);

        Self {
            image_plot,
            mask_tools,
        }
    }
}

impl eframe::App for MaskToolsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.vertical(|ui| {
            // Show mask toolbar
            self.mask_tools.show_toolbar(ui);

            ui.separator();

            // Draw plot and handle interactions
            let plot_resp = self.image_plot.show_with_toolbar(ui);

            // If drawing mask, we don't want Pan/Zoom to interfere.
            // In a real app, you might want a "Draw Mask" interaction mode.
            // For now, if active tool is not None, we override interaction mode to Select
            // so panning is disabled.
            if self.mask_tools.active_tool != MaskTool::None
                && self.image_plot.interaction_mode() != PlotInteractionMode::Select
            {
                self.image_plot
                    .set_interaction_mode(PlotInteractionMode::Select);
            }

            self.mask_tools.handle_interaction(&plot_resp.plot);
            self.mask_tools.apply(&mut self.image_plot);
        });
    }
}

fn build_image() -> Vec<f32> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let cx = (col as f32 - WIDTH as f32 / 2.0) / (WIDTH as f32 / 4.0);
            let cy = (row as f32 - HEIGHT as f32 / 2.0) / (HEIGHT as f32 / 4.0);
            pixels.push((-0.5 * (cx * cx + cy * cy)).exp());
        }
    }
    pixels
}

fn main() -> eframe::Result {
    eframe::run_native(
        "siplot: Mask Tools",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(MaskToolsApp::new(cc)) as Box<dyn eframe::App>)),
    )
}
