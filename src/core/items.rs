//! Shared item vocabulary used by both the GPU data layer and the egui overlay
//! layer: currently the line stroke style ([`LineStyle`]).
//!
//! These types live in `core` (not `render`) so the `core::Plot` model — which
//! stores overlay items (markers, shapes) carrying a [`LineStyle`] — can name
//! them without `core` depending on `render` (`doc/design.md` §9 `core/items.rs`).

/// Line stroke style (silx `linestyle`). Dash lengths for the predefined styles
/// scale with the line width (`max(width, 1)`) so they stay proportionate at any
/// thickness; a [`LineStyle::Custom`] pattern is taken verbatim. The dash unit is
/// physical pixels on the GPU curve path and logical points on the egui painter
/// overlay path.
#[derive(Clone, Debug, PartialEq)]
pub enum LineStyle {
    /// No line drawn (markers only, if any). silx `' '` / `''`.
    None,
    /// Continuous line. silx `'-'`.
    Solid,
    /// Dashed line. silx `'--'`.
    Dashed,
    /// Dash-dot line. silx `'-.'`.
    DashDot,
    /// Dotted line. silx `':'`.
    Dotted,
    /// Custom dash pattern: alternating on/off lengths (`on, off, on, off`), with
    /// `offset` the starting phase. silx `(offset, (dash pattern))`.
    Custom { offset: f32, pattern: Vec<f32> },
}

impl LineStyle {
    /// Whether this style draws a line at all (false only for [`LineStyle::None`]).
    pub(crate) fn draws_line(&self) -> bool {
        !matches!(self, LineStyle::None)
    }

    /// Dash and gap lengths plus the phase offset for egui's
    /// [`egui::Shape::dashed_line_with_offset`], or `None` for a solid (un-dashed)
    /// line. This is the painter-overlay counterpart of the GPU curve's
    /// `dash_spec`: the same proportions, expressed as the dash/gap arrays egui's
    /// dashed-line builder consumes (lengths in logical points). Predefined
    /// patterns scale with `max(width, 1)` so they look right at any thickness.
    pub(crate) fn painter_dashes(&self, width: f32) -> Option<(Vec<f32>, Vec<f32>, f32)> {
        let u = width.max(1.0);
        match self {
            LineStyle::None | LineStyle::Solid => None,
            // on, off
            LineStyle::Dashed => Some((vec![5.0 * u], vec![4.0 * u], 0.0)),
            // dot, gap
            LineStyle::Dotted => Some((vec![1.5 * u], vec![2.5 * u], 0.0)),
            // dash, gap, dot, gap
            LineStyle::DashDot => Some((vec![6.0 * u, 1.5 * u], vec![3.0 * u, 3.0 * u], 0.0)),
            LineStyle::Custom { offset, pattern } => {
                // pattern = [on, off, on, off, ...]: dashes are the even indices,
                // gaps the odd ones. egui cycles each array independently.
                let dashes: Vec<f32> = pattern.iter().step_by(2).copied().collect();
                let gaps: Vec<f32> = pattern.iter().skip(1).step_by(2).copied().collect();
                // A pattern with no gap (or a zero-length period) is just a solid
                // line: leave it un-dashed so the modulo stays well-defined.
                let period: f32 = dashes.iter().chain(&gaps).sum();
                if dashes.is_empty() || gaps.is_empty() || period <= 0.0 {
                    None
                } else {
                    Some((dashes, gaps, *offset))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_line_false_only_for_none() {
        assert!(!LineStyle::None.draws_line());
        assert!(LineStyle::Solid.draws_line());
        assert!(LineStyle::Dashed.draws_line());
        assert!(LineStyle::DashDot.draws_line());
        assert!(LineStyle::Dotted.draws_line());
    }

    #[test]
    fn painter_dashes_solid_and_none_are_undashed() {
        assert_eq!(LineStyle::Solid.painter_dashes(1.0), None);
        assert_eq!(LineStyle::None.painter_dashes(1.0), None);
    }

    #[test]
    fn painter_dashes_predefined_scale_with_width() {
        // Dashed at width 1: on 5, off 4.
        assert_eq!(
            LineStyle::Dashed.painter_dashes(1.0),
            Some((vec![5.0], vec![4.0], 0.0))
        );
        // Width 2 doubles the unit.
        assert_eq!(
            LineStyle::Dashed.painter_dashes(2.0),
            Some((vec![10.0], vec![8.0], 0.0))
        );
        // Dash-dot: dashes [6, 1.5], gaps [3, 3].
        assert_eq!(
            LineStyle::DashDot.painter_dashes(1.0),
            Some((vec![6.0, 1.5], vec![3.0, 3.0], 0.0))
        );
    }

    #[test]
    fn painter_dashes_custom_splits_on_off_and_keeps_offset() {
        let style = LineStyle::Custom {
            offset: 2.0,
            pattern: vec![3.0, 1.0, 2.0, 4.0],
        };
        assert_eq!(
            style.painter_dashes(1.0),
            Some((vec![3.0, 2.0], vec![1.0, 4.0], 2.0))
        );
        // A dash with no gap is solid (no usable period).
        let no_gap = LineStyle::Custom {
            offset: 0.0,
            pattern: vec![3.0],
        };
        assert_eq!(no_gap.painter_dashes(1.0), None);
        // An empty pattern is solid.
        let empty = LineStyle::Custom {
            offset: 0.0,
            pattern: vec![],
        };
        assert_eq!(empty.painter_dashes(1.0), None);
    }
}
