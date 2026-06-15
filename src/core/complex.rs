//! Complex-data visualisation mode — the shared silx `ComplexMixIn.ComplexMode`.
//!
//! silx defines one `ComplexMode` on the `ComplexMixIn` base shared by the 2D
//! `ImageComplexData` ([`crate::widget::complex_image_view`]) and the 3D
//! `ComplexField3D` ([`crate::render::scene3d_items`]). It lives in `core` (below
//! both `render` and `widget`) so the single enum serves both without inverting
//! the crate layering.

/// Visualization mode for complex data (2D image or 3D field).
///
/// Mirrors `ComplexMixIn.ComplexMode` in silx. Each scalar mode maps a complex
/// sample `(re, im)` to a single `f32` via [`ComplexMode::to_scalar`];
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
    /// HSV composite: hue from the phase, value from the linearly normalized
    /// amplitude (silx `AMPLITUDE_PHASE`).
    AmplitudePhase,
    /// HSV composite: hue from the phase, value from the log10-normalized
    /// amplitude over a settable displayed range (silx `LOG10_AMPLITUDE_PHASE`,
    /// see [`crate::widget::complex_image_view::amplitude_phase_log_rgba`]).
    Log10AmplitudePhase,
}

impl ComplexMode {
    /// All modes in the silx menu order, for building a picker.
    pub const ALL: [ComplexMode; 8] = [
        ComplexMode::Absolute,
        ComplexMode::SquareAmplitude,
        ComplexMode::Phase,
        ComplexMode::Real,
        ComplexMode::Imaginary,
        ComplexMode::Log10Amplitude,
        ComplexMode::AmplitudePhase,
        // silx groups LOG10_AMPLITUDE_PHASE right after AMPLITUDE_PHASE.
        ComplexMode::Log10AmplitudePhase,
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
            ComplexMode::Log10AmplitudePhase => "Log10 Amplitude and Phase",
        }
    }

    /// `true` for modes whose displayed image is an RGBA composite rather than a
    /// colormapped scalar ([`ComplexMode::AmplitudePhase`] and
    /// [`ComplexMode::Log10AmplitudePhase`]).
    pub fn is_rgba(self) -> bool {
        matches!(
            self,
            ComplexMode::AmplitudePhase | ComplexMode::Log10AmplitudePhase
        )
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
    /// Returns `0.0` for the RGBA composite modes
    /// ([`ComplexMode::AmplitudePhase`] / [`ComplexMode::Log10AmplitudePhase`]),
    /// which have no scalar representation.
    pub fn to_scalar(self, re: f32, im: f32) -> f32 {
        match self {
            ComplexMode::Absolute => re.hypot(im),
            ComplexMode::Phase => im.atan2(re),
            ComplexMode::Real => re,
            ComplexMode::Imaginary => im,
            ComplexMode::SquareAmplitude => re * re + im * im,
            ComplexMode::Log10Amplitude => re.hypot(im).log10(),
            ComplexMode::AmplitudePhase | ComplexMode::Log10AmplitudePhase => 0.0,
        }
    }
}
