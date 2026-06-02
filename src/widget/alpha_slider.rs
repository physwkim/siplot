//! Alpha (opacity) slider.
//!
//! [`AlphaSlider`] mirrors silx `BaseAlphaSlider` (`AlphaSlider.py:86-156`): a
//! slider whose state is an integer in `0..=255`, exposing the corresponding
//! opacity as a float in `[0.0, 1.0]` with a step of `1/255`. silx's concrete
//! subclasses (`ActiveImageAlphaSlider`, `NamedItemAlphaSlider`) bind the value
//! to a specific plot item; that binding needs the plot model and is deferred
//! (see this wave's report) — this is the standalone base.

use egui::{Response, Slider};

/// Orientation of the slider track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlphaSliderOrientation {
    /// Track runs left→right (egui's default).
    #[default]
    Horizontal,
    /// Track runs bottom→top.
    Vertical,
}

/// A 0..=255 integer opacity slider, exposing alpha as both `u8` and `f32`.
///
/// The integer is the canonical state (silx stores `0..255` and emits it from
/// `valueChanged`); the float opacity is `value / 255` (silx
/// `BaseAlphaSlider.getAlpha`). Setters accept either form and clamp into range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlphaSlider {
    /// Opacity as a `0..=255` integer (silx's internal slider state).
    value: u8,
    /// Track orientation.
    pub orientation: AlphaSliderOrientation,
}

impl Default for AlphaSlider {
    /// Fully opaque (silx initializes a slider with no item to 255).
    fn default() -> Self {
        Self {
            value: 255,
            orientation: AlphaSliderOrientation::default(),
        }
    }
}

impl AlphaSlider {
    /// A slider at integer opacity `value` (`0..=255`).
    pub fn new(value: u8) -> Self {
        Self {
            value,
            orientation: AlphaSliderOrientation::default(),
        }
    }

    /// A slider at float opacity `alpha` (`[0.0, 1.0]`); see [`Self::set_alpha`]
    /// for the conversion.
    pub fn from_alpha(alpha: f32) -> Self {
        let mut s = Self::default();
        s.set_alpha(alpha);
        s
    }

    /// Set the track orientation (builder form).
    pub fn with_orientation(mut self, orientation: AlphaSliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// The opacity as a `0..=255` integer (silx `QSlider.value`).
    pub fn value(&self) -> u8 {
        self.value
    }

    /// Set the opacity from a `0..=255` integer.
    pub fn set_value(&mut self, value: u8) {
        self.value = value;
    }

    /// The opacity as a float in `[0.0, 1.0]`, `value / 255` (silx
    /// `BaseAlphaSlider.getAlpha`).
    pub fn alpha(&self) -> f32 {
        alpha_from_u8(self.value)
    }

    /// Set the opacity from a float in `[0.0, 1.0]`. The float is clamped into
    /// range and converted to the nearest integer, mirroring silx's
    /// `round(255 * alpha)` when seeding the slider from an item's alpha.
    pub fn set_alpha(&mut self, alpha: f32) {
        self.value = u8_from_alpha(alpha);
    }

    /// Show the slider; returns its [`egui::Response`]. When the value changes
    /// the response reports `changed()`, matching silx's `valueChanged` signal.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Response {
        match self.orientation {
            AlphaSliderOrientation::Horizontal => {
                ui.add(Slider::new(&mut self.value, 0..=255).text("alpha"))
            }
            AlphaSliderOrientation::Vertical => {
                ui.add(Slider::new(&mut self.value, 0..=255).vertical())
            }
        }
    }
}

/// Convert a `0..=255` integer opacity to a float in `[0.0, 1.0]`: `value / 255`
/// (silx `BaseAlphaSlider.getAlpha`).
fn alpha_from_u8(value: u8) -> f32 {
    value as f32 / 255.0
}

/// Convert a float opacity in `[0.0, 1.0]` to a `0..=255` integer: clamp then
/// `round(255 * alpha)` (silx `round(255 * alpha)`).
fn u8_from_alpha(alpha: f32) -> u8 {
    let clamped = alpha.clamp(0.0, 1.0);
    (clamped * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- u8 <-> f32 conversion boundaries --------------------------------

    #[test]
    fn alpha_from_u8_at_boundaries() {
        assert_eq!(alpha_from_u8(0), 0.0);
        assert_eq!(alpha_from_u8(255), 1.0);
        // 128 / 255 ~= 0.50196.
        assert!((alpha_from_u8(128) - 128.0 / 255.0).abs() < 1e-7);
    }

    #[test]
    fn u8_from_alpha_at_boundaries() {
        assert_eq!(u8_from_alpha(0.0), 0);
        assert_eq!(u8_from_alpha(1.0), 255);
        // round(255 * 0.5) = round(127.5) = 128 (round-half-away-from-zero).
        assert_eq!(u8_from_alpha(0.5), 128);
    }

    #[test]
    fn u8_from_alpha_clamps_out_of_range() {
        assert_eq!(u8_from_alpha(-0.5), 0);
        assert_eq!(u8_from_alpha(2.0), 255);
        // NaN clamps via f32::clamp's NaN handling to the low bound.
        assert_eq!(u8_from_alpha(f32::NAN), 0);
    }

    #[test]
    fn u8_from_alpha_rounds_to_nearest() {
        // round(255 * 0.4980) = round(126.99) = 127.
        assert_eq!(u8_from_alpha(0.498), 127);
        // round(255 * 0.502) = round(128.01) = 128.
        assert_eq!(u8_from_alpha(0.502), 128);
    }

    // --- widget value/setters --------------------------------------------

    #[test]
    fn default_is_fully_opaque() {
        let s = AlphaSlider::default();
        assert_eq!(s.value(), 255);
        assert_eq!(s.alpha(), 1.0);
        assert_eq!(s.orientation, AlphaSliderOrientation::Horizontal);
    }

    #[test]
    fn new_sets_integer_value() {
        let s = AlphaSlider::new(0);
        assert_eq!(s.value(), 0);
        assert_eq!(s.alpha(), 0.0);
    }

    #[test]
    fn from_alpha_rounds_to_integer() {
        // round(255 * 0.5) = 128.
        let s = AlphaSlider::from_alpha(0.5);
        assert_eq!(s.value(), 128);
        // Clamps out-of-range alpha.
        assert_eq!(AlphaSlider::from_alpha(2.0).value(), 255);
        assert_eq!(AlphaSlider::from_alpha(-1.0).value(), 0);
    }

    #[test]
    fn set_value_and_set_alpha_round_trip_at_boundaries() {
        let mut s = AlphaSlider::new(0);

        s.set_value(255);
        assert_eq!(s.value(), 255);
        assert_eq!(s.alpha(), 1.0);

        s.set_value(128);
        assert_eq!(s.value(), 128);
        assert!((s.alpha() - 128.0 / 255.0).abs() < 1e-7);

        s.set_alpha(0.0);
        assert_eq!(s.value(), 0);

        s.set_alpha(1.0);
        assert_eq!(s.value(), 255);
    }

    #[test]
    fn with_orientation_sets_vertical() {
        let s = AlphaSlider::default().with_orientation(AlphaSliderOrientation::Vertical);
        assert_eq!(s.orientation, AlphaSliderOrientation::Vertical);
    }
}
