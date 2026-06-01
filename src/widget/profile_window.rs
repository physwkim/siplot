use egui::Color32;
use egui_wgpu::RenderState;

use crate::core::backend::ItemHandle;
use crate::core::plot::PlotId;
use crate::core::roi::Roi;
use crate::render::gpu_curve::CurveData;
use crate::widget::high_level::{Plot1D, line_profile_values, rect_profile_values};

/// A window widget to display the 1D profile of an image based on an ROI.
pub struct ProfileWindow {
    plot: Plot1D,
    curve_handle: Option<ItemHandle>,
    window_id: egui::Id,
    open: bool,
}

impl ProfileWindow {
    /// Create a new ProfileWindow with a backing Plot1D.
    pub fn new(render_state: &RenderState, plot_id: PlotId) -> Self {
        let mut plot = Plot1D::new(render_state, plot_id);
        plot.set_graph_title("Profile");

        Self {
            plot,
            curve_handle: None,
            window_id: egui::Id::new(plot_id).with("profile_window"),
            open: false,
        }
    }

    /// Is the window currently open?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open or close the window.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Re-calculate and update the profile curve based on the given ROI.
    pub fn update_profile(&mut self, width: u32, height: u32, data: &[f32], roi: &Roi) {
        let profile = match roi {
            Roi::Line { start, end } => line_profile_values(width, height, data, *start, *end).ok(),
            Roi::Rect { x, y } => {
                // By default, average along the columns (vertical axis) for a row profile.
                rect_profile_values(width, height, data, (x.0, x.1, y.0, y.1), true).ok()
            }
            Roi::HRange { y } => {
                let row = ((y.0 + y.1) / 2.0).round() as u32;
                crate::widget::high_level::horizontal_profile_values(width, height, data, row)
                    .ok()
                    .map(|y_vals| {
                        let x_vals: Vec<f64> = (0..width as usize).map(|i| i as f64).collect();
                        (x_vals, y_vals)
                    })
            }
            Roi::VRange { x } => {
                let col = ((x.0 + x.1) / 2.0).round() as u32;
                crate::widget::high_level::vertical_profile_values(width, height, data, col)
                    .ok()
                    .map(|y_vals| {
                        let x_vals: Vec<f64> = (0..height as usize).map(|i| i as f64).collect();
                        (x_vals, y_vals)
                    })
            }
            _ => None,
        };

        if let Some((x, y)) = profile {
            if let Some(handle) = self.curve_handle {
                let curve = CurveData::new(x, y, Color32::YELLOW);
                self.plot.update_curve_data(handle, &curve);
            } else {
                self.curve_handle =
                    Some(
                        self.plot
                            .add_curve_with_legend(&x, &y, Color32::YELLOW, "profile"),
                    );
            }
            // Auto-scale limits based on data.
            self.plot.reset_zoom_to_data();
        }
    }

    /// Show the profile window using the given egui context.
    pub fn show(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        egui::Window::new("Profile Window")
            .id(self.window_id)
            .open(&mut open)
            .default_size([400.0, 300.0])
            .show(ctx, |ui| {
                self.plot.show(ui);
            });
        self.open = open;
    }
}
