//! Regions of interest (ROIs): rectangular, horizontal-band, and vertical-band
//! selections drawn over the data area with draggable edge handles.
//!
//! The geometry is data-space and the hit-testing / edge-move math is pure (no
//! egui input), so it is unit-testable; the widget wires pointer drags to
//! [`Roi::edge_at`] and [`Roi::move_edge`] and emits a change when an edge moves
//! (silx `RegionOfInterest`, `doc/design.md` §13 C3).

use egui::{Pos2, Rect};

use crate::core::transform::Transform;

/// A draggable edge of an ROI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoiEdge {
    /// Data `x` minimum (left).
    Left,
    /// Data `x` maximum (right).
    Right,
    /// Data `y` minimum (bottom of the data area).
    Bottom,
    /// Data `y` maximum (top of the data area).
    Top,
    /// Generic vertex handle at `index`; used by [`Roi::Point`], [`Roi::Line`],
    /// and [`Roi::Polygon`] variants.
    Vertex(usize),
}

/// A region of interest in data coordinates. Bounds are kept normalized
/// (`min ≤ max`) by [`Roi::move_edge`].
#[derive(Clone, Debug, PartialEq)]
pub enum Roi {
    /// Axis-aligned rectangle `x = (x_min, x_max)`, `y = (y_min, y_max)`.
    Rect { x: (f64, f64), y: (f64, f64) },
    /// Horizontal band `y = (y_min, y_max)` spanning the full X extent.
    HRange { y: (f64, f64) },
    /// Vertical band `x = (x_min, x_max)` spanning the full Y extent.
    VRange { x: (f64, f64) },
    /// Single movable point.
    Point { x: f64, y: f64 },
    /// Line segment between two movable endpoints.
    Line { start: (f64, f64), end: (f64, f64) },
    /// Polygon with N movable vertices (requires at least 1 vertex; 0-vertex is a no-op for drawing).
    Polygon { vertices: Vec<(f64, f64)> },
}

impl Roi {
    /// The screen rectangle this ROI draws into. Bands span the data area on
    /// their free axis.
    pub fn screen_rect(&self, t: &Transform) -> Rect {
        let area = t.area;
        match self {
            Roi::Rect { x, y } => {
                let a = t.data_to_pixel(x.0, y.0);
                let b = t.data_to_pixel(x.1, y.1);
                Rect::from_two_pos(a, b)
            }
            Roi::HRange { y } => {
                let py0 = t.data_to_pixel(t.x.min, y.0).y;
                let py1 = t.data_to_pixel(t.x.min, y.1).y;
                Rect::from_x_y_ranges(area.left()..=area.right(), py0.min(py1)..=py0.max(py1))
            }
            Roi::VRange { x } => {
                let px0 = t.data_to_pixel(x.0, t.y.min).x;
                let px1 = t.data_to_pixel(x.1, t.y.min).x;
                Rect::from_x_y_ranges(px0.min(px1)..=px0.max(px1), area.top()..=area.bottom())
            }
            Roi::Point { x, y } => {
                let p = t.data_to_pixel(*x, *y);
                Rect::from_center_size(p, egui::vec2(1.0, 1.0))
            }
            Roi::Line { start, end } => {
                let a = t.data_to_pixel(start.0, start.1);
                let b = t.data_to_pixel(end.0, end.1);
                Rect::from_two_pos(a, b)
            }
            Roi::Polygon { vertices } => {
                let mut rect = Rect::NOTHING;
                for &(x, y) in vertices {
                    let p = t.data_to_pixel(x, y);
                    if rect.is_negative() {
                        rect = Rect::from_center_size(p, egui::vec2(1.0, 1.0));
                    } else {
                        rect = rect.union(Rect::from_center_size(p, egui::vec2(1.0, 1.0)));
                    }
                }
                if rect.is_negative() { area } else { rect }
            }
        }
    }

    /// The draggable edges this ROI exposes.
    fn edges(&self) -> Vec<RoiEdge> {
        match self {
            Roi::Rect { .. } => vec![RoiEdge::Left, RoiEdge::Right, RoiEdge::Bottom, RoiEdge::Top],
            Roi::HRange { .. } => vec![RoiEdge::Bottom, RoiEdge::Top],
            Roi::VRange { .. } => vec![RoiEdge::Left, RoiEdge::Right],
            Roi::Point { .. } => vec![RoiEdge::Vertex(0)],
            Roi::Line { .. } => vec![RoiEdge::Vertex(0), RoiEdge::Vertex(1)],
            Roi::Polygon { vertices } => (0..vertices.len()).map(RoiEdge::Vertex).collect(),
        }
    }

    /// Screen-space position of vertex `index` for Point/Line/Polygon ROIs.
    fn vertex_pixel(&self, t: &Transform, index: usize) -> Option<Pos2> {
        let (x, y) = match self {
            Roi::Point { x, y } if index == 0 => (*x, *y),
            Roi::Line { start, end } => match index {
                0 => *start,
                1 => *end,
                _ => return None,
            },
            Roi::Polygon { vertices } => vertices.get(index).copied()?,
            _ => return None,
        };
        Some(t.data_to_pixel(x, y))
    }

    /// Screen-space midpoints of this ROI's draggable edges, for drawing handle
    /// marks (one per edge, in [`Roi::edges`] order).
    pub fn handle_centers(&self, t: &Transform) -> Vec<Pos2> {
        let r = self.screen_rect(t);
        self.edges()
            .iter()
            .map(|edge| match edge {
                RoiEdge::Left => egui::pos2(r.left(), r.center().y),
                RoiEdge::Right => egui::pos2(r.right(), r.center().y),
                RoiEdge::Top => egui::pos2(r.center().x, r.top()),
                RoiEdge::Bottom => egui::pos2(r.center().x, r.bottom()),
                RoiEdge::Vertex(n) => self.vertex_pixel(t, *n).unwrap_or(r.center()),
            })
            .collect()
    }

    /// The edge under `cursor` (screen pixels) within `grab_px`, or `None`.
    /// When several edges are in range, the perpendicularly-closest one wins.
    pub fn edge_at(&self, t: &Transform, cursor: Pos2, grab_px: f32) -> Option<RoiEdge> {
        match self {
            Roi::Point { .. } | Roi::Line { .. } | Roi::Polygon { .. } => {
                let mut best: Option<(RoiEdge, f32)> = None;
                for edge in self.edges() {
                    if let RoiEdge::Vertex(n) = edge
                        && let Some(p) = self.vertex_pixel(t, n)
                    {
                        let dist = cursor.distance(p);
                        if dist <= grab_px && best.is_none_or(|(_, d)| dist < d) {
                            best = Some((edge, dist));
                        }
                    }
                }
                best.map(|(e, _)| e)
            }
            _ => {
                // Rect, HRange, VRange: existing rect-based edge detection.
                let r = self.screen_rect(t);
                let mut best: Option<(RoiEdge, f32)> = None;
                for edge in self.edges() {
                    let dist = match edge {
                        // Vertical edges: cursor must be within the rect's y span.
                        RoiEdge::Left | RoiEdge::Right => {
                            if cursor.y < r.top() - grab_px || cursor.y > r.bottom() + grab_px {
                                continue;
                            }
                            let ex = if edge == RoiEdge::Left {
                                r.left()
                            } else {
                                r.right()
                            };
                            (cursor.x - ex).abs()
                        }
                        // Horizontal edges: cursor must be within the rect's x span.
                        RoiEdge::Bottom | RoiEdge::Top => {
                            if cursor.x < r.left() - grab_px || cursor.x > r.right() + grab_px {
                                continue;
                            }
                            // Top edge = data y.max = screen top (smaller y).
                            let ey = if edge == RoiEdge::Top {
                                r.top()
                            } else {
                                r.bottom()
                            };
                            (cursor.y - ey).abs()
                        }
                        RoiEdge::Vertex(_) => continue,
                    };
                    if dist <= grab_px && best.is_none_or(|(_, d)| dist < d) {
                        best = Some((edge, dist));
                    }
                }
                best.map(|(edge, _)| edge)
            }
        }
    }

    /// Move `edge` to the data point `data = (x, y)`, clamping so the ROI stays
    /// normalized (`min ≤ max`). Edges that do not apply to this ROI kind are
    /// ignored.
    pub fn move_edge(&mut self, edge: RoiEdge, data: (f64, f64)) {
        let (dx, dy) = data;
        match self {
            Roi::Rect { x, y } => match edge {
                RoiEdge::Left => x.0 = dx.min(x.1),
                RoiEdge::Right => x.1 = dx.max(x.0),
                RoiEdge::Bottom => y.0 = dy.min(y.1),
                RoiEdge::Top => y.1 = dy.max(y.0),
                RoiEdge::Vertex(_) => {}
            },
            Roi::HRange { y } => match edge {
                RoiEdge::Bottom => y.0 = dy.min(y.1),
                RoiEdge::Top => y.1 = dy.max(y.0),
                _ => {}
            },
            Roi::VRange { x } => match edge {
                RoiEdge::Left => x.0 = dx.min(x.1),
                RoiEdge::Right => x.1 = dx.max(x.0),
                _ => {}
            },
            Roi::Point { x, y } => {
                if let RoiEdge::Vertex(0) = edge {
                    *x = dx;
                    *y = dy;
                }
            }
            Roi::Line { start, end } => match edge {
                RoiEdge::Vertex(0) => *start = (dx, dy),
                RoiEdge::Vertex(1) => *end = (dx, dy),
                _ => {}
            },
            Roi::Polygon { vertices } => {
                if let RoiEdge::Vertex(n) = edge
                    && let Some(v) = vertices.get_mut(n)
                {
                    *v = (dx, dy);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    // 100×100 px area mapping data [0,10]×[0,10]; 1 data unit = 10 px, y flipped.
    fn t() -> Transform {
        Transform::new(
            0.0,
            10.0,
            0.0,
            10.0,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
        )
    }

    #[test]
    fn rect_screen_rect_flips_y() {
        let roi = Roi::Rect {
            x: (2.0, 8.0),
            y: (3.0, 7.0),
        };
        let r = roi.screen_rect(&t());
        // x: 2->20, 8->80; y: data 3 (bottom) -> 70px, data 7 (top) -> 30px.
        assert!((r.left() - 20.0).abs() < 1e-3 && (r.right() - 80.0).abs() < 1e-3);
        assert!((r.top() - 30.0).abs() < 1e-3 && (r.bottom() - 70.0).abs() < 1e-3);
    }

    #[test]
    fn edge_at_grabs_nearest_edge() {
        let roi = Roi::Rect {
            x: (2.0, 8.0),
            y: (3.0, 7.0),
        };
        // Near the left edge (x≈20px), mid-height.
        assert_eq!(
            roi.edge_at(&t(), pos2(21.0, 50.0), 4.0),
            Some(RoiEdge::Left)
        );
        // Near the top edge (screen y≈30px).
        assert_eq!(roi.edge_at(&t(), pos2(50.0, 31.0), 4.0), Some(RoiEdge::Top));
        // Far from any edge -> None.
        assert_eq!(roi.edge_at(&t(), pos2(50.0, 50.0), 4.0), None);
    }

    #[test]
    fn hrange_only_exposes_horizontal_edges() {
        let roi = Roi::HRange { y: (3.0, 7.0) };
        // Anywhere along the bottom band edge (full-width) grabs Bottom.
        assert_eq!(
            roi.edge_at(&t(), pos2(5.0, 70.0), 4.0),
            Some(RoiEdge::Bottom)
        );
        // A vertical-edge probe finds nothing (no Left/Right on a band).
        assert_eq!(roi.edge_at(&t(), pos2(0.0, 50.0), 4.0), None);
    }

    #[test]
    fn move_edge_clamps_to_stay_normalized() {
        let mut roi = Roi::Rect {
            x: (2.0, 8.0),
            y: (3.0, 7.0),
        };
        // Drag the left edge past the right edge: it clamps at the right.
        roi.move_edge(RoiEdge::Left, (12.0, 5.0));
        assert_eq!(
            roi,
            Roi::Rect {
                x: (8.0, 8.0),
                y: (3.0, 7.0)
            }
        );
        // Normal move.
        roi.move_edge(RoiEdge::Right, (9.0, 5.0));
        assert_eq!(
            roi,
            Roi::Rect {
                x: (8.0, 9.0),
                y: (3.0, 7.0)
            }
        );
    }

    #[test]
    fn point_roi_vertex_handle_moves_it() {
        let mut roi = Roi::Point { x: 5.0, y: 5.0 };
        roi.move_edge(RoiEdge::Vertex(0), (3.0, 4.0));
        assert_eq!(roi, Roi::Point { x: 3.0, y: 4.0 });
    }

    #[test]
    fn line_roi_endpoints_move_independently() {
        let mut roi = Roi::Line {
            start: (0.0, 0.0),
            end: (10.0, 10.0),
        };
        roi.move_edge(RoiEdge::Vertex(0), (1.0, 2.0));
        roi.move_edge(RoiEdge::Vertex(1), (9.0, 8.0));
        assert_eq!(
            roi,
            Roi::Line {
                start: (1.0, 2.0),
                end: (9.0, 8.0)
            }
        );
    }

    #[test]
    fn polygon_vertex_move_updates_specific_vertex() {
        let mut roi = Roi::Polygon {
            vertices: vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)],
        };
        roi.move_edge(RoiEdge::Vertex(1), (6.0, 1.0));
        assert_eq!(
            roi,
            Roi::Polygon {
                vertices: vec![(0.0, 0.0), (6.0, 1.0), (5.0, 5.0)]
            }
        );
    }

    #[test]
    fn edge_at_finds_line_endpoint() {
        let roi = Roi::Line {
            start: (2.0, 5.0),
            end: (8.0, 5.0),
        };
        // start is at data (2,5) → pixel (20, 50); end at (8,5) → pixel (80, 50)
        assert_eq!(
            roi.edge_at(&t(), pos2(21.0, 50.0), 4.0),
            Some(RoiEdge::Vertex(0))
        );
        assert_eq!(
            roi.edge_at(&t(), pos2(79.0, 50.0), 4.0),
            Some(RoiEdge::Vertex(1))
        );
        assert_eq!(roi.edge_at(&t(), pos2(50.0, 50.0), 4.0), None); // mid-line, no handle
    }
}
