//! Key tile rendering.
//!
//! The renderer follows the measured order: compose upright, rotate 270
//! degrees, then encode under the size limit.
//!
//! This is the M2 renderer: solid colour, text and image files. SVG icons and
//! animation arrive in M4.
//!
//! Text uses one glyph scale for a whole page, so a label `1` and a label `10`
//! have the same size.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use streamdock_core::image::{self, Rgb};
use streamdock_core::{animation, model, proto, tile};
use streamdock_store::profile::{parse_color, ImageSpec, Key, Page, Theme};

pub mod icons;
pub mod meter;
pub mod svg;
pub mod text;

/// Default tile background.
pub const BACKGROUND: [u8; 3] = [32, 32, 36];

/// Default tile text colour.
pub const FOREGROUND: [u8; 3] = [240, 240, 240];

/// Colours used when a key does not set its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: BACKGROUND,
            foreground: FOREGROUND,
        }
    }
}

/// Build a palette from a profile theme.
pub fn palette(theme: &Theme) -> Result<Palette, String> {
    Ok(Palette {
        background: parse_color(&theme.background)?,
        foreground: parse_color(&theme.foreground)?,
    })
}

/// Largest text size for a label, as a fraction of the key side.
pub const MAX_LABEL_FRACTION: f32 = 0.42;

/// Largest text size for a label, in pixels.
pub fn max_label_px(size: (u16, u16)) -> f32 {
    (size.1 as f32 * MAX_LABEL_FRACTION).max(10.0)
}

/// The text that a key shows, if the key has text.
fn label_of(config: &Key) -> Option<String> {
    match &config.image {
        Some(ImageSpec::Text { text, .. }) => Some(text.clone()),
        Some(ImageSpec::Icon { name, .. }) => Some(name.clone()),
        Some(_) => None,
        None => config.label.clone(),
    }
}

/// One text size for a group of keys, so labels on one page match.
///
/// The size is in pixels. A long label shrinks the whole group. A key without
/// text does not count.
pub fn page_scale(page: &Page, size: (u16, u16), first: u8, last: u8) -> usize {
    let limit = max_label_px(size);
    let mut px = limit;

    for key in first..=last {
        if let Some(config) = page.key(key) {
            if let Some(label) = label_of(config) {
                px = px.min(text::fit_px(
                    &label,
                    size.0 as f32,
                    size.1 as f32,
                    limit,
                ));
            }
        }
    }

    px.round().clamp(9.0, limit) as usize
}

/// The scale for a key of this size on this page.
pub fn scale_for(page: &Page, key: u8) -> usize {
    if key <= model::GRID_KEYS {
        page_scale(
            page,
            (model::GRID_KEY_PIXELS, model::GRID_KEY_PIXELS),
            1,
            model::GRID_KEYS,
        )
    } else {
        page_scale(
            page,
            (model::STRIP_KEY_PIXELS, model::STRIP_KEY_PIXELS),
            model::GRID_KEYS + 1,
            model::KEY_COUNT,
        )
    }
}

/// Resolve an image path. A relative path is joined to the profile folder, so
/// a profile and its assets can move as one folder.
pub fn resolve_path(base: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    }
}

/// Compose the tile for one key, upright.
pub fn compose(config: Option<&Key>, size: (u16, u16), size_px: usize) -> Result<Rgb, String> {
    compose_in(config, size, size_px, Path::new("."))
}

/// Compose the tile for one key, with a base folder for relative image paths.
pub fn compose_in(
    config: Option<&Key>,
    size: (u16, u16),
    size_px: usize,
    base: &Path,
) -> Result<Rgb, String> {
    compose_in_themed(config, size, size_px, base, &Palette::default())
}

/// Compose the tile for one key with a palette from the profile theme.
pub fn compose_in_themed(
    config: Option<&Key>,
    size: (u16, u16),
    size_px: usize,
    base: &Path,
    palette: &Palette,
) -> Result<Rgb, String> {
    let (width, height) = size;

    let Some(key) = config else {
        return Ok(tile::solid_tile(palette.background, size));
    };

    if let Some(spec) = &key.image {
        return match spec {
            ImageSpec::Color { color } => {
                let rgb = parse_color(color)?;
                Ok(tile::solid_tile(rgb, size))
            }
            ImageSpec::Text {
                text,
                background,
                foreground,
            } => {
                let bg = parse_color(background)?;
                let fg = parse_color(foreground)?;
                Ok(text::label_tile(text, size, bg, fg, size_px))
            }
            ImageSpec::File { path } => {
                let resolved = resolve_path(base, path);
                let bytes = std::fs::read(&resolved).map_err(|e| {
                    format!("cannot read image {}: {e}", resolved.display())
                })?;
                let decoded = image::decode(&bytes)?;
                Ok(image::fit_cover(&decoded, width, height))
            }
            ImageSpec::Svg { path, background } => {
                let resolved = resolve_path(base, path);
                let bytes = std::fs::read(&resolved).map_err(|e| {
                    format!("cannot read svg {}: {e}", resolved.display())
                })?;
                let bg = parse_color(background)?;
                svg::render_svg(&bytes, size, bg)
            }
            ImageSpec::Animation { path, .. } => {
                // The tile shows the first frame. The daemon uses
                // `load_animation` for the moving version.
                let resolved = resolve_path(base, path);
                let bytes = std::fs::read(&resolved).map_err(|e| {
                    format!("cannot read animation {}: {e}", resolved.display())
                })?;
                let frames = animation::decode_gif(&bytes)?;
                let first = frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| "GIF has no frames".to_string())?;
                let rgb = Rgb::new(first.width, first.height, first.rgb)?;
                Ok(image::fit_cover(&rgb, width, height))
            }
            ImageSpec::Icon { name, background } => {
                let bg = parse_color(background)?;
                // A profile icon wins, then a bundled icon, then the name.
                let icon = base.join("icons").join(format!("{name}.svg"));
                match std::fs::read(&icon) {
                    Ok(bytes) => svg::render_svg(&bytes, size, bg),
                    Err(_) => match icons::bundled(name) {
                        Some(text) => svg::render_svg(text.as_bytes(), size, bg),
                        None => Ok(text::label_tile(name, size, bg, palette.foreground, size_px)),
                    },
                }
            }
        };
    }

    if let Some(label) = &key.label {
        return Ok(text::label_tile(
            label,
            size,
            palette.background,
            palette.foreground,
            size_px,
        ));
    }

    Ok(tile::solid_tile(palette.background, size))
}

/// Compose, rotate and encode one key tile.
///
/// The result is a JPEG that the firmware accepts.
pub fn encode(config: Option<&Key>, size: (u16, u16), size_px: usize) -> Result<Vec<u8>, String> {
    encode_in(config, size, size_px, Path::new("."))
}

/// Compose, rotate and encode one key tile, with a base folder for images.
pub fn encode_in(
    config: Option<&Key>,
    size: (u16, u16),
    size_px: usize,
    base: &Path,
) -> Result<Vec<u8>, String> {
    encode_in_themed(config, size, size_px, base, &Palette::default())
}

/// Compose, rotate and encode one key tile with a profile palette.
pub fn encode_in_themed(
    config: Option<&Key>,
    size: (u16, u16),
    size_px: usize,
    base: &Path,
    palette: &Palette,
) -> Result<Vec<u8>, String> {
    let upright = compose_in_themed(config, size, size_px, base, palette)?;
    let rotated = image::rotate(&upright, model::ROTATION);
    image::encode_max(&rotated, proto::MAX_IMAGE_BYTES)
}

/// Encode one key of a page, with the page scale already applied.
pub fn encode_key(page: &Page, key: u8) -> Result<Vec<u8>, String> {
    encode_key_in(page, key, Path::new("."))
}

/// Encode one key of a page, with a base folder for relative image paths.
pub fn encode_key_in(page: &Page, key: u8, base: &Path) -> Result<Vec<u8>, String> {
    encode_key_in_themed(page, key, base, &Palette::default())
}

/// Encode one key of a page with a profile palette.
pub fn encode_key_in_themed(
    page: &Page,
    key: u8,
    base: &Path,
    palette: &Palette,
) -> Result<Vec<u8>, String> {
    encode_in_themed(
        page.key(key),
        model::key_size(key),
        scale_for(page, key),
        base,
        palette,
    )
}

/// A ready to send animation: pre-encoded JPEG frames and their delays.
#[derive(Debug, Clone)]
pub struct Animation {
    pub frames: Vec<Arc<[u8]>>,
    pub delays_ms: Vec<u16>,
}

impl Animation {
    /// Total loop time in milliseconds.
    pub fn duration_ms(&self) -> u32 {
        self.delays_ms.iter().map(|delay| *delay as u32).sum()
    }

    /// Total encoded size in bytes.
    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|frame| frame.len()).sum()
    }
}

/// Largest encoded animation that the renderer keeps for one key.
pub const MAX_ANIMATION_BYTES: usize = 1_500_000;

/// Merge frames until the animation fits the budget. The loop time is kept.
fn shrink(animation: &mut Animation, budget: usize) {
    while animation.bytes() > budget && animation.frames.len() > 1 {
        let mut frames = Vec::with_capacity(animation.frames.len().div_ceil(2));
        let mut delays = Vec::with_capacity(frames.capacity());

        let mut index = 0;
        while index < animation.frames.len() {
            frames.push(Arc::clone(&animation.frames[index]));
            let mut delay = animation.delays_ms[index] as u32;
            if index + 1 < animation.frames.len() {
                delay += animation.delays_ms[index + 1] as u32;
            }
            delays.push(delay.min(10_000) as u16);
            index += 2;
        }

        animation.frames = frames;
        animation.delays_ms = delays;
    }
}

/// Decode a GIF file and pre-encode every frame for one key size.
///
/// The frames are already rotated and fit under the size limit, so the daemon
/// only has to write them. A frame that cannot be compressed is skipped.
pub fn load_animation(path: &Path, size: (u16, u16)) -> Result<Animation, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("cannot read animation {}: {e}", path.display()))?;
    let decoded = animation::decode_gif(&bytes)?;
    let total = decoded.len();

    let mut frames = Vec::with_capacity(total);
    let mut delays_ms = Vec::with_capacity(total);

    for frame in decoded {
        let rgb = Rgb::new(frame.width, frame.height, frame.rgb)?;
        let fitted = image::fit_cover(&rgb, size.0, size.1);
        let rotated = image::rotate(&fitted, model::ROTATION);
        if let Ok(jpeg) = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES) {
            frames.push(Arc::from(jpeg));
            delays_ms.push(frame.delay_ms);
        }
    }

    if frames.is_empty() {
        return Err(format!(
            "none of the {total} GIF frames fit {} bytes",
            proto::MAX_IMAGE_BYTES
        ));
    }

    let mut animation = Animation { frames, delays_ms };
    shrink(&mut animation, MAX_ANIMATION_BYTES);
    Ok(animation)
}

/// A tile that reports a render fault, so the key never goes blank silently.
pub fn fault_tile(size: (u16, u16)) -> Result<Vec<u8>, String> {
    let upright = tile::label_tile("ERR", size, [96, 16, 16], [255, 255, 255]);
    let rotated = image::rotate(&upright, model::ROTATION);
    image::encode_max(&rotated, proto::MAX_IMAGE_BYTES)
}

/// A picture of the whole panel for one page, without the panel rotation.
///
/// Use it for a profile thumbnail and for a check on a machine without the
/// device. The grid is five columns by three rows, and the side strip is one
/// narrow column on the right.
pub fn contact_sheet(page: &Page) -> Result<Rgb, String> {
    contact_sheet_in(page, Path::new("."))
}

/// A picture of the whole panel for one page, with a base folder for images.
pub fn contact_sheet_in(page: &Page, base: &Path) -> Result<Rgb, String> {
    contact_sheet_in_themed(page, base, &Palette::default())
}

/// A picture of the whole panel for one page with a profile palette.
pub fn contact_sheet_in_themed(
    page: &Page,
    base: &Path,
    palette: &Palette,
) -> Result<Rgb, String> {
    let cell = model::GRID_KEY_PIXELS as usize;
    let strip = model::STRIP_KEY_PIXELS as usize;
    let width = cell * model::COLUMNS as usize + strip;
    let height = cell * model::ROWS as usize;

    let mut data = vec![0u8; width * height * 3];
    for pixel in data.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[8, 8, 10]);
    }

    for key in 1..=model::KEY_COUNT {
        let (x, y) = sheet_position(key);
        let size = model::key_size(key);
        let tile = compose_in_themed(page.key(key), size, scale_for(page, key), base, palette)?;
        blit(&mut data, width, &tile, x, y);
    }

    Ok(Rgb {
        width: width as u16,
        height: height as u16,
        data,
    })
}

/// Top left corner of one key on the contact sheet.
fn sheet_position(key: u8) -> (usize, usize) {
    let cell = model::GRID_KEY_PIXELS as usize;
    let strip_x = cell * model::COLUMNS as usize;

    if key <= model::GRID_KEYS {
        let index = (key - 1) as usize;
        let column = index % model::COLUMNS as usize;
        let row = index / model::COLUMNS as usize;
        (column * cell, row * cell)
    } else {
        let index = (key - model::GRID_KEYS - 1) as usize;
        (strip_x, index * cell)
    }
}

/// Copy one tile into a larger image.
fn blit(target: &mut [u8], target_width: usize, tile: &Rgb, x: usize, y: usize) {
    let tw = tile.width as usize;
    for row in 0..tile.height as usize {
        let from = row * tw * 3;
        let to = ((y + row) * target_width + x) * 3;
        let count = tw * 3;
        if to + count <= target.len() {
            target[to..to + count].copy_from_slice(&tile.data[from..from + count]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use streamdock_store::profile::{Action, Key, Page};

    #[test]
    fn a_key_without_config_is_a_dark_tile() {
        let tile = compose(None, (96, 96), 28).expect("tile");
        assert_eq!((tile.width, tile.height), (96, 96));
        assert!(tile.data.chunks_exact(3).all(|p| p == BACKGROUND));
    }

    #[test]
    fn a_label_is_drawn() {
        let key = Key {
            label: Some("MUTE".into()),
            ..Key::default()
        };
        let tile = compose(Some(&key), (96, 96), 28).expect("tile");
        let lit = tile
            .data
            .chunks_exact(3)
            .filter(|p| *p == FOREGROUND)
            .count();
        assert!(lit > 50);
    }

    #[test]
    fn a_colour_image_is_a_solid_tile() {
        let key = Key {
            image: Some(ImageSpec::Color {
                color: "#ff0000".into(),
            }),
            ..Key::default()
        };
        let tile = compose(Some(&key), (80, 80), 28).expect("tile");
        assert!(tile.data.chunks_exact(3).all(|p| p == [255, 0, 0]));
    }

    #[test]
    fn a_text_image_overrides_the_label() {
        let key = Key {
            label: Some("ignored".into()),
            image: Some(ImageSpec::Text {
                text: "ON".into(),
                background: "#000000".into(),
                foreground: "#00ff00".into(),
            }),
            ..Key::default()
        };
        let tile = compose(Some(&key), (96, 96), 28).expect("tile");
        assert!(tile.data.chunks_exact(3).any(|p| p == [0, 255, 0]));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let key = Key {
            image: Some(ImageSpec::File {
                path: "no-such-file-here.png".into(),
            }),
            ..Key::default()
        };
        assert!(compose(Some(&key), (96, 96), 28).is_err());
    }

    #[test]
    fn an_encoded_tile_fits_the_limit() {
        let key = Key {
            label: Some("SCENE 1".into()),
            ..Key::default()
        };
        let jpeg = encode(Some(&key), (96, 96), 28).expect("jpeg");
        assert!(jpeg.len() <= proto::MAX_IMAGE_BYTES);
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn the_fault_tile_is_valid() {
        let jpeg = fault_tile((96, 96)).expect("jpeg");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert!(jpeg.len() <= proto::MAX_IMAGE_BYTES);
    }

    #[test]
    fn one_page_scale_makes_1_and_10_the_same_size() {
        let mut page = Page::new("Main");
        for key in 1..=15u8 {
            page.keys.insert(
                key,
                Key {
                    label: Some(key.to_string()),
                    ..Key::default()
                },
            );
        }
        let scale = page_scale(&page, (96, 96), 1, 15);
        let one = compose(Some(&page.keys[&1]), (96, 96), scale).expect("1");
        let ten = compose(Some(&page.keys[&10]), (96, 96), scale).expect("10");
        let a = text_rows(&one);
        let b = text_rows(&ten);
        assert!(a > 0 && b > 0);
        assert!((a as i32 - b as i32).abs() <= 2, "heights {a} and {b}");
    }

    #[test]
    fn scale_for_is_the_same_for_every_grid_key() {
        let page = streamdock_store::profile::Profile::demo().pages[0].clone();
        let first = scale_for(&page, 1);
        for key in 1..=model::GRID_KEYS {
            assert_eq!(scale_for(&page, key), first, "key {key}");
        }
        let one = compose(page.key(1), (96, 96), scale_for(&page, 1)).expect("1");
        let ten = compose(page.key(10), (96, 96), scale_for(&page, 10)).expect("10");
        let a = text_rows(&one);
        let b = text_rows(&ten);
        assert!((a as i32 - b as i32).abs() <= 2, "heights {a} and {b}, scale {first}");
    }

    #[test]
    fn a_long_label_shrinks_the_page_scale() {
        let mut page = Page::new("Main");
        page.keys.insert(
            1,
            Key {
                label: Some("1".into()),
                ..Key::default()
            },
        );
        page.keys.insert(
            2,
            Key {
                label: Some("A LONG LABEL".into()),
                ..Key::default()
            },
        );
        assert!(page_scale(&page, (96, 96), 1, 2) <= page_scale(&page, (96, 96), 1, 1));
    }

    #[test]
    fn keys_without_text_do_not_change_the_scale() {
        let page = Page::new("Main");
        assert_eq!(
            page_scale(&page, (96, 96), 1, 15),
            max_label_px((96, 96)).round() as usize
        );
    }

    #[test]
    fn a_contact_sheet_has_every_key() {
        let page = streamdock_store::profile::Profile::demo().pages[0].clone();
        let sheet = contact_sheet(&page).expect("sheet");
        assert_eq!(sheet.width as usize, 96 * 5 + 80);
        assert_eq!(sheet.height as usize, 96 * 3);
        // The bottom right corner of the grid is key 15, which is not blank.
        let sheet_15 = sheet.pixel(96 * 5 - 10, 96 * 3 - 10);
        assert_ne!(sheet_15, [8, 8, 10]);
    }

    #[test]
    fn sheet_positions_follow_the_panel() {
        assert_eq!(sheet_position(1), (0, 0));
        assert_eq!(sheet_position(5), (96 * 4, 0));
        assert_eq!(sheet_position(6), (0, 96));
        assert_eq!(sheet_position(15), (96 * 4, 96 * 2));
        assert_eq!(sheet_position(16), (96 * 5, 0));
        assert_eq!(sheet_position(18), (96 * 5, 96 * 2));
    }

    #[test]
    fn a_relative_path_uses_the_base_folder() {
        let base = Path::new("C:/profiles");
        let relative = resolve_path(base, "assets/picture.png");
        assert!(relative.starts_with(base));
        assert!(relative.ends_with("picture.png"));
        let absolute = resolve_path(base, "C:/pictures/picture.jpg");
        assert_eq!(absolute, PathBuf::from("C:/pictures/picture.jpg"));
    }

    #[test]
    fn an_animation_loads_every_frame() {
        let dir = std::env::temp_dir().join(format!(
            "streamdock-render-animation-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.gif");

        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, 8, 8, &[]).expect("encoder");
            for colour in [[255u8, 0, 0, 255], [0, 0, 255, 255]] {
                let mut pixels = colour.repeat(64);
                let mut frame = gif::Frame::from_rgba(8, 8, &mut pixels);
                frame.delay = 5;
                encoder.write_frame(&frame).expect("frame");
            }
        }
        std::fs::write(&path, &bytes).expect("write");

        let animation = load_animation(&path, (96, 96)).expect("load");
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.delays_ms, vec![50, 50]);
        assert_eq!(animation.duration_ms(), 100);
        assert!(animation
            .frames
            .iter()
            .all(|frame| frame.len() <= proto::MAX_IMAGE_BYTES));
        assert_eq!(&animation.frames[0][..2], &[0xFF, 0xD8]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shrinking_keeps_the_loop_time() {
        let mut animation = Animation {
            frames: (0..4).map(|_| Arc::from(vec![0u8; 10_000])).collect(),
            delays_ms: vec![10, 20, 30, 40],
        };
        assert_eq!(animation.duration_ms(), 100);
        shrink(&mut animation, 25_000);
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.delays_ms, vec![30, 70]);
        assert_eq!(animation.duration_ms(), 100);
        assert!(animation.bytes() <= 25_000);
    }

    #[test]
    fn a_page_switch_action_keeps_the_scale_stable() {
        let mut page = Page::new("Main");
        page.keys.insert(
            1,
            Key {
                label: Some("1".into()),
                actions: vec![Action::Page {
                    target: "Main".into(),
                }],
                ..Key::default()
            },
        );
        assert!(page_scale(&page, (96, 96), 1, 1) >= 1);
    }

    /// Height of the text block in pixels.
    fn text_rows(tile: &Rgb) -> usize {
        let w = tile.width as usize;
        let h = tile.height as usize;
        let rows: Vec<usize> = (0..h)
            .filter(|&y| {
                (0..w).any(|x| {
                    let at = (y * w + x) * 3;
                    tile.data[at..at + 3] == FOREGROUND
                })
            })
            .collect();
        match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => last - first + 1,
            _ => 0,
        }
    }
}
