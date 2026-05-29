//! Colormaps.
//!
//! A colormap is a 256-entry RGBA lookup table plus a value range (`vmin`,
//! `vmax`). The image shader normalizes each scalar to `[0, 1]` against the
//! range and indexes the LUT (`doc/design.md` §5).
//!
//! Scope: linear normalization only. Log/sqrt/gamma/arcsinh, NaN sentinel
//! handling, and autoscale (`vmin`/`vmax = None`) arrive in later steps.

/// A 256-color lookup table with a linear value range.
///
/// `vmin`/`vmax` are the data values mapped to the first and last LUT entries.
/// Precondition: `vmax > vmin`.
#[derive(Clone, Debug, PartialEq)]
pub struct Colormap {
    /// 256 RGBA entries, sRGB-encoded (uploaded to an sRGB LUT texture).
    pub lut: [[u8; 4]; 256],
    pub vmin: f64,
    pub vmax: f64,
}

impl Colormap {
    /// The perceptually-uniform "viridis" colormap over `[vmin, vmax]`.
    pub fn viridis(vmin: f64, vmax: f64) -> Self {
        let mut lut = [[0u8; 4]; 256];
        for (i, entry) in lut.iter_mut().enumerate() {
            let c = colorous::VIRIDIS.eval_continuous(i as f64 / 255.0);
            *entry = [c.r, c.g, c.b, 255];
        }
        Self { lut, vmin, vmax }
    }
}
