//! Cursor position readout bar ported from silx
//! `gui/plot/tools/PositionInfo.py`.
//!
//! [`PositionInfo`] is a horizontal label bar showing the current cursor data
//! coordinates through a list of named converter functions
//! `(name, fn(x, y) -> String)`. It mirrors silx `PositionInfo`
//! (PositionInfo.py:64-318):
//!
//! - The default converters display `X` and `Y` (PositionInfo.py:117).
//! - A polar converter pair (`Radius`, `Angle`) is provided as the silx
//!   documentation example (PositionInfo.py:92-94).
//! - When no cursor position is available the value fields show `"------"`
//!   (PositionInfo.py:131).
//! - Numeric converter results are formatted with `%.7g` (silx
//!   `valueToString`, PositionInfo.py:315) via [`format_value`].
//!
//! Snapping (PositionInfo.py:179-292) and the live `PlotWidget` event wiring
//! (`sigPlotSignal`) are out of scope here: this widget is fed a cursor
//! position by the caller and is independent of any GPU/plot state.

/// A converter mapping cursor data coordinates `(x, y)` to a display string.
///
/// This is the boxed `Fn(f64, f64) -> String` half of the silx
/// `(name, function)` converter pair (PositionInfo.py:127); the convenience
/// constructors wrap numeric silx converters with [`format_value`] (`%.7g`).
pub type Converter = Box<dyn Fn(f64, f64) -> String>;

/// A horizontal cursor-coordinate readout bar.
///
/// Holds an ordered list of `(label, converter)` pairs. Each converter maps
/// the cursor data coordinates `(x, y)` to a display string; numeric silx
/// converters are wrapped with [`format_value`] (`%.7g`) by the convenience
/// constructors.
pub struct PositionInfo {
    converters: Vec<(String, Converter)>,
}

impl Default for PositionInfo {
    /// The silx default: `X` and `Y` columns (PositionInfo.py:117).
    fn default() -> Self {
        Self::with_xy()
    }
}

impl PositionInfo {
    /// Create a readout bar from an explicit list of converters.
    pub fn new(converters: Vec<(String, Converter)>) -> Self {
        Self { converters }
    }

    /// The silx default converters: `X -> x`, `Y -> y` (PositionInfo.py:117),
    /// each formatted with `%.7g`.
    pub fn with_xy() -> Self {
        Self::new(vec![
            ("X".to_owned(), Box::new(|x: f64, _y: f64| format_value(x))),
            ("Y".to_owned(), Box::new(|_x: f64, y: f64| format_value(y))),
        ])
    }

    /// The silx documentation polar example (PositionInfo.py:92-94):
    /// `Radius -> sqrt(x*x + y*y)`, `Angle -> degrees(atan2(y, x))`, each
    /// formatted with `%.7g`.
    pub fn polar() -> Self {
        Self::new(vec![
            (
                "Radius".to_owned(),
                Box::new(|x: f64, y: f64| format_value(x.hypot(y))),
            ),
            (
                "Angle".to_owned(),
                Box::new(|x: f64, y: f64| format_value(y.atan2(x).to_degrees())),
            ),
        ])
    }

    /// Append a converter `(label, fn)` to the bar.
    pub fn push(&mut self, label: impl Into<String>, converter: Converter) {
        self.converters.push((label.into(), converter));
    }

    /// Convenience: append a numeric converter, formatting its result with
    /// `%.7g` (silx `valueToString`, PositionInfo.py:315).
    pub fn push_numeric(
        &mut self,
        label: impl Into<String>,
        converter: impl Fn(f64, f64) -> f64 + 'static,
    ) {
        self.converters.push((
            label.into(),
            Box::new(move |x, y| format_value(converter(x, y))),
        ));
    }

    /// The number of converter columns.
    pub fn len(&self) -> usize {
        self.converters.len()
    }

    /// Whether the bar has no converters.
    pub fn is_empty(&self) -> bool {
        self.converters.is_empty()
    }

    /// Compute the display string for each converter at `(x, y)`.
    ///
    /// Returns one string per column. This is the pure core of [`Self::ui`];
    /// `None` cursor yields the silx empty placeholder `"------"`
    /// (PositionInfo.py:131) for every column.
    pub fn values(&self, cursor: Option<[f64; 2]>) -> Vec<String> {
        match cursor {
            None => vec![EMPTY_PLACEHOLDER.to_owned(); self.converters.len()],
            Some([x, y]) => self
                .converters
                .iter()
                .map(|(_label, func)| func(x, y))
                .collect(),
        }
    }

    /// Render the readout bar: a horizontal row of `<label>: <value>` pairs.
    ///
    /// `cursor` is the current cursor position in data coordinates, or `None`
    /// when the cursor is outside the plot area (silx shows `"------"`).
    pub fn ui(&self, ui: &mut egui::Ui, cursor: Option<[f64; 2]>) {
        ui.horizontal(|ui| {
            let values = self.values(cursor);
            for ((label, _func), value) in self.converters.iter().zip(values) {
                ui.strong(format!("{label}:"));
                ui.label(value);
                ui.add_space(8.0);
            }
        });
    }
}

/// The silx empty-field placeholder shown when no cursor is available
/// (PositionInfo.py:131).
const EMPTY_PLACEHOLDER: &str = "------";

/// Format a numeric value like silx `valueToString` (`"%.7g"`,
/// PositionInfo.py:315): up to 7 significant digits, trailing zeros trimmed,
/// switching to exponential notation for very large/small magnitudes.
///
/// Non-finite values format as `"nan"` / `"inf"` like C `%g` would; we keep
/// Rust's spelling for those edge cases since silx delegates to Python's
/// `%g` which also prints `nan` / `inf`.
pub fn format_value(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 {
            "-inf".to_owned()
        } else {
            "inf".to_owned()
        };
    }
    crate::widget::stats_widget::format_significant(value, 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_xy() {
        let p = PositionInfo::default();
        assert_eq!(p.len(), 2);
        let v = p.values(Some([3.0, 4.0]));
        assert_eq!(v, vec!["3".to_owned(), "4".to_owned()]);
    }

    #[test]
    fn no_cursor_shows_placeholder() {
        let p = PositionInfo::default();
        let v = p.values(None);
        assert_eq!(v, vec!["------".to_owned(), "------".to_owned()]);
    }

    #[test]
    fn polar_radius_is_hypot() {
        let p = PositionInfo::polar();
        // (3,4) -> radius 5, angle atan2(4,3) deg ~= 53.13010...
        let v = p.values(Some([3.0, 4.0]));
        assert_eq!(v[0], "5");
        // %.7g of 53.13010235... -> "53.1301" (trailing zero trimmed by %g).
        assert_eq!(v[1], "53.1301");
    }

    #[test]
    fn polar_angle_on_axes() {
        let p = PositionInfo::polar();
        // (1, 0) -> radius 1, angle 0.
        let v = p.values(Some([1.0, 0.0]));
        assert_eq!(v[0], "1");
        assert_eq!(v[1], "0");
        // (0, 1) -> angle 90.
        let v = p.values(Some([0.0, 1.0]));
        assert_eq!(v[1], "90");
    }

    #[test]
    fn custom_numeric_converter() {
        let mut p = PositionInfo::new(vec![]);
        p.push_numeric("Sum", |x, y| x + y);
        assert!(!p.is_empty());
        let v = p.values(Some([2.5, 1.5]));
        assert_eq!(v, vec!["4".to_owned()]);
    }

    #[test]
    fn custom_string_converter() {
        let p = PositionInfo::new(vec![(
            "Quad".to_owned(),
            Box::new(|x: f64, y: f64| {
                if x >= 0.0 && y >= 0.0 {
                    "I".to_owned()
                } else {
                    "other".to_owned()
                }
            }),
        )]);
        assert_eq!(p.values(Some([1.0, 1.0])), vec!["I".to_owned()]);
        assert_eq!(p.values(Some([-1.0, 1.0])), vec!["other".to_owned()]);
    }

    #[test]
    fn format_value_seven_sig_figs() {
        // %.7g of 1/3.
        assert_eq!(format_value(1.0 / 3.0), "0.3333333");
        assert_eq!(format_value(1234567.0), "1234567");
        // 8 sig figs collapses to exponential under %g (exp >= 7).
        assert_eq!(format_value(12345678.0), "1.234568e+07");
    }

    #[test]
    fn format_value_non_finite() {
        assert_eq!(format_value(f64::NAN), "nan");
        assert_eq!(format_value(f64::INFINITY), "inf");
        assert_eq!(format_value(f64::NEG_INFINITY), "-inf");
    }
}
