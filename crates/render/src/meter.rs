//! The image for the side strip: system meters, one per band.
//!
//! The strip is one display, 80 by 400 pixels on the unit under test. Each
//! meter gets an equal band with its name, its value and a bar.

use streamdock_core::image::Rgb;
use streamdock_core::tile;

use crate::text;

/// One meter to draw.
#[derive(Debug, Clone, Copy)]
pub struct Meter<'a> {
    /// Short name, such as `CPU`.
    pub label: &'a str,
    /// The value as text, such as `63%` or `--`.
    pub value: &'a str,
    /// Fill of the bar, 0 to 100. `None` draws an empty bar.
    pub percent: Option<f32>,
    /// Colour of the bar and of the value text.
    pub colour: [u8; 3],
}

/// Side margin of the bar in pixels.
pub const BAR_MARGIN: u16 = 10;

/// Height of the bar in pixels.
pub const BAR_HEIGHT: u16 = 14;

/// Corner radius of the bar in pixels.
pub const BAR_RADIUS: u16 = 4;

/// Draw one strip image with the meters stacked from the top.
pub fn strip(
    size: (u16, u16),
    meters: &[Meter],
    background: [u8; 3],
    foreground: [u8; 3],
) -> Rgb {
    let mut base = tile::solid_tile(background, size);
    if meters.is_empty() {
        return base;
    }

    let width = size.0;
    let band = size.1 / meters.len() as u16;
    let track = mix(background, foreground, 0.16);
    let dim = mix(background, foreground, 0.55);
    let line = mix(background, foreground, 0.10);

    let label_height = (band as f32 * 0.22) as u16;
    let value_height = (band as f32 * 0.42) as u16;
    let pad = (band - label_height - value_height - BAR_HEIGHT) / 3;

    for (index, meter) in meters.iter().enumerate() {
        let top = band * index as u16;

        // A thin rule keeps the bands apart without a hard border.
        if index > 0 {
            fill_rect(&mut base, 0, top, width, 1, &line);
        }

        let label = text::label_tile(
            meter.label,
            (width, label_height),
            background,
            dim,
            (label_height as f32 * 0.85) as usize,
        );
        blit(&mut base, &label, 0, top + pad);

        let value = text::label_tile(
            meter.value,
            (width, value_height),
            background,
            meter.colour,
            (value_height as f32 * 0.95) as usize,
        );
        blit(&mut base, &value, 0, top + pad + label_height);

        let bar_y = top + band - pad - BAR_HEIGHT;
        let inner = width - 2 * BAR_MARGIN;
        fill_round(&mut base, BAR_MARGIN, bar_y, inner, BAR_HEIGHT, BAR_RADIUS, &track);
        if let Some(percent) = meter.percent {
            let filled = (inner as f32 * percent.clamp(0.0, 100.0) / 100.0).round() as u16;
            if filled > 0 {
                fill_round(
                    &mut base,
                    BAR_MARGIN,
                    bar_y,
                    filled,
                    BAR_HEIGHT,
                    BAR_RADIUS,
                    &meter.colour,
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

    fn image(meters: &[Meter]) -> Rgb {
        strip((80, 400), meters, BG, FG)
    }

    fn red_pixels_in_row(image: &Rgb, y: u16) -> usize {
        (0..image.width)
            .filter(|x| image.pixel(*x, y) == RED)
            .count()
    }

    #[test]
    fn the_strip_has_the_requested_size() {
        let image = image(&[]);
        assert_eq!(image.width, 80);
        assert_eq!(image.height, 400);
    }

    #[test]
    fn a_full_bar_is_wider_than_a_half_bar() {
        let full = image(&[Meter {
            label: "CPU",
            value: "100%",
            percent: Some(100.0),
            colour: RED,
        }]);
        let half = image(&[Meter {
            label: "CPU",
            value: "50%",
            percent: Some(50.0),
            colour: RED,
        }]);

        // Inside the bar of the single band, which runs from 343 to 356.
        let row = 349;
        let full_pixels = red_pixels_in_row(&full, row);
        let half_pixels = red_pixels_in_row(&half, row);
        assert!(full_pixels > half_pixels, "full {full_pixels} half {half_pixels}");
        assert_eq!(full_pixels, (80 - 2 * BAR_MARGIN) as usize);
    }

    #[test]
    fn a_missing_value_leaves_the_bar_empty() {
        let image = image(&[Meter {
            label: "GPU",
            value: "--",
            percent: None,
            colour: RED,
        }]);
        let row = 349;
        assert_eq!(red_pixels_in_row(&image, row), 0);
    }

    #[test]
    fn three_meters_stay_inside_their_bands() {
        let meters = [
            Meter {
                label: "CPU",
                value: "10%",
                percent: Some(10.0),
                colour: RED,
            },
            Meter {
                label: "GPU",
                value: "20%",
                percent: Some(20.0),
                colour: RED,
            },
            Meter {
                label: "MEM",
                value: "30%",
                percent: Some(30.0),
                colour: RED,
            },
        ];
        let image = image(&meters);
        // Bands are 126 pixels tall, so the three bars sit in these rows.
        assert!(red_pixels_in_row(&image, 110) > 0);
        assert!(red_pixels_in_row(&image, 245) > 0);
        assert!(red_pixels_in_row(&image, 378) > 0);
    }

    #[test]
    fn the_bar_never_leaves_the_strip() {
        let image = image(&[Meter {
            label: "CPU",
            value: "100%",
            percent: Some(100.0),
            colour: RED,
        }]);
        let row = 349;
        assert_eq!(image.pixel(0, row), BG);
        assert_eq!(image.pixel(9, row), BG);
        assert_eq!(image.pixel(10, row), RED);
        assert_eq!(image.pixel(69, row), RED);
        assert_eq!(image.pixel(70, row), BG);
        assert_eq!(image.pixel(79, row), BG);
    }

    #[test]
    fn the_bar_has_rounded_ends() {
        let image = image(&[Meter {
            label: "CPU",
            value: "100%",
            percent: Some(100.0),
            colour: RED,
        }]);
        // The top row of the bar is inset by the radius, the middle is not.
        assert_eq!(image.pixel(BAR_MARGIN, 343), BG);
        assert_eq!(image.pixel(BAR_MARGIN + BAR_RADIUS, 343), RED);
        assert_eq!(image.pixel(BAR_MARGIN, 349), RED);
    }
}
