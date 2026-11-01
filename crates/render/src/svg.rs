//! SVG rendering for key tiles.
//!
//! `resvg` draws into a small pixmap, which is filled with the key background
//! first. The result is opaque RGB, so the panel gets a normal JPEG.
//!
//! The SVG is scaled to fit the key and centred. A square icon keeps its
//! aspect ratio.

use resvg::tiny_skia::{Color, Pixmap, Transform};
use resvg::usvg;
use streamdock_core::image::Rgb;

/// Render one SVG file into a tile.
pub fn render_svg(bytes: &[u8], size: (u16, u16), background: [u8; 3]) -> Result<Rgb, String> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| format!("decode svg: {e}"))?;

    let (width, height) = (size.0, size.1);
    let mut pixmap = Pixmap::new(width as u32, height as u32)
        .ok_or_else(|| "cannot allocate the tile".to_string())?;

    pixmap.fill(Color::from_rgba8(
        background[0],
        background[1],
        background[2],
        255,
    ));

    let tree_size = tree.size();
    if tree_size.width() <= 0.0 || tree_size.height() <= 0.0 {
        return Err("SVG has no size".into());
    }

    let scale = (width as f32 / tree_size.width()).min(height as f32 / tree_size.height());
    let dx = (width as f32 - tree_size.width() * scale) / 2.0;
    let dy = (height as f32 - tree_size.height() * scale) / 2.0;
    let transform = Transform::from_scale(scale, scale).post_translate(dx, dy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let pixels = pixmap.data();
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in pixels.chunks_exact(4) {
        data.push(pixel[0]);
        data.push(pixel[1]);
        data.push(pixel[2]);
    }

    Rgb::new(width, height, data)
}

/// Parse an SVG without rendering it. Used to check an upload.
pub fn validate_svg(bytes: &[u8]) -> Result<(), String> {
    let options = usvg::Options::default();
    usvg::Tree::from_data(bytes, &options)
        .map(|_| ())
        .map_err(|e| format!("decode svg: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <rect x="0" y="0" width="10" height="10" fill="#ff0000"/>
    </svg>"##;

    #[test]
    fn a_square_svg_fills_the_tile() {
        let tile = render_svg(RED_SQUARE, (96, 96), [0, 0, 255]).expect("render");
        assert_eq!((tile.width, tile.height), (96, 96));
        assert_eq!(tile.pixel(48, 48), [255, 0, 0]);
        assert_eq!(tile.pixel(0, 0), [255, 0, 0]);
    }

    #[test]
    fn a_wide_svg_keeps_its_aspect_ratio() {
        let wide = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10">
            <rect x="0" y="0" width="20" height="10" fill="#00ff00"/>
        </svg>"##;
        let tile = render_svg(wide, (96, 96), [0, 0, 0]).expect("render");
        assert_eq!(tile.pixel(48, 48), [0, 255, 0]);
        // Above and below the scaled shape the background shows.
        assert_eq!(tile.pixel(48, 2), [0, 0, 0]);
        assert_eq!(tile.pixel(48, 94), [0, 0, 0]);
    }

    #[test]
    fn a_bad_svg_is_rejected() {
        assert!(render_svg(b"not svg", (96, 96), [0, 0, 0]).is_err());
        assert!(validate_svg(b"not svg").is_err());
        assert!(validate_svg(RED_SQUARE).is_ok());
    }
}
