use egui::Color32;

use crate::core::backend::ItemHandle;
use crate::widget::high_level::Plot2D;
use crate::widget::plot_widget::PlotResponse;

/// Drawing tools mirroring silx `_BaseMaskToolsWidget` draw modes
/// (rectangle, ellipse, polygon, pencil/eraser).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskTool {
    None,
    Pencil,
    Eraser,
    Rectangle,
    Polygon,
    Ellipse,
}

/// A widget for interactively drawing a multi-level mask over a 2D image.
///
/// The mask mirrors silx `ImageMask`: a `uint8` array the same shape as the
/// image, where `0` means unmasked and `1..=255` are the (up to 254)
/// non-overlapping mask levels. Drawing writes the current [`level`], the
/// eraser clears it back to `0`.
///
/// [`level`]: Self::level
pub struct MaskToolsWidget {
    /// Per-pixel mask level in image (row, col) order: `0` is unmasked,
    /// `1..=255` is a mask level.
    pub mask: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub color: Color32,

    /// Current mask level edited by the drawing tools (silx `levelSpinBox`,
    /// range 1..=255).
    pub level: u8,

    pub active_tool: MaskTool,
    pub brush_size: u32,

    mask_handle: Option<ItemHandle>,
    is_dirty: bool,
}

impl MaskToolsWidget {
    /// Create a new MaskToolsWidget for an image of the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            mask: vec![0; (width * height) as usize],
            width,
            height,
            color: Color32::from_rgba_unmultiplied(255, 0, 0, 128), // Default semi-transparent red
            level: 1,
            active_tool: MaskTool::None,
            brush_size: 1,
            mask_handle: None,
            is_dirty: true, // Force initial upload
        }
    }

    /// Reset the mask to the given dimensions and clear it.
    pub fn reset_geometry(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.mask = vec![0; (width * height) as usize];
        self.is_dirty = true;
    }

    /// Set all pixels of the current level back to `0`.
    ///
    /// Mirrors silx `BaseMask.clear(level)`.
    pub fn clear(&mut self) {
        let level = self.level;
        for cell in &mut self.mask {
            if *cell == level {
                *cell = 0;
            }
        }
        self.is_dirty = true;
    }

    /// Clear every mask level (reset the whole mask to `0`).
    ///
    /// Mirrors silx `resetSelectionMask` (`reset()` to zeros).
    pub fn clear_all(&mut self) {
        self.mask.fill(0);
        self.is_dirty = true;
    }

    /// Apply the mask onto a `Plot2D`.
    ///
    /// This should be called every frame after handling interaction,
    /// so the mask visual overlay stays up-to-date.
    pub fn apply(&mut self, plot: &mut Plot2D) {
        if !self.is_dirty {
            return;
        }

        // Build RGBA from the level buffer: any masked pixel (non-zero level)
        // is painted with the overlay color, unmasked pixels are transparent.
        let rgba: Vec<[u8; 4]> = self
            .mask
            .iter()
            .map(|&level| {
                if level != 0 {
                    [
                        self.color.r(),
                        self.color.g(),
                        self.color.b(),
                        self.color.a(),
                    ]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect();

        if let Some(handle) = self.mask_handle {
            // Update existing mask image
            if plot
                .try_update_rgba_image(handle, self.width, self.height, &rgba)
                .is_err()
            {
                // If update fails (e.g. size mismatch), clear handle to force recreation
                plot.remove(handle);
                self.mask_handle = None;
            }
        }

        if self.mask_handle.is_none() {
            // Mark the overlay as a mask item: derive a boolean (masked /
            // unmasked) view from the level buffer for the existing API.
            let bool_view: Vec<bool> = self.mask.iter().map(|&level| level != 0).collect();
            if let Ok(handle) = plot.add_mask(self.width, self.height, &bool_view, self.color) {
                self.mask_handle = Some(handle);
            }
        }

        self.is_dirty = false;
    }

    /// Show the masking tools toolbar.
    pub fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Mask:");

            ui.selectable_value(&mut self.active_tool, MaskTool::None, "○")
                .on_hover_text("Disable masking");
            ui.selectable_value(&mut self.active_tool, MaskTool::Pencil, "Pencil")
                .on_hover_text("Draw mask");
            ui.selectable_value(&mut self.active_tool, MaskTool::Eraser, "Eraser")
                .on_hover_text("Erase mask");
            ui.selectable_value(&mut self.active_tool, MaskTool::Rectangle, "Rectangle")
                .on_hover_text("Mask a rectangular region");
            ui.selectable_value(&mut self.active_tool, MaskTool::Polygon, "Polygon")
                .on_hover_text("Mask a polygonal region");
            ui.selectable_value(&mut self.active_tool, MaskTool::Ellipse, "Ellipse")
                .on_hover_text("Mask an elliptical region");

            ui.add(egui::Slider::new(&mut self.level, 1..=255).text("Mask level"));

            if self.active_tool != MaskTool::None {
                ui.add(egui::Slider::new(&mut self.brush_size, 1..=50).text("Brush size"));
            }

            if ui.button("Clear").clicked() {
                self.clear();
            }
            if ui.button("Clear All").clicked() {
                self.clear_all();
            }
        });
    }

    /// Handle pointer interaction from the plot response to paint/erase the mask.
    pub fn handle_interaction(&mut self, plot_response: &PlotResponse) {
        if !matches!(self.active_tool, MaskTool::Pencil | MaskTool::Eraser) {
            return;
        }

        // Only draw when the primary pointer button is held down
        if (plot_response
            .response
            .dragged_by(egui::PointerButton::Primary)
            || plot_response
                .response
                .clicked_by(egui::PointerButton::Primary))
            && let Some(pointer_pos) = plot_response.response.interact_pointer_pos()
        {
            let (data_x, data_y) = plot_response.transform.pixel_to_data(pointer_pos);
            let center_col = data_x.floor() as i64;
            let center_row = data_y.floor() as i64;

            let value = if self.active_tool == MaskTool::Pencil {
                self.level
            } else {
                0
            };
            let r = self.brush_size as i64;
            let r_squared = r * r;

            let w = self.width as i64;
            let h = self.height as i64;

            let mut changed = false;

            for dy in -r..=r {
                for dx in -r..=r {
                    // Circular brush
                    if dx * dx + dy * dy <= r_squared {
                        let col = center_col + dx;
                        let row = center_row + dy;

                        if col >= 0 && col < w && row >= 0 && row < h {
                            let idx = (row as usize) * (self.width as usize) + (col as usize);
                            if self.mask[idx] != value {
                                self.mask[idx] = value;
                                changed = true;
                            }
                        }
                    }
                }
            }

            if changed {
                self.is_dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_only_affects_current_level() {
        // Mirrors silx BaseMask.clear(level): only `level` pixels go to 0.
        let mut w = MaskToolsWidget::new(2, 2);
        w.mask = vec![1, 2, 1, 0];
        w.level = 1;
        w.clear();
        assert_eq!(w.mask, vec![0, 2, 0, 0]);
    }

    #[test]
    fn clear_all_resets_every_level() {
        // Mirrors silx resetSelectionMask: whole buffer back to 0.
        let mut w = MaskToolsWidget::new(2, 2);
        w.mask = vec![1, 2, 255, 0];
        w.clear_all();
        assert_eq!(w.mask, vec![0, 0, 0, 0]);
    }
}
