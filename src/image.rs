//! JPEG decode, rotate and encode for the key displays.
//!
//! The firmware shows the image rotated relative to the file. The model data
//! gives the rotation that the panel expects.

use jpeg_decoder::PixelFormat;

/// One decoded image in 8 bit RGB.
pub struct Rgb {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

/// Decode a JPEG file into RGB pixels.
pub fn decode(bytes: &[u8]) -> Result<Rgb, String> {
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
                let (c, m, y, k) = (p[0] as u16, p[1] as u16, p[2] as u16, p[3] as u16);
                let f = |v: u16| (255 - (v * (255 - k.min(255)) / 255).min(255)) as u8;
                [f(c), f(m), f(y)]
            })
            .collect(),
    };

    Ok(Rgb {
        width,
        height,
        data,
    })
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
        _ => Rgb {
            width: src.width,
            height: src.height,
            data: src.data.clone(),
        },
    }
}

/// Encode the image as JPEG and lower the quality until it fits `max_bytes`.
pub fn encode_max(image: &Rgb, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut quality = 90i32;
    loop {
        let mut out = Vec::new();
        let encoder = jpeg_encoder::Encoder::new(&mut out, quality as u8);
        encoder
            .encode(
                &image.data,
                image.width,
                image.height,
                jpeg_encoder::ColorType::Rgb,
            )
            .map_err(|e| format!("encode: {e}"))?;

        if out.len() <= max_bytes {
            return Ok(out);
        }
        if quality <= 20 {
            return Err(format!(
                "cannot compress the image to {max_bytes} bytes, smallest try was {} bytes",
                out.len()
            ));
        }
        quality -= 10;
    }
}
