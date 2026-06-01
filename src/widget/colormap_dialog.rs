use crate::core::colormap::{Colormap, ColormapName, Normalization};
use crate::widget::high_level::Plot2D;

/// A widget for interactively configuring the colormap of a Plot2D.
pub struct ColormapDialog {
    pub name: ColormapName,
    pub normalization: Normalization,
    pub vmin: f64,
    pub vmax: f64,
    pub autoscale: bool,

    // Gamma for Gamma normalization
    pub gamma: f32,

    window_id: egui::Id,
    pub open: bool,
}

impl Default for ColormapDialog {
    fn default() -> Self {
        Self {
            name: ColormapName::Viridis,
            normalization: Normalization::Linear,
            vmin: 0.0,
            vmax: 1.0,
            autoscale: true,
            gamma: 2.0,
            window_id: egui::Id::new("colormap_dialog"),
            open: false,
        }
    }
}

impl ColormapDialog {
    /// Create a new ColormapDialog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize the dialog from an existing Colormap.
    pub fn with_colormap(mut self, cmap: &Colormap) -> Self {
        self.vmin = cmap.vmin;
        self.vmax = cmap.vmax;
        self.normalization = cmap.normalization;
        self.gamma = cmap.gamma;
        self
    }

    /// Show the Colormap dialog. If it's open and modified, updates the plot in real-time.
    pub fn show(&mut self, ctx: &egui::Context, plot: &mut Plot2D) {
        let mut open = self.open;
        let mut changed = false;

        egui::Window::new("Colormap")
            .id(self.window_id)
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let prev_name = self.name;
                    egui::ComboBox::from_id_salt("cmap_name")
                        .selected_text(self.name.label())
                        .show_ui(ui, |ui| {
                            for &name in &ColormapName::ALL {
                                ui.selectable_value(&mut self.name, name, name.label());
                            }
                        });
                    if self.name != prev_name {
                        changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Normalization:");
                    let prev_norm = self.normalization;
                    egui::ComboBox::from_id_salt("cmap_norm")
                        .selected_text(format!("{:?}", self.normalization))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.normalization,
                                Normalization::Linear,
                                "Linear",
                            );
                            ui.selectable_value(&mut self.normalization, Normalization::Log, "Log");
                            ui.selectable_value(
                                &mut self.normalization,
                                Normalization::Sqrt,
                                "Sqrt",
                            );
                            ui.selectable_value(
                                &mut self.normalization,
                                Normalization::Gamma,
                                "Gamma",
                            );
                        });
                    if self.normalization != prev_norm {
                        changed = true;
                    }
                });

                if self.normalization == Normalization::Gamma {
                    ui.horizontal(|ui| {
                        ui.label("Gamma:");
                        let prev = self.gamma;
                        ui.add(
                            egui::DragValue::new(&mut self.gamma)
                                .speed(0.1)
                                .range(0.1..=10.0),
                        );
                        if self.gamma != prev {
                            changed = true;
                        }
                    });
                }

                ui.separator();

                let prev_auto = self.autoscale;
                ui.checkbox(&mut self.autoscale, "Autoscale");
                if self.autoscale != prev_auto {
                    changed = true;
                }

                if self.autoscale {
                    ui.add_enabled(false, egui::DragValue::new(&mut self.vmin).prefix("Min: "));
                    ui.add_enabled(false, egui::DragValue::new(&mut self.vmax).prefix("Max: "));
                } else {
                    let prev_vmin = self.vmin;
                    let prev_vmax = self.vmax;
                    ui.add(
                        egui::DragValue::new(&mut self.vmin)
                            .prefix("Min: ")
                            .speed(0.1),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.vmax)
                            .prefix("Max: ")
                            .speed(0.1),
                    );
                    if self.vmin != prev_vmin || self.vmax != prev_vmax {
                        changed = true;
                    }
                }
            });

        self.open = open;

        if changed {
            self.apply(plot);
        }
    }

    /// Re-calculate and apply the colormap to the plot.
    pub fn apply(&self, plot: &mut Plot2D) {
        let mut final_vmin = self.vmin;
        let mut final_vmax = self.vmax;

        if self.autoscale {
            // Very naive autoscale: use [0.0, 1.0] if no images, otherwise try to get stats.
            // Since we can't easily query image stats globally here without iterating items,
            // we will just assume Plot2D handles it, or just use 0..1 for now.
            // Ideally we'd query the stats of the main image.
            let mut found_stats = false;
            if let Some(&handle) = plot.get_all_images().first()
                && let Some(stats) = plot.image_stats(handle)
                && let Some(scalar) = &stats.scalar
                && let (Some(smin), Some(smax)) = (scalar.min, scalar.max)
            {
                final_vmin = smin;
                final_vmax = smax;
                found_stats = true;
            }
            if !found_stats {
                final_vmin = 0.0;
                final_vmax = 1.0;
            }
        }

        let cmap = Colormap::new(self.name, final_vmin, final_vmax)
            .with_normalization(self.normalization)
            .with_gamma(self.gamma);

        plot.set_default_colormap(cmap);
    }
}
