//! CPU pick traversal for `SceneWidget::pick` (plot3d PK2): a click is
//! unprojected into a ray (no GPU readback) and intersected with the scene's
//! data geometry. These tests are render-free — they build a `SceneWidget`
//! (which needs a headless wgpu `RenderState` only to install the scene
//! resources), set a known triangle / point, frame a fixed viewpoint, and assert
//! the pick lands where the geometry is.

use egui_kittest::wgpu::{create_render_state, default_wgpu_setup};
use siplot::egui::Color32;
use siplot::{CameraFace, PointMarker, Scene3dGeometry, ScenePickKind, SceneWidget};

/// Frame a unit-box scene from the Front viewpoint, with the camera sized
/// square so the centre-screen ray is unambiguous. `pick` reads the widget's own
/// CPU geometry + camera, so the `RenderState` (only needed to install the scene
/// resources at construction) need not outlive this helper.
fn front_view_widget(id: u64, geometry: Scene3dGeometry) -> SceneWidget {
    let rs = create_render_state(default_wgpu_setup());
    let mut w = SceneWidget::new(&rs, id);
    w.set_geometry(&rs, geometry);
    w.set_viewpoint(CameraFace::Front); // look along -Z through the box centre
    w.camera_mut().set_size((200.0, 200.0));
    w
}

#[test]
fn pick_hits_surface_under_screen_centre() {
    // A triangle in the z = 0.5 plane covering the box centre (0.5, 0.5).
    let mut geo = Scene3dGeometry::new();
    geo.add_triangle(
        [0.0, 0.0, 0.5],
        [1.0, 0.0, 0.5],
        [0.5, 1.0, 0.5],
        Color32::WHITE,
    );
    let w = front_view_widget(21, geo);

    let pick = w.pick((0.0, 0.0)).expect("centre ray hits the triangle");
    assert_eq!(pick.kind, ScenePickKind::Surface);
    // The hit lies on the z = 0.5 plane near the box centre.
    assert!(
        (pick.position.z - 0.5).abs() < 1e-3,
        "hit z = {} (want 0.5)",
        pick.position.z
    );
    assert!(
        (pick.position.x - 0.5).abs() < 0.1,
        "hit x = {}",
        pick.position.x
    );
    assert!(
        (pick.position.y - 0.5).abs() < 0.25,
        "hit y = {}",
        pick.position.y
    );
}

#[test]
fn pick_misses_when_ray_clears_the_geometry() {
    // A small triangle near one corner; the centre ray does not cross it and
    // there are no points, so the pick is empty.
    let mut geo = Scene3dGeometry::new();
    geo.add_triangle(
        [0.0, 0.0, 0.5],
        [0.1, 0.0, 0.5],
        [0.0, 0.1, 0.5],
        Color32::WHITE,
    );
    let w = front_view_widget(22, geo);
    assert!(
        w.pick((0.0, 0.0)).is_none(),
        "centre ray must miss the corner triangle"
    );
}

#[test]
fn pick_selects_scatter_point_under_the_cursor() {
    // A single scatter point at the box centre; the centre ray hits it.
    let mut geo = Scene3dGeometry::new();
    geo.add_point([0.5, 0.5, 0.5], Color32::WHITE, 12.0, PointMarker::Circle);
    let w = front_view_widget(23, geo);

    let pick = w.pick((0.0, 0.0)).expect("centre ray hits the point");
    assert_eq!(pick.kind, ScenePickKind::Point { index: 0 });
    assert!((pick.position.x - 0.5).abs() < 1e-6);
    assert!((pick.position.y - 0.5).abs() < 1e-6);
    assert!((pick.position.z - 0.5).abs() < 1e-6);
}

#[test]
fn pick_prefers_the_nearer_surface() {
    // Two centre-covering triangles at z = 0.2 and z = 0.8. From the Front view
    // (camera on the +Z side looking toward -Z) the z = 0.8 plane is nearer, so
    // it must win.
    let mut geo = Scene3dGeometry::new();
    geo.add_triangle(
        [0.0, 0.0, 0.2],
        [1.0, 0.0, 0.2],
        [0.5, 1.0, 0.2],
        Color32::WHITE,
    );
    geo.add_triangle(
        [0.0, 0.0, 0.8],
        [1.0, 0.0, 0.8],
        [0.5, 1.0, 0.8],
        Color32::WHITE,
    );
    let w = front_view_widget(24, geo);

    let pick = w.pick((0.0, 0.0)).expect("centre ray hits a triangle");
    assert_eq!(pick.kind, ScenePickKind::Surface);
    assert!(
        (pick.position.z - 0.8).abs() < 1e-2,
        "nearer plane (z=0.8) should win, got z = {}",
        pick.position.z
    );
}
