//! The image for the side strip: three bands, one widget each.
//!
//! The strip is one display, 96 by 460 pixels on the unit under test. A band
//! holds a small name, a large value, an optional second line and an optional
//! bar.

use streamdock_core::image::Rgb;
use streamdock_core::tile;

use crate::text;

/// One band of the strip.
#[derive(Debug, Clone, Copy)]
pub struct Band<'a> {
    /// Small name at the top, such as `CPU`. Empty for no label.
    pub label: &'a str,
    /// The large line, such as `63%`, `14:05` or `18°`.
    pub value: &'a str,
    /// A small line under the value, such as `Partly cloudy` or the date.
    pub subtitle: Option<&'a str>,
    /// Fill of the bar, 0 to 100. `None` draws no bar.
    pub percent: Option<f32>,
    /// Colour of the value and of the bar.
    pub colour: [u8; 3],
}

/// Side margin of the bar in pixels.
pub const BAR_MARGIN: u16 = 10;

/// Height of the bar in pixels.
pub const BAR_HEIGHT: u16 = 14;

/// Corner radius of the bar in pixels.
pub const BAR_RADIUS: u16 = 4;

/// Draw one strip image with the bands stacked from the top.
///
/// `scale` changes the text size. 1.0 is the normal size, 0.6 is small and
/// 1.6 is large.
pub fn strip(
    size: (u16, u16),
    bands: &[Band],
    scale: f32,
    background: [u8; 3],
    foreground: [u8; 3],
) -> Rgb {
    let mut base = tile::solid_tile(background, size);
    if bands.is_empty() {
        return base;
    }

    let scale = scale.clamp(0.4, 2.0);
    let width = size.0;
    let height = size.1;
    let band_height = height / bands.len() as u16;
    let track = mix(background, foreground, 0.16);
    let dim = mix(background, foreground, 0.55);
    let line = mix(background, foreground, 0.10);

    for (index, band) in bands.iter().enumerate() {
        let top = band_height * index as u16;

        // A thin rule keeps the bands apart without a hard border.
        if index > 0 {
            fill_rect(&mut base, 0, top, width, 1, &line);
        }

        // The text size option moves the room between the text and the gaps,
        // not only the font size, so a bigger setting really is bigger.
        let bar_height = if band.percent.is_some() { BAR_HEIGHT } else { 0 };
        let room = band_height.saturating_sub(bar_height + 3);
        let mut label_height = (band_height as f32 * 0.16 * scale) as u16;
        let mut value_height = (band_height as f32 * 0.36 * scale) as u16;
        let mut subtitle_height = if band.subtitle.is_some() {
            (band_height as f32 * 0.13 * scale) as u16
        } else {
            0
        };

        let text = label_height + value_height + subtitle_height;
        if text > room && text > 0 {
            let factor = room as f32 / text as f32;
            label_height = (label_height as f32 * factor) as u16;
            value_height = (value_height as f32 * factor) as u16;
            subtitle_height = (subtitle_height as f32 * factor) as u16;
        }

        let used = label_height + value_height + subtitle_height + bar_height;
        let pad = band_height.saturating_sub(used) / 3;

        let mut y = top + pad;

        if !band.label.is_empty() {
            let label = text::label_tile(
                band.label,
                (width, label_height),
                background,
                dim,
                (label_height as f32 * 0.85 * scale) as usize,
            );
            blit(&mut base, &label, 0, y);
        }
        y += label_height;

        let value = text::label_tile(
            band.value,
            (width, value_height),
            background,
            band.colour,
            (value_height as f32 * 0.95 * scale) as usize,
        );
        blit(&mut base, &value, 0, y);
        y += value_height;

        if let Some(subtitle) = band.subtitle {
            let line_tile = text::label_tile(
                subtitle,
                (width, subtitle_height),
                background,
                dim,
                (subtitle_height as f32 * 0.85 * scale) as usize,
            );
            blit(&mut base, &line_tile, 0, y);
        }

        if let Some(percent) = band.percent {
            let bar_y = top + band_height - pad - BAR_HEIGHT;
            let inner = width - 2 * BAR_MARGIN;
            fill_round(&mut base, BAR_MARGIN, bar_y, inner, BAR_HEIGHT, BAR_RADIUS, &track);

            let filled = (inner as f32 * percent.clamp(0.0, 100.0) / 100.0).round() as u16;
            if filled > 0 {
                fill_round(
                    &mut base,
                    BAR_MARGIN,
                    bar_y,
                    filled,
                    BAR_HEIGHT,
                    BAR_RADIUS,
                    &band.colour,
                );
            }
        }
    }

    base
}

/// Copy one image into another at a corner.
fn blit(base: &mut Rgb, part: &Rgb, x0: u16, y0: u16) {
    let base_width = base.width as usize;
    let part_width = part.width as usize;
    let part_height = part.height as usize;

    for row in 0..part_height {
        let y = y0 as usize + row;
        if y >= base.height as usize || x0 as usize + part_width > base_width {
            break;
        }
        let from = row * part_width * 3;
        let to = (y * base_width + x0 as usize) * 3;
        base.data[to..to + part_width * 3]
            .copy_from_slice(&part.data[from..from + part_width * 3]);
    }
}

/// Fill one rectangle with a colour.
fn fill_rect(image: &mut Rgb, x0: u16, y0: u16, width: u16, height: u16, colour: &[u8; 3]) {
    let stride = image.width as usize;
    for y in y0..y0.saturating_add(height) {
        if y >= image.height {
            break;
        }
        for x in x0..x0.saturating_add(width) {
            if x >= image.width {
                break;
            }
            let at = (y as usize * stride + x as usize) * 3;
            image.data[at..at + 3].copy_from_slice(colour);
        }
    }
}

/// Fill a rectangle with rounded ends.
fn fill_round(
    image: &mut Rgb,
    x0: u16,
    y0: u16,
    width: u16,
    height: u16,
    radius: u16,
    colour: &[u8; 3],
) {
    let radius = radius.min(width / 2).min(height / 2);
    if radius == 0 {
        fill_rect(image, x0, y0, width, height, colour);
        return;
    }

    // Each row keeps the full width, except the rows of the two caps.
    for row in 0..height {
        let inset = if row < radius {
            radius - row
        } else if row + radius >= height {
            radius - (height - 1 - row)
        } else {
            0
        };
        fill_rect(
            image,
            x0 + inset,
            y0 + row,
            width.saturating_sub(2 * inset),
            1,
            colour,
        );
    }
}

/// Mix two colours. `amount` 0 keeps `from`, 1 gives `to`.
fn mix(from: [u8; 3], to: [u8; 3], amount: f32) -> [u8; 3] {
    let blend = |a: u8, b: u8| {
        (a as f32 + (b as f32 - a as f32) * amount)
            .clamp(0.0, 255.0)
            .round() as u8
    };
    [
        blend(from[0], to[0]),
        blend(from[1], to[1]),
        blend(from[2], to[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: [u8; 3] = [20, 20, 24];
    const FG: [u8; 3] = [240, 240, 240];
    const RED: [u8; 3] = [255, 0, 0];
    const SIZE: (u16, u16) = (96, 460);

    fn image(bands: &[Band]) -> Rgb {
        strip(SIZE, bands, 1.0, BG, FG)
    }

    fn bar_rows(band: u16) -> u16 {
        // One third of 460 is 153, and the bar sits at the bottom of a band.
        band
    }

    fn red_pixels_in_row(image: &Rgb, y: u16) -> usize {
        (0..image.width)
            .filter(|x| image.pixel(*x, y) == RED)
            .count()
    }

    #[test]
    fn the_strip_has_the_requested_size() {
        let image = image(&[]);
        assert_eq!(image.width, 96);
        assert_eq!(image.height, 460);
    }

    #[test]
    fn a_full_bar_is_wider_than_a_half_bar() {
        let full = image(&[Band {
            label: "CPU",
            value: "100%",
            subtitle: None,
            percent: Some(100.0),
            colour: RED,
        }]);
        let half = image(&[Band {
            label: "CPU",
            value: "50%",
            subtitle: None,
            percent: Some(50.0),
            colour: RED,
        }]);

        // A single band is 460 tall: the bar runs from 377 to 390.
        let row = 384;
        let full_pixels = red_pixels_in_row(&full, row);
        let half_pixels = red_pixels_in_row(&half, row);
        assert!(full_pixels > half_pixels, "full {full_pixels} half {half_pixels}");
        assert_eq!(full_pixels, (96 - 2 * BAR_MARGIN) as usize);
    }

    #[test]
    fn a_missing_value_leaves_no_bar() {
        let image = image(&[Band {
            label: "GPU",
            value: "--",
            subtitle: None,
            percent: None,
            colour: RED,
        }]);
        // The dashes are drawn in the band colour, but no row is a solid bar.
        let widest = (0..image.height)
            .map(|row| red_pixels_in_row(&image, row))
            .max()
            .unwrap_or(0);
        assert!(
            widest < 70,
            "a bar would fill {} pixels, the widest row has {widest}",
            96 - 2 * BAR_MARGIN
        );
    }

    #[test]
    fn the_bar_never_leaves_the_strip() {
        let image = image(&[Band {
            label: "CPU",
            value: "100%",
            subtitle: None,
            percent: Some(100.0),
            colour: RED,
        }]);
        let row = 384;
        assert_eq!(image.pixel(0, row), BG);
        assert_eq!(image.pixel(9, row), BG);
        assert_eq!(image.pixel(10, row), RED);
        assert_eq!(image.pixel(85, row), RED);
        assert_eq!(image.pixel(86, row), BG);
        assert_eq!(image.pixel(95, row), BG);
    }

    #[test]
    fn the_bar_has_rounded_ends() {
        let image = image(&[Band {
            label: "CPU",
            value: "100%",
            subtitle: None,
            percent: Some(100.0),
            colour: RED,
        }]);
        let top = 377;
        // The top row of the bar is inset by the radius, a middle row is not.
        assert_eq!(image.pixel(BAR_MARGIN, top), BG);
        assert_eq!(image.pixel(BAR_MARGIN + BAR_RADIUS, top), RED);
        assert_eq!(image.pixel(BAR_MARGIN, top + 7), RED);
    }

    #[test]
    fn three_bands_stay_inside_their_thirds() {
        let bands = [
            Band {
                label: "CPU",
                value: "10%",
                subtitle: None,
                percent: Some(10.0),
                colour: RED,
            },
            Band {
                label: "GPU",
                value: "20%",
                subtitle: None,
                percent: Some(20.0),
                colour: RED,
            },
            Band {
                label: "MEM",
                value: "30%",
                subtitle: None,
                percent: Some(30.0),
                colour: RED,
            },
        ];
        let image = image(&bands);
        // Each band is 153 pixels tall.
        assert!(red_pixels_in_row(&image, bar_rows(130)) > 0);
        assert!(red_pixels_in_row(&image, bar_rows(283)) > 0);
        assert!(red_pixels_in_row(&image, bar_rows(436)) > 0);
    }

    #[test]
    fn a_band_without_a_bar_uses_the_room_for_text() {
        let image = image(&[Band {
            label: "BERLIN",
            value: "18°",
            subtitle: Some("Cloudy"),
            percent: None,
            colour: RED,
        }]);
        // The subtitle adds a third line, so the band keeps its rule but has
        // no bar.
        let mut red_rows = 0;
        for row in 0..image.height {
            if red_pixels_in_row(&image, row) > 0 {
                red_rows += 1;
            }
        }
        assert!(red_rows > 0, "the value text is drawn in the band colour");
        assert!(red_rows < 100, "no bar, so the colour stays in the text");
    }

    #[test]
    fn a_bigger_text_scale_draws_more_pixels() {
        let band = [Band {
            label: "CPU",
            value: "42%",
            subtitle: None,
            percent: None,
            colour: RED,
        }];
        let small = strip(SIZE, &band, 0.7, BG, FG);
        let normal = strip(SIZE, &band, 1.0, BG, FG);
        let large = strip(SIZE, &band, 1.5, BG, FG);

        let red = |image: &Rgb| {
            let mut count = 0;
            for row in 0..image.height {
                count += red_pixels_in_row(image, row);
            }
            count
        };

        let small_pixels = red(&small);
        let normal_pixels = red(&normal);
        let large_pixels = red(&large);
        assert!(small_pixels < normal_pixels, "{small_pixels} < {normal_pixels}");
        assert!(normal_pixels < large_pixels, "{normal_pixels} < {large_pixels}");
    }
}
