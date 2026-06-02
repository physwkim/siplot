//! Display of a single 2D complex dataset with selectable visualization mode.
//!
//! Ports silx `ComplexImageView.py` (and the conversion math of
//! `silx/gui/plot/items/complex.py`, `ImageComplexData`). A
//! [`ComplexImageView`] owns a [`Plot2D`], the complex data, and the current
//! [`ComplexMode`]; switching modes recomputes the displayed image in place
//! without resetting the zoom.
//!
//! Scalar modes feed a colormapped `f32` image to the plot; `AMPLITUDE_PHASE`
//! feeds an HSV-composite RGBA image. silx maps the phase through a `hsv`
//! colormap over `[-pi, pi]`, which is what the [`phase_hsv_lut`] /
//! [`hsv_to_rgb`] helpers reproduce here (the crate colormap catalog has no
//! `hsv` entry).

use egui_wgpu::RenderState;

use crate::core::backend::{ImageSpec, ItemHandle};
use crate::core::colormap::{Colormap, Normalization};
use crate::core::plot::PlotId;
use crate::widget::high_level::{Plot2D, PlotDataError};
use crate::widget::plot_widget::PlotResponse;

/// Visualization mode for complex 2D data.
///
/// Mirrors `ImageComplexData.ComplexMode` in silx. Each scalar mode maps a
/// complex sample `(re, im)` to a single `f32` via [`ComplexMode::to_scalar`];
/// [`ComplexMode::AmplitudePhase`] instead produces an RGBA composite.
///
/// silx exposes `ABSOLUTE`, `PHASE`, `REAL`, `IMAGINARY`, `SQUARE_AMPLITUDE`,
/// `AMPLITUDE_PHASE`, and `LOG10_AMPLITUDE_PHASE`. The first six are mirrored
/// directly; `Log10Amplitude` here is the scalar `log10(|z|)` map (silx only
/// uses log10 amplitude inside its RGBA `LOG10_AMPLITUDE_PHASE` composite).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComplexMode {
    /// `|z|` — the absolute value (`numpy.absolute`).
    Absolute,
    /// `angle(z)` — the phase in `[-pi, pi]` (`numpy.angle`).
    Phase,
    /// `re(z)` — the real part.
    Real,
    /// `im(z)` — the imaginary part.
    Imaginary,
    /// `|z|^2` — the square amplitude (`numpy.absolute(z) ** 2`).
    SquareAmplitude,
    /// `log10(|z|)` — the base-10 log of the amplitude.
    Log10Amplitude,
    /// HSV composite: hue from the phase, value from the normalized amplitude.
    AmplitudePhase,
}

impl ComplexMode {
    /// All modes in the silx menu order, for building a picker.
    pub const ALL: [ComplexMode; 7] = [
        ComplexMode::Absolute,
        ComplexMode::SquareAmplitude,
        ComplexMode::Phase,
        ComplexMode::Real,
        ComplexMode::Imaginary,
        ComplexMode::Log10Amplitude,
        ComplexMode::AmplitudePhase,
    ];

    /// Human-readable label, matching the silx menu text.
    pub fn label(self) -> &'static str {
        match self {
            ComplexMode::Absolute => "Amplitude",
            ComplexMode::SquareAmplitude => "Square amplitude",
            ComplexMode::Phase => "Phase",
            ComplexMode::Real => "Real part",
            ComplexMode::Imaginary => "Imaginary part",
            ComplexMode::Log10Amplitude => "Log10(amplitude)",
            ComplexMode::AmplitudePhase => "Amplitude and Phase",
        }
    }

    /// `true` for modes whose displayed image is an RGBA composite rather than a
    /// colormapped scalar (only [`ComplexMode::AmplitudePhase`]).
    pub fn is_rgba(self) -> bool {
        matches!(self, ComplexMode::AmplitudePhase)
    }

    /// Convert a complex sample `(re, im)` to the scalar shown by this mode.
    ///
    /// Faithful to silx `ImageComplexData.__convertComplexData`:
    /// - `Absolute`        → `hypot(re, im)` = `numpy.absolute`
    /// - `Phase`           → `atan2(im, re)` = `numpy.angle`
    /// - `Real`            → `re`
    /// - `Imaginary`       → `im`
    /// - `SquareAmplitude` → `re^2 + im^2` = `numpy.absolute(z) ** 2`
    /// - `Log10Amplitude`  → `log10(hypot(re, im))`
    ///
    /// Returns `0.0` for [`ComplexMode::AmplitudePhase`], which has no scalar
    /// representation (use [`amplitude_phase_rgba`] instead).
    pub fn to_scalar(self, re: f32, im: f32) -> f32 {
        match self {
            ComplexMode::Absolute => re.hypot(im),
            ComplexMode::Phase => im.atan2(re),
            ComplexMode::Real => re,
            ComplexMode::Imaginary => im,
            ComplexMode::SquareAmplitude => re * re + im * im,
            ComplexMode::Log10Amplitude => re.hypot(im).log10(),
            ComplexMode::AmplitudePhase => 0.0,
        }
    }
}

/// Map an HSV triple to sRGB bytes (saturation and value in `[0, 1]`, hue
/// wrapped into `[0, 1)`).
///
/// This is the standard HSV→RGB conversion silx's `hsv` colormap performs;
/// at `s == 1`, `v == 1` it sweeps the full hue circle (red → yellow → green →
/// cyan → blue → magenta → red), which is how silx color-codes the phase.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match sector as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round().clamp(0.0, 255.0) as u8,
        (g * 255.0).round().clamp(0.0, 255.0) as u8,
        (b * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Build the 256-entry `hsv` LUT silx uses for the phase colormap: hue swept
/// linearly across `[0, 1]` at full saturation and value.
pub fn phase_hsv_lut() -> [[u8; 4]; 256] {
    let mut lut = [[0u8; 4]; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let [r, g, b] = hsv_to_rgb(i as f32 / 255.0, 1.0, 1.0);
        *entry = [r, g, b, 255];
    }
    lut
}

/// The silx phase colormap: an `hsv` LUT over the fixed range `[-pi, pi]`.
///
/// Used by [`ComplexMode::Phase`] (and the phase channel of the composite),
/// matching silx `Colormap(name="hsv", vmin=-numpy.pi, vmax=numpy.pi)`.
pub fn phase_colormap() -> Colormap {
    Colormap {
        lut: phase_hsv_lut(),
        vmin: -std::f64::consts::PI,
        vmax: std::f64::consts::PI,
        normalization: Normalization::Linear,
        gamma: 2.0,
    }
}

/// Build the `AMPLITUDE_PHASE` RGBA composite for row-major complex `data`.
///
/// Hue is the phase `atan2(im, re)` mapped from `[-pi, pi]` to `[0, 1]`,
/// saturation is `1`, and value is the amplitude `hypot(re, im)` normalized to
/// its maximum over the whole array (matching the task's HSV mapping; silx's
/// `_complex2rgbalin` puts the same normalized amplitude in the alpha channel).
/// Alpha is fully opaque. When the data is empty or the max amplitude is `0`,
/// all values are taken as `0`.
pub fn amplitude_phase_rgba(data: &[(f32, f32)]) -> Vec<[u8; 4]> {
    let max_amp = data
        .iter()
        .map(|&(re, im)| re.hypot(im))
        .fold(0.0f32, f32::max);

    data.iter()
        .map(|&(re, im)| {
            let phase = im.atan2(re);
            // Map [-pi, pi] -> [0, 1]; +pi and -pi share the same hue (red).
            let hue = (phase + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
            let value = if max_amp > 0.0 {
                re.hypot(im) / max_amp
            } else {
                0.0
            };
            let [r, g, b] = hsv_to_rgb(hue, 1.0, value);
            [r, g, b, 255]
        })
        .collect()
}

/// Compute the `[min, max]` of `values` over finite entries, returning
/// `(0.0, 1.0)` when there is no finite value (degenerate range fallback that
/// the colormap maps to its low color).
fn finite_range(values: &[f32]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &v in values {
        if v.is_finite() {
            let v = v as f64;
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
    }
    if min.is_finite() && max.is_finite() && max > min {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

/// Display an image of complex data and let the user choose the visualization.
///
/// Mirrors silx `ComplexImageView`: it owns a [`Plot2D`], the complex data
/// (`(re, im)` pairs) with its width/height, and the current [`ComplexMode`].
///
/// ```ignore
/// let mut view = ComplexImageView::new(render_state, 0);
/// view.set_data(w, h, &samples)?;
///
/// // frame loop
/// view.show_mode_controls(ui);
/// view.show(ui);
/// ```
pub struct ComplexImageView {
    plot: Plot2D,
    width: u32,
    height: u32,
    data: Vec<(f32, f32)>,
    mode: ComplexMode,
    image_handle: Option<ItemHandle>,
    dirty: bool,
}

impl ComplexImageView {
    /// Create a complex image view backed by wgpu plot id `id`.
    ///
    /// The default mode is [`ComplexMode::Absolute`], matching silx's
    /// `ImageComplexData` default.
    pub fn new(render_state: &RenderState, id: PlotId) -> Self {
        Self {
            plot: Plot2D::new(render_state, id),
            width: 0,
            height: 0,
            data: Vec::new(),
            mode: ComplexMode::Absolute,
            image_handle: None,
            dirty: false,
        }
    }

    /// Set the complex data to display.
    ///
    /// `data` is a row-major array of `(re, im)` pairs of length
    /// `width * height`. Returns [`PlotDataError`] on a length mismatch.
    pub fn set_data(
        &mut self,
        width: u32,
        height: u32,
        data: &[(f32, f32)],
    ) -> Result<(), PlotDataError> {
        let expected = (width as usize).saturating_mul(height as usize);
        if data.len() != expected {
            return Err(PlotDataError::ImageDataLength {
                expected,
                actual: data.len(),
            });
        }
        self.width = width;
        self.height = height;
        self.data = data.to_vec();
        self.dirty = true;
        Ok(())
    }

    /// The current visualization mode.
    pub fn mode(&self) -> ComplexMode {
        self.mode
    }

    /// Set the visualization mode, recomputing the displayed image on the next
    /// [`Self::show`] without resetting the zoom.
    pub fn set_mode(&mut self, mode: ComplexMode) {
        if mode != self.mode {
            self.mode = mode;
            self.dirty = true;
        }
    }

    /// Access the underlying [`Plot2D`].
    pub fn plot(&self) -> &Plot2D {
        &self.plot
    }

    /// Mutably access the underlying [`Plot2D`].
    pub fn plot_mut(&mut self) -> &mut Plot2D {
        &mut self.plot
    }

    /// Render the complex image in `ui`, rebuilding the displayed image first
    /// if the data or mode changed.
    pub fn show(&mut self, ui: &mut egui::Ui) -> PlotResponse {
        if self.dirty && !self.data.is_empty() {
            self.rebuild_image();
            self.dirty = false;
        }
        self.plot.show(ui)
    }

    /// A combo box to pick the visualization mode. Returns the current mode.
    ///
    /// Call before [`Self::show`].
    pub fn show_mode_controls(&mut self, ui: &mut egui::Ui) -> ComplexMode {
        egui::ComboBox::from_label("Complex mode")
            .selected_text(self.mode.label())
            .show_ui(ui, |ui| {
                for mode in ComplexMode::ALL {
                    if ui
                        .selectable_label(self.mode == mode, mode.label())
                        .clicked()
                        && self.mode != mode
                    {
                        self.mode = mode;
                        self.dirty = true;
                    }
                }
            });
        self.mode
    }

    /// Recompute the displayed image for the current mode and update the plot
    /// in place (reusing the existing item handle so the zoom is preserved).
    fn rebuild_image(&mut self) {
        if self.mode.is_rgba() {
            let rgba = amplitude_phase_rgba(&self.data);
            self.set_rgba_image(&rgba);
        } else {
            let scalar: Vec<f32> = self
                .data
                .iter()
                .map(|&(re, im)| self.mode.to_scalar(re, im))
                .collect();
            let colormap = self.scalar_colormap(&scalar);
            self.set_scalar_image(&scalar, colormap);
        }
    }

    /// The colormap for a scalar mode: the fixed `hsv` phase colormap for
    /// [`ComplexMode::Phase`], or an autoscaled viridis for every other scalar
    /// mode (silx: phase uses the fixed `[-pi, pi]` hsv colormap, the rest use
    /// the autoscaling default colormap).
    fn scalar_colormap(&self, scalar: &[f32]) -> Colormap {
        if self.mode == ComplexMode::Phase {
            phase_colormap()
        } else {
            let (vmin, vmax) = finite_range(scalar);
            Colormap::viridis(vmin, vmax)
        }
    }

    /// Upload or replace the scalar image, preserving the zoom on update.
    fn set_scalar_image(&mut self, scalar: &[f32], colormap: Colormap) {
        if let Some(handle) = self.image_handle {
            let spec = ImageSpec::scalar(self.width, self.height, scalar, colormap.clone());
            if self.plot.update_image_spec(handle, spec) {
                return;
            }
        }
        let handle = self
            .plot
            .try_add_image(self.width, self.height, scalar, colormap)
            .expect("scalar length validated by set_data");
        self.image_handle = Some(handle);
    }

    /// Upload or replace the RGBA composite image, preserving the zoom on
    /// update.
    fn set_rgba_image(&mut self, rgba: &[[u8; 4]]) {
        if let Some(handle) = self.image_handle {
            let spec = ImageSpec::rgba(self.width, self.height, rgba);
            if self.plot.update_image_spec(handle, spec) {
                return;
            }
        }
        let handle = self
            .plot
            .try_add_rgba_image(self.width, self.height, rgba)
            .expect("rgba length validated by set_data");
        self.image_handle = Some(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: f32 = std::f32::consts::PI;

    // ── Per-mode scalar conversion at known complex values ──────────────────

    #[test]
    fn absolute_is_hypot() {
        assert_eq!(ComplexMode::Absolute.to_scalar(3.0, 4.0), 5.0);
        assert_eq!(ComplexMode::Absolute.to_scalar(0.0, 0.0), 0.0);
    }

    #[test]
    fn phase_is_atan2() {
        // atan2(im, re): real axis -> 0, +imag axis -> +pi/2, -real -> +pi.
        assert_eq!(ComplexMode::Phase.to_scalar(1.0, 0.0), 0.0);
        assert!((ComplexMode::Phase.to_scalar(0.0, 1.0) - PI / 2.0).abs() < 1e-6);
        assert!((ComplexMode::Phase.to_scalar(-1.0, 0.0) - PI).abs() < 1e-6);
        assert!((ComplexMode::Phase.to_scalar(0.0, -1.0) + PI / 2.0).abs() < 1e-6);
    }

    #[test]
    fn real_is_re() {
        assert_eq!(ComplexMode::Real.to_scalar(3.0, 4.0), 3.0);
        assert_eq!(ComplexMode::Real.to_scalar(-2.5, 9.0), -2.5);
    }

    #[test]
    fn imaginary_is_im() {
        assert_eq!(ComplexMode::Imaginary.to_scalar(3.0, 4.0), 4.0);
        assert_eq!(ComplexMode::Imaginary.to_scalar(-2.5, -9.0), -9.0);
    }

    #[test]
    fn square_amplitude_is_re2_plus_im2() {
        assert_eq!(ComplexMode::SquareAmplitude.to_scalar(3.0, 4.0), 25.0);
        assert_eq!(ComplexMode::SquareAmplitude.to_scalar(0.0, 0.0), 0.0);
    }

    #[test]
    fn log10_amplitude_is_log10_of_hypot() {
        // |z| = 100 -> log10 = 2; |z| = 1 -> 0.
        assert!((ComplexMode::Log10Amplitude.to_scalar(100.0, 0.0) - 2.0).abs() < 1e-6);
        assert_eq!(ComplexMode::Log10Amplitude.to_scalar(1.0, 0.0), 0.0);
    }

    #[test]
    fn amplitude_phase_returns_zero_scalar() {
        // No scalar representation; the RGBA path is used instead.
        assert_eq!(ComplexMode::AmplitudePhase.to_scalar(3.0, 4.0), 0.0);
    }

    // ── AMPLITUDE_PHASE HSV mapping at boundary phases ──────────────────────

    #[test]
    fn amplitude_phase_hue_at_zero_phase_is_cyan() {
        // phase 0 -> hue 0.5 (mid of [-pi,pi]->[0,1]) -> cyan; |z| = max -> v = 1.
        // Use a single sample so it is the max amplitude (value = 1).
        let rgba = amplitude_phase_rgba(&[(1.0, 0.0)]);
        // hue = (0 + pi)/(2pi) = 0.5 -> hsv(0.5,1,1) = cyan (0,255,255).
        assert_eq!(rgba, vec![[0, 255, 255, 255]]);
    }

    #[test]
    fn amplitude_phase_hue_at_plus_half_pi() {
        // phase +pi/2 -> hue (pi/2 + pi)/(2pi) = 0.75 -> hsv(0.75,1,1):
        // h*6 = 4.5 (sector 4, f = 0.5) -> (t, p, v) = (0.5, 0, 1) -> violet.
        let rgba = amplitude_phase_rgba(&[(0.0, 1.0)]);
        assert_eq!(rgba, vec![[128, 0, 255, 255]]);
    }

    #[test]
    fn amplitude_phase_hue_at_minus_pi() {
        // phase -pi -> hue 0.0 -> hsv(0,1,1) = red (255,0,0).
        let rgba = amplitude_phase_rgba(&[(-1.0, 0.0)]);
        // atan2(0, -1) = +pi in IEEE, but -0.0 imaginary gives -pi; use exact -pi sample.
        let rgba_neg = amplitude_phase_rgba(&[(-1.0, -0.0)]);
        // +pi maps to hue 1.0 == 0.0 (red) and -pi maps to hue 0.0 (red): both red.
        assert_eq!(rgba, vec![[255, 0, 0, 255]]);
        assert_eq!(rgba_neg, vec![[255, 0, 0, 255]]);
    }

    #[test]
    fn amplitude_phase_value_scales_with_amplitude() {
        // Two samples: max amplitude 2 at phase 0 (cyan, v=1); half amplitude 1
        // at phase 0 (v=0.5 -> half-bright cyan).
        let rgba = amplitude_phase_rgba(&[(2.0, 0.0), (1.0, 0.0)]);
        assert_eq!(rgba[0], [0, 255, 255, 255]);
        // v = 0.5: hsv(0.5, 1, 0.5) -> (0, 128, 128) after rounding.
        assert_eq!(rgba[1], [0, 128, 128, 255]);
    }

    #[test]
    fn amplitude_phase_empty_is_empty() {
        assert!(amplitude_phase_rgba(&[]).is_empty());
    }

    #[test]
    fn amplitude_phase_zero_amplitude_is_black() {
        // max_amp == 0 -> value 0 everywhere -> black.
        let rgba = amplitude_phase_rgba(&[(0.0, 0.0), (0.0, 0.0)]);
        assert_eq!(rgba, vec![[0, 0, 0, 255], [0, 0, 0, 255]]);
    }

    // ── hsv LUT / phase colormap ────────────────────────────────────────────

    #[test]
    fn hsv_lut_endpoints_are_red() {
        // Full hue sweep is cyclic: index 0 and the wrapped end are both red.
        let lut = phase_hsv_lut();
        assert_eq!(lut[0], [255, 0, 0, 255]);
        assert_eq!(hsv_to_rgb(1.0, 1.0, 1.0), [255, 0, 0]);
    }

    #[test]
    fn phase_colormap_range_is_minus_pi_to_pi() {
        let cm = phase_colormap();
        assert_eq!(cm.vmin, -std::f64::consts::PI);
        assert_eq!(cm.vmax, std::f64::consts::PI);
    }

    // ── finite_range boundaries ─────────────────────────────────────────────

    #[test]
    fn finite_range_ignores_non_finite_and_falls_back() {
        assert_eq!(finite_range(&[1.0, 5.0, 3.0]), (1.0, 5.0));
        // All-equal -> degenerate -> fallback (0, 1).
        assert_eq!(finite_range(&[2.0, 2.0]), (0.0, 1.0));
        // NaN/inf are skipped; remaining single finite value is degenerate.
        assert_eq!(finite_range(&[f32::NAN, f32::INFINITY, 4.0]), (0.0, 1.0));
        // No finite values -> fallback.
        assert_eq!(finite_range(&[f32::NAN]), (0.0, 1.0));
    }
}
