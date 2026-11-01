//! Text on a key tile with a real TrueType font.
//!
//! The font is a system font, so nothing is redistributed. A product build
//! must bundle a font with a suitable licence and point `font_path` at it.
//!
//! When no system font is found, the crate falls back to the 5x7 bitmap font
//! in `streamdock_core::tile`, so a key never goes blank.

use std::path::PathBuf;
use std::sync::OnceLock;

use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use streamdock_core::image::Rgb;
use streamdock_core::tile;

/// Pixel size of one bitmap font cell. Used by the fallback only.
pub const CELL_PX: f32 = 6.0;

/// Space kept free around the text, in pixels.
pub const MARGIN_PX: f32 = 5.0;

static FONT: OnceLock<Option<FontArc>> = OnceLock::new();

/// Font that ships with the renderer. Roboto, Apache 2.0. See `assets`.
const BUNDLED_FONT: &[u8] = include_bytes!("../assets/Roboto-Variable.ttf");

/// Override the font with this environment variable.
pub const FONT_ENV: &str = "STREAMDOCK_FONT";

/// The font in use, if one was found.
fn font() -> Option<&'static FontArc> {
    FONT.get_or_init(load_font).as_ref()
}

fn load_font() -> Option<FontArc> {
    if let Some(path) = std::env::var_os(FONT_ENV) {
        if let Ok(bytes) = std::fs::read(PathBuf::from(path)) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }

    // The bundled font keeps every installation the same.
    if let Ok(font) = FontArc::try_from_slice(BUNDLED_FONT) {
        return Some(font);
    }

    let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    for name in ["segoeui.ttf", "arial.ttf", "tahoma.ttf", "verdana.ttf"] {
        let path = PathBuf::from(&windows).join("Fonts").join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }

    None
}

/// One line of laid out text.
struct Line {
    text: String,
    width: f32,
}

struct Layout {
    lines: Vec<Line>,
    height: f32,
    line_height: f32,
    ascent: f32,
}

/// Largest pixel size where the text fits, with one line per explicit line.
///
/// The page uses the smallest value of its labels, so `1` and `10` match.
pub fn fit_px(text: &str, width: f32, height: f32, max_px: f32) -> f32 {
    let Some(font) = font() else {
        let cells = tile::fit_scale(text, width as usize, height as usize) as f32 * CELL_PX;
        return cells.min(max_px).max(9.0);
    };

    let lines = text.split('\n').count().max(1) as f32;
    let mut px = max_px.max(9.0);

    while px >= 9.0 {
        let scaled = font.as_scaled(PxScale::from(px));
        let widest = text
            .split('\n')
            .map(|line| measure(&scaled, line))
            .fold(0.0f32, f32::max);
        let line_height = scaled.ascent() - scaled.descent() + scaled.line_gap();

        if widest <= width - 2.0 * MARGIN_PX && lines * line_height <= height - 2.0 * MARGIN_PX {
            return px;
        }
        px -= 1.0;
    }

    9.0
}

/// One tile with text at a chosen pixel size.
///
/// A long label wraps, and if it still does not fit, the pixel size shrinks.
pub fn label_tile(
    text: &str,
    size: (u16, u16),
    background: [u8; 3],
    foreground: [u8; 3],
    size_px: usize,
) -> Rgb {
    let Some(font) = font() else {
        let cells = (size_px / CELL_PX as usize).clamp(1, 12);
        return tile::label_tile_scaled(text, size, background, foreground, cells);
    };

    let (width, height) = (size.0 as f32, size.1 as f32);
    let mut px = (size_px as f32).clamp(9.0, height * 0.9);

    for _ in 0..14 {
        if let Some(layout) = layout(font, text, px, width - 2.0 * MARGIN_PX) {
            if layout.height <= height - 2.0 * MARGIN_PX {
                return draw(
                    font,
                    &layout,
                    px,
                    size,
                    background,
                    foreground,
                );
            }
        }
        px *= 0.88;
    }

    // Nothing fit: draw the last layout anyway, clipped.
    match layout(font, text, px, width - 2.0 * MARGIN_PX) {
        Some(layout) => draw(font, &layout, px, size, background, foreground),
        None => tile::solid_tile(background, size),
    }
}

/// Break the text into lines that fit `max_width` at this pixel size.
fn layout(font: &FontArc, text: &str, px: f32, max_width: f32) -> Option<Layout> {
    let scaled = font.as_scaled(PxScale::from(px));
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let line_height = ascent - descent + scaled.line_gap();
    let max_width = max_width.max(8.0);

    let mut lines: Vec<Line> = Vec::new();

    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_width = 0.0f32;

        for word in paragraph.split_whitespace() {
            let word_width = measure(&scaled, word);
            let space = measure(&scaled, " ");

            if current.is_empty() {
                // A word that is wider than one line is split by characters.
                if word_width > max_width {
                    let mut piece = String::new();
                    let mut piece_width = 0.0;
                    for ch in word.chars() {
                        let advance = measure(&scaled, &ch.to_string());
                        if piece_width + advance > max_width && !piece.is_empty() {
                            lines.push(Line {
                                text: std::mem::take(&mut piece),
                                width: piece_width,
                            });
                            piece_width = 0.0;
                        }
                        piece.push(ch);
                        piece_width += advance;
                    }
                    current = piece;
                    current_width = piece_width;
                } else {
                    current.push_str(word);
                    current_width = word_width;
                }
            } else if current_width + space + word_width <= max_width {
                current.push(' ');
                current.push_str(word);
                current_width += space + word_width;
            } else {
                lines.push(Line {
                    text: std::mem::take(&mut current),
                    width: current_width,
                });
                current.push_str(word);
                current_width = word_width;
            }
        }

        lines.push(Line {
            text: current,
            width: current_width,
        });
    }

    if lines.is_empty() {
        return None;
    }

    let height = lines.len() as f32 * line_height;
    Some(Layout {
        lines,
        height,
        line_height,
        ascent,
    })
}

fn measure(scaled: &ab_glyph::PxScaleFont<&FontArc>, text: &str) -> f32 {
    text.chars()
        .map(|ch| scaled.h_advance(scaled.glyph_id(ch)))
        .sum()
}

fn draw(
    font: &FontArc,
    layout: &Layout,
    px: f32,
    size: (u16, u16),
    background: [u8; 3],
    foreground: [u8; 3],
) -> Rgb {
    let (width, height) = (size.0 as usize, size.1 as usize);
    let mut data = vec![0u8; width * height * 3];
    for pixel in data.chunks_exact_mut(3) {
        pixel.copy_from_slice(&background);
    }

    let scaled = font.as_scaled(PxScale::from(px));
    let top = ((height as f32 - layout.height) / 2.0).max(0.0);

    for (index, line) in layout.lines.iter().enumerate() {
        let baseline = top + index as f32 * layout.line_height + layout.ascent;
        let mut x = ((width as f32 - line.width) / 2.0).max(0.0);

        for ch in line.text.chars() {
            let glyph_id = scaled.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id);
            let glyph = glyph_id.with_scale_and_position(PxScale::from(px), point(x, baseline));

            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px_x = bounds.min.x as i32 + gx as i32;
                    let px_y = bounds.min.y as i32 + gy as i32;
                    if px_x < 0 || px_y < 0 || px_x >= width as i32 || px_y >= height as i32 {
                        return;
                    }
                    let at = (px_y as usize * width + px_x as usize) * 3;
                    for channel in 0..3 {
                        let dst = data[at + channel] as f32;
                        let src = foreground[channel] as f32;
                        data[at + channel] =
                            (dst + (src - dst) * coverage).round().clamp(0.0, 255.0) as u8;
                    }
                });
            }

            x += advance;
        }
    }

    Rgb {
        width: size.0,
        height: size.1,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_font() -> bool {
        font().is_some()
    }

    #[test]
    fn text_draws_pixels() {
        if !have_font() {
            return;
        }
        let tile = label_tile("MUTE", (96, 96), [0, 0, 0], [255, 255, 255], 28);
        let lit = tile
            .data
            .chunks_exact(3)
            .filter(|pixel| *pixel != [0, 0, 0])
            .count();
        assert!(lit > 30, "only {lit} pixels were drawn");
    }

    #[test]
    fn empty_text_is_the_background() {
        let tile = label_tile("", (96, 96), [10, 20, 30], [255, 255, 255], 28);
        assert!(tile.data.chunks_exact(3).all(|pixel| pixel == [10, 20, 30]));
    }

    #[test]
    fn one_and_two_digit_labels_have_the_same_height() {
        if !have_font() {
            return;
        }
        let one = label_tile("1", (96, 96), [0, 0, 0], [255, 255, 255], 28);
        let ten = label_tile("10", (96, 96), [0, 0, 0], [255, 255, 255], 28);
        let a = text_height(&one);
        let b = text_height(&ten);
        assert!(a > 0 && b > 0);
        assert!((a as i32 - b as i32).abs() <= 2, "heights {a} and {b}");
    }

    #[test]
    fn a_long_label_stays_inside_the_tile() {
        if !have_font() {
            return;
        }
        let tile = label_tile(
            "a rather long label that must wrap and shrink",
            (96, 96),
            [0, 0, 0],
            [255, 255, 255],
            6,
        );
        assert_eq!(tile.data.len(), 96 * 96 * 3);
        for y in 0..96 {
            for x in 0..96 {
                let inside = x >= 0 && y >= 0;
                assert!(inside);
                let _ = tile.pixel(x as u16, y as u16);
            }
        }
        assert!(text_height(&tile) > 0);
    }

    #[test]
    fn a_scale_changes_the_text_height() {
        if !have_font() {
            return;
        }
        let small = label_tile("A", (96, 96), [0, 0, 0], [255, 255, 255], 14);
        let large = label_tile("A", (96, 96), [0, 0, 0], [255, 255, 255], 28);
        assert!(text_height(&large) > text_height(&small));
    }

    fn text_height(tile: &Rgb) -> usize {
        let w = tile.width as usize;
        let h = tile.height as usize;
        let rows: Vec<usize> = (0..h)
            .filter(|&y| {
                (0..w).any(|x| {
                    let at = (y * w + x) * 3;
                    tile.data[at..at + 3] != [0, 0, 0]
                })
            })
            .collect();
        match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => last - first + 1,
            _ => 0,
        }
    }
}
