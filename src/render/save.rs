//! Save the current plot view to a PNG (silx `saveGraph`).
//!
//! [`save_graph`] renders the data layer (background clear, image, curves) for
//! the plot's current limits into an offscreen texture at a chosen pixel size,
//! copies it back to the CPU, and writes a PNG. This captures the wgpu-rendered
//! data layer; the egui-drawn chrome (axes, ticks, colorbar) is not included
//! (`doc/design.md` §13 E1).
//!
//! The readback's row stride is padded to `COPY_BYTES_PER_ROW_ALIGNMENT`, and
//! the bytes are converted to tightly packed RGBA8 (swapping channels when the
//! surface format is BGRA). The pure byte-layout and PNG-encoding helpers are
//! unit-tested; the GPU render + readback runs only with a real device.
//!
//! Beyond PNG, the raster snapshot can also be exported as PPM (P6) and SVG (a
//! base64 PNG `<image>`), mirroring silx `PlotImageFile.saveImageToFile`. The
//! `encode_*` helpers are pure functions over the RGBA pixels so they are
//! testable without a GPU or the filesystem.

use std::fmt;
use std::path::Path;

use egui::{Pos2, Rect};
use egui_wgpu::{RenderState, wgpu};

use crate::core::plot::Plot;
use crate::core::transform::Scale;
use crate::render::backend_wgpu::WgpuResources;

/// Why a [`save_graph`] call failed.
#[derive(Debug)]
pub enum SaveError {
    /// Writing the PNG file to disk failed.
    Io(std::io::Error),
    /// Encoding the pixels as PNG failed.
    Encode(png::EncodingError),
    /// The GPU readback (buffer map / device poll) failed, or the size was zero.
    Readback(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "save_graph: writing PNG: {e}"),
            SaveError::Encode(e) => write!(f, "save_graph: encoding PNG: {e}"),
            SaveError::Readback(e) => write!(f, "save_graph: GPU readback: {e}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::Io(e) => Some(e),
            SaveError::Encode(e) => Some(e),
            SaveError::Readback(_) => None,
        }
    }
}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

impl From<png::EncodingError> for SaveError {
    fn from(e: png::EncodingError) -> Self {
        SaveError::Encode(e)
    }
}

/// Row stride, in bytes, for an `width`-pixel RGBA8 row padded up to wgpu's
/// `COPY_BYTES_PER_ROW_ALIGNMENT` (the alignment `copy_texture_to_buffer`
/// requires).
pub(crate) fn padded_bytes_per_row(width: u32) -> u32 {
    let unpadded = 4 * width;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unpadded.div_ceil(align) * align
}

/// Convert a mapped readback buffer (rows padded to `bytes_per_row`) into a
/// tightly packed `width * height * 4` RGBA8 image, swapping R/B when the
/// surface format is BGRA so the output is always RGBA.
pub(crate) fn rows_to_rgba8(
    mapped: &[u8],
    width: u32,
    height: u32,
    bytes_per_row: u32,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let bpr = bytes_per_row as usize;
    let row_bytes = w * 4;
    let swap = matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );

    let mut out = vec![0u8; w * h * 4];
    for row in 0..h {
        let src = &mapped[row * bpr..row * bpr + row_bytes];
        let dst = &mut out[row * row_bytes..(row + 1) * row_bytes];
        if swap {
            for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        } else {
            dst.copy_from_slice(src);
        }
    }
    out
}

/// Encode tightly packed `width * height` RGBA8 pixels as a PNG byte stream.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(out)
}

/// Drop the alpha channel of a tightly packed `width * height` RGBA8 buffer,
/// returning tightly packed `width * height` RGB8 (3 bytes per pixel).
///
/// silx's raster image export (`PlotImageFile.saveImageToFile`) operates on an
/// `(h, w, 3)` RGB array; the PPM body carries RGB, so the readback's RGBA is
/// reduced to RGB by discarding alpha.
pub fn rgba_to_rgb(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let n = (width as usize) * (height as usize);
    let mut out = Vec::with_capacity(n * 3);
    for px in rgba.chunks_exact(4).take(n) {
        out.push(px[0]);
        out.push(px[1]);
        out.push(px[2]);
    }
    out
}

/// Encode tightly packed `width * height` RGBA8 pixels as a binary (P6) PPM
/// byte stream.
///
/// Faithful to silx `PlotImageFile.saveImageToFile` (`fileFormat == "ppm"`):
/// the header is `P6\n<width> <height>\n255\n` followed by raw RGB bytes (the
/// alpha channel is dropped). The header is ASCII and self-describing.
pub fn encode_ppm(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let rgb = rgba_to_rgb(rgba, width, height);
    let header = format!("P6\n{width} {height}\n255\n");
    let mut out = Vec::with_capacity(header.len() + rgb.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&rgb);
    out
}

/// Standard base64 alphabet (RFC 4648), used by [`encode_svg`].
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as standard (RFC 4648) base64 with `=` padding.
///
/// Implemented inline so the SVG export needs no external base64 crate
/// (mirrors silx using the stdlib `base64.b64encode`).
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Encode tightly packed `width * height` RGB8 pixels (3 bytes/pixel) as PNG.
fn encode_rgb_png(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb)?;
    }
    Ok(out)
}

/// Encode tightly packed `width * height` RGBA8 pixels as an SVG document that
/// embeds the raster as a base64 PNG `<image>`.
///
/// Faithful to silx `PlotImageFile.saveImageToFile` (`fileFormat == "svg"`):
/// the same XML declaration, SVG 1.1 DOCTYPE, root `<svg>` carrying `width`/
/// `height` in px, and a single `<image xlink:href="data:image/png;base64,…">`
/// placed at `x=0 y=0` with the same width/height and `id="image"`. silx tracks
/// no vector primitives in its raster path, so the rendered bitmap is embedded
/// rather than re-emitted as vector geometry (see Defer note in the module).
///
/// The embedded PNG is RGB (alpha dropped) to match silx's `(h, w, 3)` array.
pub fn encode_svg(rgba: &[u8], width: u32, height: u32) -> Result<String, png::EncodingError> {
    let rgb = rgba_to_rgb(rgba, width, height);
    let png = encode_rgb_png(&rgb, width, height)?;
    let b64 = base64_encode(&png);
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    s.push_str("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"\n");
    s.push_str("  \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n");
    s.push_str("<svg xmlns:xlink=\"http://www.w3.org/1999/xlink\"\n");
    s.push_str("     xmlns=\"http://www.w3.org/2000/svg\"\n");
    s.push_str("     version=\"1.1\"\n");
    s.push_str(&format!("     width=\"{width}\"\n"));
    s.push_str(&format!("     height=\"{height}\">\n"));
    s.push_str("    <image xlink:href=\"data:image/png;base64,");
    s.push_str(&b64);
    s.push_str("\"\n");
    s.push_str("           x=\"0\"\n");
    s.push_str("           y=\"0\"\n");
    s.push_str(&format!("           width=\"{width}\"\n"));
    s.push_str(&format!("           height=\"{height}\"\n"));
    s.push_str("           id=\"image\" />\n");
    s.push_str("</svg>");
    Ok(s)
}

/// Per-axis log flags `[x, y]` (1.0 = log10) for the shaders, matching a
/// transform's scales.
fn axis_log_flags(t: &crate::core::transform::Transform) -> [f32; 2] {
    [
        f32::from(t.x.scale == Scale::Log10),
        f32::from(t.y.scale == Scale::Log10),
    ]
}

/// Render the plot's current view to a `size = (width, height)` pixel PNG at
/// `path`. Captures the data layer (clear + image + curves); chrome is not
/// included. Requires [`crate::install`] to have run on `render_state`.
pub fn save_graph(
    render_state: &RenderState,
    plot: &Plot,
    size: (u32, u32),
    path: impl AsRef<Path>,
) -> Result<(), SaveError> {
    let (w, h) = size;
    if w == 0 || h == 0 {
        return Err(SaveError::Readback("zero-size target".into()));
    }

    // Build the transform for a target-sized area. The ortho mapping is area
    // independent, but the viewport pixel size drives the line-width expansion.
    let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(w as f32, h as f32));
    let transform = plot.transform(area);
    let transform_right = plot.transform_y2(area);
    let ortho_left = transform.ortho_matrix();
    let axis_log_left = axis_log_flags(&transform);
    let (ortho_right, axis_log_right) = match &transform_right {
        Some(t) => (t.ortho_matrix(), axis_log_flags(t)),
        None => (ortho_left, axis_log_left),
    };
    let bg = egui::Rgba::from(plot.data_background).to_array();

    let rgba = {
        let renderer = render_state.renderer.read();
        let res: &WgpuResources = renderer
            .callback_resources
            .get()
            .expect("WgpuResources not installed — call egui_silx::install() first");
        res.render_to_rgba(
            &render_state.device,
            &render_state.queue,
            render_state.target_format,
            plot.id,
            size,
            bg,
            ortho_left,
            axis_log_left,
            ortho_right,
            axis_log_right,
        )?
    };

    let png = encode_png(&rgba, w, h)?;
    std::fs::write(path, png)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_per_row_rounds_up_to_256() {
        assert_eq!(padded_bytes_per_row(1), 256); // 4 → 256
        assert_eq!(padded_bytes_per_row(64), 256); // 256 → 256 (exact)
        assert_eq!(padded_bytes_per_row(65), 512); // 260 → 512
        assert_eq!(padded_bytes_per_row(100), 512); // 400 → 512
    }

    #[test]
    fn rows_to_rgba8_unpads_and_passes_rgba_through() {
        // 1×2 image, row stride padded to 256. Rgba format → no channel swap.
        let bpr = padded_bytes_per_row(1);
        let mut mapped = vec![0u8; (bpr as usize) * 2];
        mapped[0..4].copy_from_slice(&[10, 20, 30, 40]); // row 0
        mapped[bpr as usize..bpr as usize + 4].copy_from_slice(&[50, 60, 70, 80]); // row 1
        let out = rows_to_rgba8(&mapped, 1, 2, bpr, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(out, vec![10, 20, 30, 40, 50, 60, 70, 80]);
    }

    #[test]
    fn rows_to_rgba8_swaps_bgra_to_rgba() {
        let bpr = padded_bytes_per_row(1);
        let mut mapped = vec![0u8; bpr as usize];
        mapped[0..4].copy_from_slice(&[30, 20, 10, 40]); // stored BGRA
        let out = rows_to_rgba8(&mapped, 1, 1, bpr, wgpu::TextureFormat::Bgra8UnormSrgb);
        assert_eq!(out, vec![10, 20, 30, 40]); // → RGBA
    }

    #[test]
    fn encode_png_round_trips() {
        // 2×2 RGBA encoded then decoded yields the same pixels.
        let rgba: Vec<u8> = (0..16).map(|i| i as u8 * 16).collect();
        let png = encode_png(&rgba, 2, 2).expect("encode");

        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = decoder.read_info().expect("read info");
        let mut buf = vec![0u8; reader.output_buffer_size().expect("buffer size")];
        let info = reader.next_frame(&mut buf).expect("frame");
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&buf[..rgba.len()], rgba.as_slice());
    }

    #[test]
    fn rgba_to_rgb_drops_alpha() {
        // 2×1 RGBA → RGB; alpha bytes (4th of each quad) are removed.
        let rgba = [10, 20, 30, 99, 40, 50, 60, 88];
        let rgb = rgba_to_rgb(&rgba, 2, 1);
        assert_eq!(rgb, vec![10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn encode_ppm_header_and_pixels_round_trip() {
        // 2×1 image with distinct pixels.
        let rgba = [1, 2, 3, 255, 4, 5, 6, 255];
        let ppm = encode_ppm(&rgba, 2, 1);

        // Header is exactly "P6\n2 1\n255\n" then raw RGB.
        let header = b"P6\n2 1\n255\n";
        assert_eq!(&ppm[..header.len()], header);
        // Raw RGB body, alpha dropped.
        assert_eq!(&ppm[header.len()..], &[1, 2, 3, 4, 5, 6]);
        // Total length = header + width*height*3 (2×1 pixels × 3 channels).
        assert_eq!(ppm.len(), header.len() + 6);
    }

    #[test]
    fn base64_encode_matches_known_vector() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encode_svg_is_well_formed_with_size_and_png_payload() {
        let rgba = [
            11, 22, 33, 255, 44, 55, 66, 255, 77, 88, 99, 255, 1, 2, 3, 255,
        ];
        let svg = encode_svg(&rgba, 2, 2).expect("svg");

        // XML declaration and SVG 1.1 DOCTYPE.
        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>"));
        assert!(svg.contains("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\""));
        // Root dimensions in px appear on the <svg> element.
        assert!(svg.contains("width=\"2\""));
        assert!(svg.contains("height=\"2\""));
        // The <image> element with a base64 PNG data URI and id.
        assert!(svg.contains("<image xlink:href=\"data:image/png;base64,"));
        assert!(svg.contains("x=\"0\""));
        assert!(svg.contains("y=\"0\""));
        assert!(svg.contains("id=\"image\" />"));
        assert!(svg.trim_end().ends_with("</svg>"));

        // The embedded payload decodes to a valid RGB PNG of the right size,
        // matching the input pixels (alpha dropped).
        let marker = "base64,";
        let start = svg.find(marker).expect("data uri") + marker.len();
        let end = svg[start..].find('"').expect("end quote") + start;
        let b64 = &svg[start..end];
        let png_bytes = base64_decode_for_test(b64);
        let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
        let mut reader = decoder.read_info().expect("read info");
        let mut buf = vec![0u8; reader.output_buffer_size().expect("buffer size")];
        let info = reader.next_frame(&mut buf).expect("frame");
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert_eq!(info.color_type, png::ColorType::Rgb);
        let expected_rgb = rgba_to_rgb(&rgba, 2, 2);
        assert_eq!(&buf[..expected_rgb.len()], expected_rgb.as_slice());
    }

    /// Minimal base64 decoder for the SVG payload round-trip test.
    fn base64_decode_for_test(s: &str) -> Vec<u8> {
        fn val(c: u8) -> Option<u8> {
            match c {
                b'A'..=b'Z' => Some(c - b'A'),
                b'a'..=b'z' => Some(c - b'a' + 26),
                b'0'..=b'9' => Some(c - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0u32;
        for &c in s.as_bytes() {
            if c == b'=' {
                break;
            }
            let Some(v) = val(c) else { continue };
            acc = (acc << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }
}
