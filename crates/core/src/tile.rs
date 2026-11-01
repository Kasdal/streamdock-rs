//! Tiny tile renderer used by the test pattern.
//!
//! This is not the product renderer. It draws a number, a background and an
//! orientation marker, so a person can check the key order, the key IDs and the
//! rotation on the real panel. It needs no font file and no extra crate.

use crate::image::Rgb;

/// Background colour of a test tile.
const BACKGROUND: [u8; 3] = [24, 24, 28];

/// Marker colour at the top left corner of the upright tile.
const MARKER: [u8; 3] = [220, 48, 48];

/// Digit colour.
const DIGIT: [u8; 3] = [240, 240, 240];

/// 5x7 bitmap for each digit. Bit 4 is the left column.
const FONT: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

/// One upright tile with the number and an orientation marker.
///
/// The caller must still apply the panel rotation before the JPEG encode.
pub fn number_tile(number: u8, size: (u16, u16)) -> Rgb {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut data = vec![0u8; w * h * 3];
    for pixel in data.chunks_exact_mut(3) {
        pixel.copy_from_slice(&BACKGROUND);
    }

    // Marker at the top left, so a wrong rotation is easy to see.
    let marker = (w / 12).clamp(4, 10);
    fill_rect(&mut data, w, 4, 4, marker, marker, &MARKER);

    let digits: Vec<u8> = if number >= 10 {
        vec![number / 10, number % 10]
    } else {
        vec![number]
    };

    // Each glyph is 5 cells wide and 7 tall. One cell is a gap between glyphs.
    // The scale comes from a two glyph width, so 1..9 and 10..18 look the same.
    let scale = (w / 13).min(h / 9).max(1);
    let text_w = (digits.len() * 6 - 1) * scale;
    let text_h = 7 * scale;
    let x0 = (w.saturating_sub(text_w)) / 2;
    let y0 = (h.saturating_sub(text_h)) / 2;

    for (index, &digit) in digits.iter().enumerate() {
        let glyph = &FONT[digit as usize % 10];
        let gx = x0 + index * 6 * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    fill_rect(
                        &mut data,
                        w,
                        gx + col * scale,
                        y0 + row * scale,
                        scale,
                        scale,
                        &DIGIT,
                    );
                }
            }
        }
    }

    Rgb {
        width: size.0,
        height: size.1,
        data,
    }
}

/// One tile with a solid colour.
pub fn solid_tile(rgb: [u8; 3], size: (u16, u16)) -> Rgb {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut data = vec![0u8; w * h * 3];
    for pixel in data.chunks_exact_mut(3) {
        pixel.copy_from_slice(&rgb);
    }
    Rgb {
        width: size.0,
        height: size.1,
        data,
    }
}

/// One tile with a frame of the given thickness and colour.
///
/// The frame shows whether the panel shows the complete image. A missing side
/// means that the real display is smaller than the requested size.
pub fn frame_tile(size: (u16, u16), thickness: u16, rgb: [u8; 3]) -> Rgb {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut data = vec![0u8; w * h * 3];
    for pixel in data.chunks_exact_mut(3) {
        pixel.copy_from_slice(&BACKGROUND);
    }

    let t = (thickness.max(1) as usize).min(w / 2).min(h / 2);
    fill_rect(&mut data, w, 0, 0, w, t, &rgb);
    fill_rect(&mut data, w, 0, h - t, w, t, &rgb);
    fill_rect(&mut data, w, 0, t, t, h - 2 * t, &rgb);
    fill_rect(&mut data, w, w - t, t, t, h - 2 * t, &rgb);

    Rgb {
        width: size.0,
        height: size.1,
        data,
    }
}

/// One tile of deterministic noise. It compresses badly, so it fills the
/// image size budget. The probe uses it to find the real firmware limit.
pub fn noise_tile(size: (u16, u16), seed: u32) -> Rgb {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut data = vec![0u8; w * h * 3];
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 16) as u8
    };
    for pixel in data.chunks_exact_mut(3) {
        pixel[0] = next();
        pixel[1] = next();
        pixel[2] = next();
    }
    Rgb {
        width: size.0,
        height: size.1,
        data,
    }
}

/// Fill a rectangle. Coordinates outside the image are clipped.
fn fill_rect(
    data: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgb: &[u8; 3],
) {
    let height = data.len() / (stride * 3);
    for py in y..(y + h).min(height) {
        for px in x..(x + w).min(stride) {
            let at = (py * stride + px) * 3;
            data[at..at + 3].copy_from_slice(rgb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_has_the_requested_size() {
        let tile = number_tile(7, (96, 96));
        assert_eq!(tile.width, 96);
        assert_eq!(tile.height, 96);
        assert_eq!(tile.data.len(), 96 * 96 * 3);
    }

    #[test]
    fn marker_is_at_the_top_left() {
        let tile = number_tile(1, (96, 96));
        let at = |x: usize, y: usize| {
            let i = (y * 96 + x) * 3;
            [tile.data[i], tile.data[i + 1], tile.data[i + 2]]
        };
        assert_eq!(at(5, 5), MARKER);
        assert_eq!(at(91, 91), BACKGROUND);
    }

    #[test]
    fn digits_are_drawn() {
        for number in 0..=18u8 {
            let tile = number_tile(number, (96, 96));
            let white = tile
                .data
                .chunks_exact(3)
                .filter(|p| *p == DIGIT)
                .count();
            assert!(white > 0, "number {number} drew no pixel");
        }
    }

    #[test]
    fn two_digit_number_fits() {
        let tile = number_tile(18, (80, 80));
        let white = tile
            .data
            .chunks_exact(3)
            .filter(|p| *p == DIGIT)
            .count();
        assert!(white > 0);
        // The marker must stay untouched by the text.
        let at = |x: usize, y: usize| {
            let i = (y * 80 + x) * 3;
            [tile.data[i], tile.data[i + 1], tile.data[i + 2]]
        };
        assert_eq!(at(5, 5), MARKER);
    }

    /// Height of the digit block in pixels.
    fn digit_height(tile: &Rgb) -> usize {
        let w = tile.width as usize;
        let h = tile.height as usize;
        let rows: Vec<usize> = (0..h)
            .filter(|&y| {
                (0..w).any(|x| {
                    let at = (y * w + x) * 3;
                    tile.data[at..at + 3] == DIGIT
                })
            })
            .collect();
        match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => last - first + 1,
            _ => 0,
        }
    }

    #[test]
    fn one_and_two_digit_numbers_have_the_same_glyph_size() {
        let one = digit_height(&number_tile(1, (96, 96)));
        let ten = digit_height(&number_tile(10, (96, 96)));
        let fifteen = digit_height(&number_tile(15, (96, 96)));
        assert!(one > 0);
        assert_eq!(one, ten);
        assert_eq!(one, fifteen);
    }

    #[test]
    fn frame_has_all_four_sides() {
        let tile = frame_tile((80, 80), 2, [255, 255, 255]);
        assert_eq!((tile.width, tile.height), (80, 80));
        assert_eq!(tile.pixel(40, 1), [255, 255, 255]);
        assert_eq!(tile.pixel(40, 78), [255, 255, 255]);
        assert_eq!(tile.pixel(1, 40), [255, 255, 255]);
        assert_eq!(tile.pixel(78, 40), [255, 255, 255]);
        assert_eq!(tile.pixel(40, 40), BACKGROUND);
    }
}
