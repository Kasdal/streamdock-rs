//! JPEG decode, rotate, scale and encode for the key displays.
//!
//! The firmware shows the image rotated relative to the file. The model data
//! gives the rotation that the panel expects.
//!
//! Order of operations, measured on hardware: compose the tile upright, rotate
//! it, then encode. Do not rotate after the encode.

use jpeg_decoder::PixelFormat;

/// One decoded image in 8 bit RGB.
#[derive(Clone)]
pub struct Rgb {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

impl Rgb {
    /// Make an image from RGB bytes.
    pub fn new(width: u16, height: u16, data: Vec<u8>) -> Result<Self, String> {
        if data.len() != width as usize * height as usize * 3 {
            return Err(format!(
                "{width}x{height} needs {} bytes, got {}",
                width as usize * height as usize * 3,
                data.len()
            ));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// The pixel at one position.
    pub fn pixel(&self, x: u16, y: u16) -> [u8; 3] {
        let at = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[at], self.data[at + 1], self.data[at + 2]]
    }
}

/// Decode a JPEG or PNG file into RGB pixels.
pub fn decode(bytes: &[u8]) -> Result<Rgb, String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        decode_png(bytes)
    } else {
        Err("not a JPEG or PNG file".into())
    }
}

/// Decode one JPEG file into RGB pixels.
fn decode_jpeg(bytes: &[u8]) -> Result<Rgb, String> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let pixels = decoder.decode().map_err(|e| format!("decode: {e}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| "decode: no image info".to_string())?;

    let (width, height) = (info.width, info.height);
    let data = match info.pixel_format {
        PixelFormat::RGB24 => pixels,
        PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g]).collect(),
        PixelFormat::L16 => pixels
            .chunks_exact(2)
            .flat_map(|p| {
                let g = p[0];
                [g, g, g]
            })
            .collect(),
        PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .flat_map(|p| {
                // Stored inverted: 0 is full ink, 255 is no ink.
                let k = p[3] as u32;
                let f = |v: u8| ((v as u32 * k) / 255) as u8;
                [f(p[0]), f(p[1]), f(p[2])]
            })
            .collect(),
    };

    Rgb::new(width, height, data)
}

/// Largest PNG side that the decoder accepts, in pixels.
pub const MAX_PNG_SIDE: u32 = 4096;

/// Decode one PNG file into RGB pixels.
///
/// The alpha channel is dropped. A key tile is opaque, so the profile
/// background shows through only when the renderer draws it.
fn decode_png(bytes: &[u8]) -> Result<Rgb, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("decode png: {e}"))?;

    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("decode png: {e}"))?;

    if info.width > MAX_PNG_SIDE || info.height > MAX_PNG_SIDE {
        return Err(format!(
            "PNG is {}x{}, the limit is {MAX_PNG_SIDE}x{MAX_PNG_SIDE}",
            info.width, info.height
        ));
    }

    let pixels = &buffer[..info.buffer_size()];
    let data = match info.color_type {
        png::ColorType::Rgb => pixels.to_vec(),
        png::ColorType::Rgba => pixels
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g]).collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0]])
            .collect(),
        png::ColorType::Indexed => {
            return Err("indexed PNG is not supported, save it as RGB".into())
        }
    };

    Rgb::new(info.width as u16, info.height as u16, data)
}

/// Rotate the image 90 degrees clockwise.
pub fn rotate_cw(src: &Rgb) -> Rgb {
    let (w, h) = (src.width as usize, src.height as usize);
    let mut data = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let from = (y * w + x) * 3;
            let dx = h - 1 - y;
            let dy = x;
            let to = (dy * h + dx) * 3;
            data[to..to + 3].copy_from_slice(&src.data[from..from + 3]);
        }
    }
    Rgb {
        width: h as u16,
        height: w as u16,
        data,
    }
}

/// Rotate the image by a multiple of 90 degrees clockwise.
pub fn rotate(src: &Rgb, degrees: u16) -> Rgb {
    match degrees % 360 {
        90 => rotate_cw(src),
        180 => rotate_cw(&rotate_cw(src)),
        270 => rotate_cw(&rotate_cw(&rotate_cw(src))),
        _ => src.clone(),
    }
}

/// Scale the image to an exact size with bilinear sampling.
pub fn resize(src: &Rgb, width: u16, height: u16) -> Rgb {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (dw, dh) = (width as usize, height as usize);
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Rgb {
            width,
            height,
            data: vec![0u8; dw * dh * 3],
        };
    }

    let mut data = vec![0u8; dw * dh * 3];
    for y in 0..dh {
        let sy = ((y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5).max(0.0);
        let y0 = (sy.floor() as usize).min(sh - 1);
        let y1 = (y0 + 1).min(sh - 1);
        let fy = sy - y0 as f32;

        for x in 0..dw {
            let sx = ((x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5).max(0.0);
            let x0 = (sx.floor() as usize).min(sw - 1);
            let x1 = (x0 + 1).min(sw - 1);
            let fx = sx - x0 as f32;

            let at = (y * dw + x) * 3;
            for c in 0..3 {
                let p00 = src.data[(y0 * sw + x0) * 3 + c] as f32;
                let p10 = src.data[(y0 * sw + x1) * 3 + c] as f32;
                let p01 = src.data[(y1 * sw + x0) * 3 + c] as f32;
                let p11 = src.data[(y1 * sw + x1) * 3 + c] as f32;

                let top = p00 + (p10 - p00) * fx;
                let bottom = p01 + (p11 - p01) * fx;
                data[at + c] = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    Rgb {
        width,
        height,
        data,
    }
}

/// Scale the image to cover the target size, then keep the centre.
///
/// The result keeps the aspect ratio and loses the outer parts of the long
/// side. This is what a preview tile needs.
pub fn fit_cover(src: &Rgb, width: u16, height: u16) -> Rgb {
    let (sw, sh) = (src.width as f32, src.height as f32);
    if sw == 0.0 || sh == 0.0 {
        return resize(src, width, height);
    }

    let scale = (width as f32 / sw).max(height as f32 / sh);
    let scaled_w = (sw * scale).round().max(1.0) as u16;
    let scaled_h = (sh * scale).round().max(1.0) as u16;
    let scaled = resize(src, scaled_w, scaled_h);

    let x0 = (scaled.width.saturating_sub(width)) / 2;
    let y0 = (scaled.height.saturating_sub(height)) / 2;
    crop(&scaled, x0, y0, width, height)
}

/// Cut a rectangle out of an image. Areas outside the source are black.
pub fn crop(src: &Rgb, x0: u16, y0: u16, width: u16, height: u16) -> Rgb {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let mut data = vec![0u8; width as usize * height as usize * 3];
    for y in 0..height as usize {
        let sy = y0 as usize + y;
        if sy >= sh {
            break;
        }
        for x in 0..width as usize {
            let sx = x0 as usize + x;
            if sx >= sw {
                break;
            }
            let from = (sy * sw + sx) * 3;
            let to = (y * width as usize + x) * 3;
            data[to..to + 3].copy_from_slice(&src.data[from..from + 3]);
        }
    }
    Rgb {
        width,
        height,
        data,
    }
}

/// Encode the image as JPEG at one quality.
pub fn encode_quality(image: &Rgb, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut out, quality);
    encoder
        .encode(
            &image.data,
            image.width,
            image.height,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| format!("encode: {e}"))?;
    Ok(out)
}

/// Encode the image as JPEG and lower the quality until it fits `max_bytes`.
///
/// Returns the highest quality that fits. The search starts at 90 and steps
/// down by 5.
pub fn encode_max(image: &Rgb, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut smallest = usize::MAX;
    for quality in (10..=90).rev().step_by(5) {
        let out = encode_quality(image, quality as u8)?;
        if out.len() <= max_bytes {
            return Ok(out);
        }
        smallest = smallest.min(out.len());
    }

    Err(format!(
        "cannot compress the image to {max_bytes} bytes, the smallest try was {smallest} bytes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_one() -> Rgb {
        Rgb {
            width: 2,
            height: 1,
            data: vec![255, 0, 0, 0, 255, 0],
        }
    }

    fn test_image(width: u16, height: u16) -> Rgb {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                data.push((x * 7 % 256) as u8);
                data.push((y * 11 % 256) as u8);
                data.push(((x + y) * 3 % 256) as u8);
            }
        }
        Rgb {
            width,
            height,
            data,
        }
    }

    #[test]
    fn rotate_90_swaps_the_sides() {
        let out = rotate(&two_by_one(), 90);
        assert_eq!((out.width, out.height), (1, 2));
        assert_eq!(out.pixel(0, 0), [255, 0, 0]);
        assert_eq!(out.pixel(0, 1), [0, 255, 0]);
    }

    #[test]
    fn four_rotations_are_the_identity() {
        let src = test_image(7, 5);
        let back = rotate(&src, 90);
        let back = rotate(&back, 90);
        let back = rotate(&back, 90);
        let back = rotate(&back, 90);
        assert_eq!((back.width, back.height), (7, 5));
        assert_eq!(back.data, src.data);
    }

    #[test]
    fn rotation_270_is_one_quarter_the_other_way() {
        let src = test_image(3, 2);
        let out = rotate(&src, 270);
        assert_eq!((out.width, out.height), (2, 3));
        // 270 clockwise is 90 anticlockwise: the top right pixel of the
        // source becomes the top left of the result.
        assert_eq!(out.pixel(0, 0), src.pixel(src.width - 1, 0));
        assert_eq!(out.pixel(1, 0), src.pixel(src.width - 1, 1));
    }

    #[test]
    fn rotation_zero_copies() {
        let src = test_image(4, 4);
        let out = rotate(&src, 0);
        assert_eq!(out.data, src.data);
    }

    #[test]
    fn resize_keeps_a_solid_colour() {
        let src = Rgb {
            width: 4,
            height: 4,
            data: vec![10, 20, 30].repeat(16),
        };
        let out = resize(&src, 9, 7);
        assert_eq!((out.width, out.height), (9, 7));
        assert!(out.data.chunks_exact(3).all(|p| p == [10, 20, 30]));
    }

    #[test]
    fn fit_cover_gives_the_exact_size() {
        let src = test_image(100, 40);
        let out = fit_cover(&src, 96, 96);
        assert_eq!((out.width, out.height), (96, 96));
        assert_eq!(out.data.len(), 96 * 96 * 3);
    }

    #[test]
    fn fit_cover_of_a_tall_image_also_fits() {
        let src = test_image(40, 100);
        let out = fit_cover(&src, 80, 80);
        assert_eq!((out.width, out.height), (80, 80));
    }

    #[test]
    fn crop_reads_the_requested_pixels() {
        let src = test_image(10, 10);
        let out = crop(&src, 2, 3, 4, 5);
        assert_eq!((out.width, out.height), (4, 5));
        assert_eq!(out.pixel(0, 0), src.pixel(2, 3));
    }

    #[test]
    fn encode_returns_a_jpeg_under_the_limit() {
        let src = fit_cover(&test_image(200, 200), 96, 96);
        let jpeg = encode_max(&src, 10_240).expect("encode");
        assert!(jpeg.len() <= 10_240, "got {} bytes", jpeg.len());
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let back = decode(&jpeg).expect("decode the result");
        assert_eq!((back.width, back.height), (96, 96));
    }

    #[test]
    fn encode_fails_when_the_limit_is_impossible() {
        let src = test_image(96, 96);
        assert!(encode_max(&src, 32).is_err());
    }

    #[test]
    fn new_rejects_a_wrong_length() {
        assert!(Rgb::new(2, 2, vec![0; 11]).is_err());
        assert!(Rgb::new(2, 2, vec![0; 12]).is_ok());
    }

    #[test]
    fn png_round_trips() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 3);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            let pixels: Vec<u8> = (0..36u32).map(|i| (i * 7 % 256) as u8).collect();
            writer.write_image_data(&pixels).expect("data");
            writer.finish().expect("finish");
        }
        let decoded = decode(&bytes).expect("decode png");
        assert_eq!((decoded.width, decoded.height), (4, 3));
        assert_eq!(decoded.data.len(), 4 * 3 * 3);
        assert_eq!(decoded.data[0], 0);
        assert_eq!(decoded.data[3], 21);
    }

    #[test]
    fn a_png_with_alpha_drops_the_alpha_channel() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[10, 20, 30, 128]).expect("data");
            writer.finish().expect("finish");
        }
        let decoded = decode(&bytes).expect("decode png");
        assert_eq!(decoded.pixel(0, 0), [10, 20, 30]);
    }

    #[test]
    fn unknown_formats_are_rejected() {
        assert!(decode(b"not an image").is_err());
    }
}
