//! Headless wgpu readback proving a scalar image's **per-pixel alpha map**
//! (silx `ImageData.setAlphaData`, `ImageSpec::with_alpha_map`) is sampled per
//! pixel by the image shader, not as a single global alpha.
//!
//! A uniform scalar field (every texel maps through viridis to the same
//! saturated teal) fills the `x∈[0,20] y∈[0,20]` view. The alpha map is opaque
//! (`1.0`) for the left half of the columns (`col < width/2`) and fully
//! transparent (`0.0`) for the right half. Against the white data background the
//! left half must therefore stay teal while the right half composites away to
//! background — and a control render with NO alpha map must show teal on BOTH
//! halves, proving the right-half disappearance is the alpha map at work and not
//! the image being absent there.
//!
//! The discriminator is a per-column count of teal pixels (green-dominant and
//! saturated — distinct from the white background, black axes/text, and grey
//! grid, so chrome never registers), summed over the left third vs the right
//! third of the frame. Margin-independent, mirroring `tests/roi_band_unbounded_render.rs`.
//! Needs a GPU (real or software).

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use siplot::egui;
use siplot::{Colormap, ImageSpec, PlotWidget, YAxis};

const W: usize = 400;
const H: usize = 300;

// Image grid (data extent x∈[0,WD] y∈[0,HD], origin (0,0), scale (1,1)).
const WD: u32 = 20;
const HD: u32 = 20;

/// A green-dominant, saturated pixel: the colormapped teal of the image. White
/// background, black axes/text and grey grid are all unsaturated or not
/// green-dominant, so only the image registers.
fn is_teal(px: &[u8]) -> bool {
    let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    g >= r && g >= b && (mx - mn) > 60
}

/// `(teal pixels in the left third of the frame, teal pixels in the right third)`.
fn teal_left_right(raw: &[u8]) -> (u32, u32) {
    let (mut left, mut right) = (0u32, 0u32);
    for (i, px) in raw.chunks_exact(4).enumerate() {
        if is_teal(px) {
            let col = i % W;
            if col < W / 3 {
                left += 1;
            } else if col >= 2 * W / 3 {
                right += 1;
            }
        }
    }
    (left, right)
}

/// Render the uniform teal image filling the pinned view, optionally with a
/// per-pixel alpha map, and return the `(left-third, right-third)` teal counts.
fn render(alpha_map: Option<Vec<f32>>) -> (u32, u32) {
    let rs = create_render_state(default_wgpu_setup());
    siplot::install(&rs);

    let mut plot = PlotWidget::new(&rs, 0);

    // Uniform field: every texel = 0.5, viridis over [0,1] → the same mid teal
    // for the whole image, so any left/right difference is the alpha map alone.
    let data = vec![0.5f32; (WD * HD) as usize];
    let colormap = Colormap::viridis(0.0, 1.0);
    let mut spec = ImageSpec::scalar(WD, HD, &data, colormap);
    if let Some(am) = alpha_map.as_ref() {
        spec = spec.with_alpha_map(am);
    }
    plot.add_image_spec(spec);

    // Pin the view to the image extent so the image fills the data area, and
    // drop the colormap colorbar — its viridis ramp would otherwise paint teal
    // in the right margin (chrome, alpha-independent) and pollute the count.
    plot.set_show_colorbar(false);
    plot.set_auto_reset_zoom(false);
    plot.set_graph_x_limits(0.0, WD as f64);
    plot.set_graph_y_limits(0.0, HD as f64, YAxis::Left);

    let app = Rc::new(RefCell::new(plot));
    let app_ui = app.clone();
    let renderer = WgpuTestRenderer::from_render_state(rs);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(W as f32, H as f32))
        .with_pixels_per_point(1.0)
        .renderer(renderer)
        .build_ui(move |ui| {
            app_ui.borrow_mut().show(ui);
        });

    harness.step();
    harness.step();
    let image = harness.render().expect("headless wgpu render");
    teal_left_right(image.as_raw())
}

#[test]
fn per_pixel_alpha_map_makes_only_the_masked_half_transparent() {
    // Opaque (col < WD/2) on the left, fully transparent on the right.
    let alpha_map: Vec<f32> = (0..(WD * HD))
        .map(|i| {
            let col = i % WD;
            if col < WD / 2 { 1.0 } else { 0.0 }
        })
        .collect();

    let (masked_left, masked_right) = render(Some(alpha_map));
    let (control_left, control_right) = render(None);

    // Control (no alpha map): the uniform image is opaque on BOTH halves.
    assert!(
        control_left > 200 && control_right > 200,
        "without an alpha map the uniform image must be teal on both halves: \
         left={control_left} right={control_right}"
    );

    // Masked: the left half (alpha=1) stays teal, matching the control's left.
    assert!(
        masked_left > 200,
        "the opaque (alpha=1) left half must stay teal: masked_left={masked_left}"
    );

    // Masked: the right half (alpha=0) composites away to background — no teal.
    assert_eq!(
        masked_right, 0,
        "the transparent (alpha=0) right half must show no teal (was {control_right} \
         when opaque): masked_right={masked_right}"
    );
}
