//! Headless end-to-end check for `SceneWidget` (plot3d P0.3): the widget frames
//! a scene, generates the bounding-box + RGB-axes chrome, uploads it, and paints
//! it through the offscreen-render-then-blit path; and a left-drag orbits the
//! camera so the rendered image changes.
//!
//! This exercises the whole widget wiring — geometry build, camera framing, GPU
//! upload, paint callback, and pointer interaction — on a real (or software) GPU
//! via `egui_kittest`, mirroring `tests/mask_pointer_offset.rs`. The pure camera
//! math is unit-tested separately in `core::scene3d`.

use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use siplot::egui;
use siplot::egui_wgpu::RenderState;
use siplot::{SceneWidget, Vec3};
use std::cell::RefCell;
use std::rc::Rc;

const WIN: f32 = 320.0;

struct App {
    scene: SceneWidget,
    last_rect: Option<egui::Rect>,
}

impl App {
    fn new(rs: &RenderState) -> Self {
        let mut scene = SceneWidget::new(rs, 0);
        // A unit box centred at the origin: distinct positive axis directions.
        scene.set_bounds(rs, (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)));
        Self {
            scene,
            last_rect: None,
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let resp = self.scene.show(ui);
        self.last_rect = Some(resp.rect);
    }
}

fn classify(raw: &[u8], iw: usize, ih: usize) -> (usize, usize, usize, usize) {
    let (mut red, mut green, mut blue, mut bg) = (0usize, 0usize, 0usize, 0usize);
    for px in 0..(iw * ih) {
        let i = px * 4;
        let (r, g, b, a) = (raw[i], raw[i + 1], raw[i + 2], raw[i + 3]);
        if r > 120 && g < 80 && b < 80 {
            red += 1;
        } else if g > 120 && r < 80 && b < 80 {
            green += 1;
        } else if b > 120 && r < 80 && g < 80 {
            blue += 1;
        } else if a == 255 && r < 60 && g < 60 && b < 60 && r.abs_diff(g).max(g.abs_diff(b)) <= 12 {
            // The scene background: opaque, dark, neutral grey. Tested by these
            // properties rather than an exact byte so the check holds whether the
            // target format is linear (byte ~3) or sRGB (byte ~30). The un-painted
            // border outside the scene rect is transparent (a == 0) and excluded.
            bg += 1;
        }
    }
    (red, green, blue, bg)
}

#[test]
fn scene_widget_renders_axes_and_orbits_on_drag() {
    let rs = create_render_state(default_wgpu_setup());
    let app = Rc::new(RefCell::new(App::new(&rs)));
    let renderer = WgpuTestRenderer::from_render_state(rs);

    let app_ui = app.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(WIN, WIN))
        .with_pixels_per_point(1.0)
        .renderer(renderer)
        .build_ui(move |ui| app_ui.borrow_mut().ui(ui));

    // Frame 1: the default side-view scene (no pointer → no cursor overlay).
    harness.step();
    let rect = app.borrow().last_rect.expect("scene rect");
    let image1 = harness.render().expect("headless wgpu render");
    let (iw, ih) = (image1.width() as usize, image1.height() as usize);
    let frame1 = image1.as_raw().clone();

    let (red, green, blue, bg) = classify(&frame1, iw, ih);
    // All three RGB axes must be visible.
    assert!(red > 0, "X axis (red) not visible");
    assert!(green > 0, "Y axis (green) not visible");
    assert!(blue > 0, "Z axis (blue) not visible");
    // The dark background must dominate the frame (the scene is a wireframe).
    assert!(
        bg > iw * ih / 2,
        "dark background should dominate; got {bg}/{} px",
        iw * ih
    );

    // Orbit: press at the centre and drag a quarter-width to the right. egui only
    // recognises a drag once the pointer clears its click-vs-drag threshold, so we
    // move in several steps (mirrors tests/mask_pointer_offset.rs).
    let a = rect.center();
    let b = a + egui::vec2(80.0, 0.0);
    harness.hover_at(a);
    harness.drag_at(a);
    harness.step(); // press
    for k in 1..=4 {
        harness.hover_at(a + (b - a) * (k as f32 / 4.0));
        harness.step();
    }
    harness.drop_at(b); // release; also removes the cursor overlay
    harness.step();

    let image2 = harness.render().expect("headless wgpu render");
    let frame2 = image2.as_raw();

    // The orbit must have changed the rendered view.
    let changed = frame1
        .iter()
        .zip(frame2.iter())
        .filter(|(x, y)| x.abs_diff(**y) > 30)
        .count();
    assert!(
        changed > 200,
        "left-drag orbit should change the rendered image; only {changed} byte diffs"
    );
}
