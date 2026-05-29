//! Triangles: a per-vertex-colored filled mesh drawn over the data area (silx
//! `BackendBase.addTriangles`).
//!
//! Unlike the wgpu data layer (image, curve), the mesh is drawn with egui's
//! painter: each vertex is transformed data→pixel on the CPU via the shared
//! [`Transform`] (so log / inverted / aspect-ratio axes come for free) and handed
//! to an [`egui::epaint::Mesh`] with its own per-vertex color. The build is pure
//! and unit-testable; the widget's chrome submits the mesh each frame via
//! [`crate::widget::chrome::draw_triangles`]. This mirrors silx's matplotlib
//! backend, which also rasterizes triangles on the CPU (`doc/design.md` §8).

use egui::Color32;
use egui::epaint::Mesh;

use crate::core::transform::Transform;

/// A set of filled triangles with per-vertex color (silx `addTriangles`).
///
/// `indices` are triangles into the shared `x`/`y`/`colors` vertex arrays, three
/// vertices each. `alpha` is a global opacity in `[0, 1]` multiplied into every
/// vertex color (silx `alpha`).
#[derive(Clone, Debug, PartialEq)]
pub struct Triangles {
    /// Per-vertex data X coordinates.
    pub x: Vec<f64>,
    /// Per-vertex data Y coordinates (same length as `x`).
    pub y: Vec<f64>,
    /// Triangle vertex indices into `x`/`y`/`colors`, three per triangle.
    pub indices: Vec<[u32; 3]>,
    /// Per-vertex RGBA color (same length as `x`).
    pub colors: Vec<Color32>,
    /// Global opacity multiplier in `[0, 1]` (silx `alpha`).
    pub alpha: f32,
}

impl Triangles {
    /// Build a triangle set. Panics if `y`/`colors` do not match `x` in length,
    /// or if any index is out of range — the same invariants silx requires of its
    /// `(Npoint,…)` / `(Ntriangle, 3)` arrays.
    pub fn new(x: Vec<f64>, y: Vec<f64>, indices: Vec<[u32; 3]>, colors: Vec<Color32>) -> Self {
        assert_eq!(x.len(), y.len(), "x and y must have the same length");
        assert_eq!(
            colors.len(),
            x.len(),
            "colors must have one entry per vertex"
        );
        let n = u32::try_from(x.len()).expect("vertex count fits in u32");
        assert!(
            indices.iter().flatten().all(|&i| i < n),
            "triangle index out of range"
        );
        Self {
            x,
            y,
            indices,
            colors,
            alpha: 1.0,
        }
    }

    /// Set the global opacity (silx `alpha`), clamped to `[0, 1]`.
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// Build the screen-space [`egui::epaint::Mesh`] for `t`: every vertex
    /// transformed data→pixel (honoring log / inverted / aspect via the shared
    /// transform) with the global `alpha` multiplied into its color.
    pub fn mesh(&self, t: &Transform) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.reserve_vertices(self.x.len());
        mesh.reserve_triangles(self.indices.len());
        for ((&x, &y), &color) in self.x.iter().zip(&self.y).zip(&self.colors) {
            mesh.colored_vertex(t.data_to_pixel(x, y), color.gamma_multiply(self.alpha));
        }
        for tri in &self.indices {
            mesh.add_triangle(tri[0], tri[1], tri[2]);
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};

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

    fn unit_triangle() -> Triangles {
        Triangles::new(
            vec![0.0, 10.0, 0.0],
            vec![0.0, 0.0, 10.0],
            vec![[0, 1, 2]],
            vec![Color32::RED, Color32::GREEN, Color32::BLUE],
        )
    }

    #[test]
    fn default_alpha_is_one() {
        assert_eq!(unit_triangle().alpha, 1.0);
    }

    #[test]
    fn with_alpha_clamps() {
        assert_eq!(unit_triangle().with_alpha(0.25).alpha, 0.25);
        assert_eq!(unit_triangle().with_alpha(-1.0).alpha, 0.0);
        assert_eq!(unit_triangle().with_alpha(2.0).alpha, 1.0);
    }

    #[test]
    #[should_panic(expected = "x and y must have the same length")]
    fn new_rejects_xy_length_mismatch() {
        Triangles::new(vec![0.0, 1.0], vec![0.0], vec![], vec![Color32::RED; 2]);
    }

    #[test]
    #[should_panic(expected = "colors must have one entry per vertex")]
    fn new_rejects_color_length_mismatch() {
        Triangles::new(vec![0.0], vec![0.0], vec![], vec![Color32::RED; 2]);
    }

    #[test]
    #[should_panic(expected = "triangle index out of range")]
    fn new_rejects_out_of_range_index() {
        Triangles::new(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0, 2.0],
            vec![[0, 1, 3]], // 3 is out of range for 3 vertices
            vec![Color32::RED; 3],
        );
    }

    #[test]
    fn mesh_maps_vertices_and_indices() {
        let m = unit_triangle().mesh(&t());
        assert_eq!(m.vertices.len(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        // (0,0)->(0,100) bottom-left, (10,0)->(100,100), (0,10)->(0,0) top-left.
        assert_eq!(m.vertices[0].pos, pos2(0.0, 100.0));
        assert_eq!(m.vertices[1].pos, pos2(100.0, 100.0));
        assert_eq!(m.vertices[2].pos, pos2(0.0, 0.0));
        // alpha 1.0 leaves opaque colors intact.
        assert_eq!(m.vertices[0].color, Color32::RED);
    }

    #[test]
    fn mesh_applies_global_alpha_to_vertex_colors() {
        let m = unit_triangle().with_alpha(0.5).mesh(&t());
        // gamma_multiply(0.5) on a premultiplied opaque color: 255*0.5+0.5 -> 128.
        assert_eq!(m.vertices[0].color, Color32::RED.gamma_multiply(0.5));
        assert_eq!(m.vertices[0].color.a(), 128);
    }
}
