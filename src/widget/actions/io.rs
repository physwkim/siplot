//! Plot I/O actions, mirroring silx `silx.gui.plot.actions.io`.
//!
//! The figure-save (PNG) and data-save (CSV) behaviors here mirror silx
//! `SaveAction` (`actions/io.py`). The load-bearing logic — mapping a chosen
//! file extension to a [`SaveTarget`] and serializing a curve's `(x, y)` to CSV
//! — is pure and unit-tested; the native `rfd` file dialog and the GPU figure
//! readback are thin untestable shims around it.

use std::path::Path;

use crate::render::save::SaveFormat;

/// silx default CSV `%.18e`-style float format width: 18 significant digits in
/// scientific notation (silx `SaveAction` `CURVE_FILTERS_TXT` `","`-CSV filter
/// uses `fmt="%.18e"`).
const CSV_FLOAT_PRECISION: usize = 18;

/// What a chosen save path resolves to, mirroring silx `SaveAction` splitting
/// its name-filters into figure snapshots and curve-data exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTarget {
    /// Save the figure as a raster image in the given [`SaveFormat`] (silx
    /// `SNAPSHOT_FILTER_*`). The GPU figure readback is the untestable shim.
    Figure(SaveFormat),
    /// Save the active curve's `(x, y)` data as CSV (silx `","`-separated
    /// `DEFAULT_CURVE_FILTERS` CSV).
    CurveCsv,
}

impl SaveTarget {
    /// Resolve a file extension (case-insensitive, no leading dot) to a save
    /// target. `csv` saves curve data; the raster extensions recognized by
    /// [`SaveFormat::from_extension`] (`png`, `ppm`, `svg`, `tif`/`tiff`) save
    /// the figure. Returns `None` for unknown extensions.
    pub fn from_extension(ext: &str) -> Option<Self> {
        if ext.eq_ignore_ascii_case("csv") {
            return Some(SaveTarget::CurveCsv);
        }
        SaveFormat::from_extension(ext).map(SaveTarget::Figure)
    }

    /// Resolve a path's extension to a save target via [`Self::from_extension`].
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }
}

/// Format a single `f64` as silx `%.18e` does: lowercase scientific notation
/// with [`CSV_FLOAT_PRECISION`] digits after the decimal point.
fn format_csv_float(v: f64) -> String {
    format!("{v:.*e}", CSV_FLOAT_PRECISION)
}

/// Serialize a curve's `(x, y)` to silx-style `,`-separated CSV: a header line
/// `x,y` followed by one `xval,yval` row per point, `%.18e`-formatted, `\n`
/// line endings (silx `SaveAction._saveCurve` → `save1D` with the default
/// `","`-CSV filter: `header=True`, `delimiter=","`, `fmt="%.18e"`).
///
/// `x` and `y` must have equal length; on a length mismatch the shorter is
/// followed (each row needs both columns), matching a zipped write. Pure, so the
/// exact byte output is unit-testable without touching the filesystem.
pub fn curve_to_csv(x: &[f64], y: &[f64]) -> String {
    let mut out = String::from("x,y\n");
    for (xv, yv) in x.iter().zip(y.iter()) {
        out.push_str(&format_csv_float(*xv));
        out.push(',');
        out.push_str(&format_csv_float(*yv));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_target_from_extension_maps_csv_and_raster() {
        assert_eq!(
            SaveTarget::from_extension("csv"),
            Some(SaveTarget::CurveCsv)
        );
        assert_eq!(
            SaveTarget::from_extension("CSV"),
            Some(SaveTarget::CurveCsv)
        );
        assert_eq!(
            SaveTarget::from_extension("png"),
            Some(SaveTarget::Figure(SaveFormat::Png))
        );
        assert_eq!(
            SaveTarget::from_extension("PNG"),
            Some(SaveTarget::Figure(SaveFormat::Png))
        );
        assert_eq!(
            SaveTarget::from_extension("svg"),
            Some(SaveTarget::Figure(SaveFormat::Svg))
        );
        // Unknown / matplotlib-only extensions are rejected.
        assert_eq!(SaveTarget::from_extension("pdf"), None);
        assert_eq!(SaveTarget::from_extension("xyz"), None);
    }

    #[test]
    fn save_target_from_path_uses_extension() {
        assert_eq!(
            SaveTarget::from_path(Path::new("/tmp/curve.csv")),
            Some(SaveTarget::CurveCsv)
        );
        assert_eq!(
            SaveTarget::from_path(Path::new("/tmp/plot.png")),
            Some(SaveTarget::Figure(SaveFormat::Png))
        );
        assert_eq!(SaveTarget::from_path(Path::new("/tmp/noext")), None);
    }

    #[test]
    fn curve_to_csv_produces_exact_silx_style_output() {
        let x = [0.0, 1.5];
        let y = [-2.0, 3.25];
        let csv = curve_to_csv(&x, &y);
        let expected = "x,y\n\
             0.000000000000000000e0,-2.000000000000000000e0\n\
             1.500000000000000000e0,3.250000000000000000e0\n";
        assert_eq!(csv, expected);
    }

    #[test]
    fn curve_to_csv_empty_is_header_only() {
        assert_eq!(curve_to_csv(&[], &[]), "x,y\n");
    }
}
