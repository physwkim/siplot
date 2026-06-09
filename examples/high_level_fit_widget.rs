//! Fit Widget Example.
//!
//! Demonstrates the `FitWidget` together with `ItemsSelectionDialog` reuse: when
//! several curves are present the dialog (configured single-select, curve and
//! histogram only) picks which one to fit. This mirrors silx `actions/fit.py`'s
//! `_initFit`, which builds an `ItemsSelectionDialog`, calls
//! `setItemsSelectionMode(SingleSelection)` and
//! `setAvailableKinds(["curve", "histogram"])`, then runs `setData` on the
//! chosen item.
//!
//! Run with: `cargo run --example high_level_fit_widget`

use eframe::egui;
use siplot::{FitWidget, ItemsSelectionDialog, PlotItemKind, SelectableItem, SelectionMode};

/// One fittable dataset shown in the picker.
struct Dataset {
    label: &'static str,
    x: Vec<f64>,
    y: Vec<f64>,
}

fn gaussian(mu: f64, sigma: f64, a: f64, bg: f64) -> Dataset {
    let mut x = Vec::with_capacity(100);
    let mut y = Vec::with_capacity(100);
    for i in 0..100 {
        let xi = i as f64 * 0.1;
        // Simple deterministic pseudo-noise so the example needs no rng.
        let noise = ((i * 12345) % 100) as f64 / 100.0 - 0.5;
        let z = (xi - mu) / sigma;
        let yi = a * (-0.5 * z * z).exp() + bg + noise * 1.5;
        x.push(xi);
        y.push(yi);
    }
    Dataset { label: "", x, y }
}

struct FitApp {
    fit_widget: FitWidget,
    datasets: Vec<Dataset>,
    /// Curve picker (silx `ItemsSelectionDialog`), single-select over curves.
    picker: ItemsSelectionDialog,
    /// Index of the dataset currently feeding the fit widget.
    fitted: Option<usize>,
}

impl FitApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("eframe must use the wgpu renderer");

        let mut fit_widget = FitWidget::new(rs, 0);
        fit_widget.set_open(true);

        let datasets = vec![
            Dataset {
                label: "narrow peak",
                ..gaussian(5.0, 1.0, 10.0, 2.0)
            },
            Dataset {
                label: "wide peak",
                ..gaussian(6.0, 2.0, 6.0, 1.0)
            },
            Dataset {
                label: "tall peak",
                ..gaussian(4.0, 0.7, 14.0, 3.0)
            },
        ];

        // Configure the dialog exactly as silx's fit tool does: only fittable
        // kinds offered, single selection, the first curve picked initially.
        let mut picker = ItemsSelectionDialog::new(
            datasets
                .iter()
                .enumerate()
                .map(|(i, d)| SelectableItem::new(d.label, PlotItemKind::Curve, i == 0))
                .collect(),
        );
        picker.set_available_kinds(&[PlotItemKind::Curve, PlotItemKind::Histogram]);
        picker.set_selection_mode(SelectionMode::Single);

        let mut app = Self {
            fit_widget,
            datasets,
            picker,
            fitted: None,
        };
        app.sync_fitted_item();
        app
    }

    /// Feed the picker's currently selected dataset to the fit widget, if it
    /// changed (silx `_setFittedItem` → `FitWidget.setData`).
    fn sync_fitted_item(&mut self) {
        // The picker is single-select, so at most one label comes back.
        let chosen = self
            .picker
            .selected_items()
            .next()
            .map(|it| it.label.to_string());
        let chosen_idx =
            chosen.and_then(|label| self.datasets.iter().position(|d| d.label == label));
        if chosen_idx != self.fitted {
            self.fitted = chosen_idx;
            if let Some(i) = chosen_idx {
                let d = &self.datasets[i];
                self.fit_widget.set_data(&d.x, &d.y);
            }
        }
    }
}

impl eframe::App for FitApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("fit_curve_picker")
            .resizable(true)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading("Curve to fit");
                self.picker.ui(ui);
            });
        // Apply any selection change to the fit widget's data.
        self.sync_fitted_item();

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.fit_widget.is_open() {
                self.fit_widget.show(ui.ctx());
            } else if ui.button("Open Fit Widget").clicked() {
                self.fit_widget.set_open(true);
            }
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "siplot: Fit Widget",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(FitApp::new(cc)) as Box<dyn eframe::App>)),
    )
}
