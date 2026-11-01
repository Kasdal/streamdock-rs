//! Animated GIF decoding.
//!
//! The decoder produces one flat RGB frame per GIF frame. The key tiles are
//! opaque, so a transparent pixel shows black. Frames are composited onto a
//! black canvas, and the GIF disposal method is applied between frames.
//!
//! This module does no scaling and no JPEG work. The render crate fits each
//! frame to the key size and encodes it.

use std::io::Cursor;

/// Longest GIF side that the decoder accepts, in pixels.
pub const MAX_GIF_SIDE: u16 = 4096;

/// Most frames that the decoder keeps.
///
/// A long GIF is sampled down to this count, and the delays of the skipped
/// frames are added to the frames that stay, so the loop keeps its duration.
pub const MAX_FRAMES: usize = 60;

/// Shortest frame delay that the panel uses, in milliseconds.
pub const MIN_DELAY_MS: u16 = 20;

/// Longest frame delay that the panel uses, in milliseconds.
pub const MAX_DELAY_MS: u16 = 2000;

/// Default delay when a GIF frame declares zero.
pub const DEFAULT_DELAY_MS: u16 = 100;

/// One decoded frame, already flattened onto black.
#[derive(Debug, Clone)]
pub struct GifFrame {
    pub width: u16,
    pub height: u16,
    /// RGB bytes, three per pixel.
    pub rgb: Vec<u8>,
    /// How long the frame stays on screen.
    pub delay_ms: u16,
}

/// Decode every frame of a GIF file.
pub fn decode_gif(bytes: &[u8]) -> Result<Vec<GifFrame>, String> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);

    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| format!("decode gif: {e}"))?;

    let width = decoder.width();
    let height = decoder.height();
    if width == 0 || height == 0 {
        return Err("GIF has no pixels".into());
    }
    if width > MAX_GIF_SIDE || height > MAX_GIF_SIDE {
        return Err(format!(
            "GIF is {width}x{height}, the limit is {MAX_GIF_SIDE}x{MAX_GIF_SIDE}"
        ));
    }

    let stride = width as usize;
    let mut canvas = vec![0u8; stride * height as usize * 3];
    let mut frames: Vec<GifFrame> = Vec::new();

    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|e| format!("decode gif: {e}"))?
    {
        let left = frame.left as usize;
        let top = frame.top as usize;
        let frame_width = frame.width as usize;
        let frame_height = frame.height as usize;

        if left + frame_width > stride || top + frame_height > height as usize {
            return Err("GIF frame is outside the image".into());
        }

        let saved = if frame.dispose == gif::DisposalMethod::Previous {
            Some(copy_rect(&canvas, stride, left, top, frame_width, frame_height))
        } else {
            None
        };

        draw_frame(&mut canvas, stride, frame, left, top);

        frames.push(GifFrame {
            width,
            height,
            rgb: canvas.clone(),
            delay_ms: delay_ms(frame.delay),
        });

        match frame.dispose {
            gif::DisposalMethod::Background => {
                clear_rect(&mut canvas, stride, left, top, frame_width, frame_height);
            }
            gif::DisposalMethod::Previous => {
                if let Some(saved) = saved {
                    restore_rect(&mut canvas, stride, left, top, frame_width, frame_height, &saved);
                }
            }
            _ => {}
        }
    }

    if frames.is_empty() {
        return Err("GIF has no frames".into());
    }

    Ok(sample(frames))
}

/// Blend one RGBA frame onto the RGB canvas.
fn draw_frame(canvas: &mut [u8], stride: usize, frame: &gif::Frame, left: usize, top: usize) {
    let frame_width = frame.width as usize;
    for y in 0..frame.height as usize {
        for x in 0..frame_width {
            let src = (y * frame_width + x) * 4;
            let alpha = frame.buffer[src + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let dst = ((top + y) * stride + left + x) * 3;
            if alpha == 255 {
                canvas[dst] = frame.buffer[src];
                canvas[dst + 1] = frame.buffer[src + 1];
                canvas[dst + 2] = frame.buffer[src + 2];
            } else {
                for channel in 0..3 {
                    let src_value = frame.buffer[src + channel] as u32;
                    let dst_value = canvas[dst + channel] as u32;
                    canvas[dst + channel] =
                        ((src_value * alpha + dst_value * (255 - alpha)) / 255) as u8;
                }
            }
        }
    }
}

fn copy_rect(
    canvas: &[u8],
    stride: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        let start = ((top + y) * stride + left) * 3;
        out.extend_from_slice(&canvas[start..start + width * 3]);
    }
    out
}

fn restore_rect(
    canvas: &mut [u8],
    stride: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    saved: &[u8],
) {
    for y in 0..height {
        let start = ((top + y) * stride + left) * 3;
        let from = y * width * 3;
        canvas[start..start + width * 3].copy_from_slice(&saved[from..from + width * 3]);
    }
}

fn clear_rect(
    canvas: &mut [u8],
    stride: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) {
    for y in 0..height {
        let start = ((top + y) * stride + left) * 3;
        canvas[start..start + width * 3].fill(0);
    }
}

/// Turn a GIF delay in centiseconds into milliseconds.
fn delay_ms(centiseconds: u16) -> u16 {
    if centiseconds == 0 {
        return DEFAULT_DELAY_MS;
    }
    (centiseconds as u32 * 10).clamp(MIN_DELAY_MS as u32, MAX_DELAY_MS as u32) as u16
}

/// Keep at most `MAX_FRAMES` frames and preserve the loop duration.
fn sample(frames: Vec<GifFrame>) -> Vec<GifFrame> {
    if frames.len() <= MAX_FRAMES {
        return frames;
    }

    let step = frames.len().div_ceil(MAX_FRAMES);
    let mut out = Vec::with_capacity(MAX_FRAMES);
    let mut pending_delay: u32 = 0;

    for (index, frame) in frames.into_iter().enumerate() {
        pending_delay += frame.delay_ms as u32;
        if index % step == step - 1 {
            let mut kept = frame;
            kept.delay_ms = pending_delay.clamp(MIN_DELAY_MS as u32, 10_000) as u16;
            pending_delay = 0;
            out.push(kept);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a GIF with the given frames and delays.
    fn build_gif(width: u16, height: u16, frames: &[(Vec<u8>, u16)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, width, height, &[]).expect("encoder");
            for (rgba, delay) in frames {
                let mut pixels = rgba.clone();
                let mut frame = gif::Frame::from_rgba(width, height, &mut pixels);
                frame.delay = *delay;
                encoder.write_frame(&frame).expect("frame");
            }
        }
        bytes
    }

    #[test]
    fn a_gif_keeps_its_frames_and_delays() {
        let red = vec![255, 0, 0, 255].repeat(4);
        let blue = vec![0, 0, 255, 255].repeat(4);
        let bytes = build_gif(2, 2, &[(red, 5), (blue, 7)]);

        let frames = decode_gif(&bytes).expect("decode");
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].width, frames[0].height), (2, 2));
        assert_eq!(frames[0].delay_ms, 50);
        assert_eq!(frames[1].delay_ms, 70);
        assert_eq!(frames[0].rgb.len(), 2 * 2 * 3);
        assert_eq!(&frames[0].rgb[..3], &[255, 0, 0]);
        assert_eq!(&frames[1].rgb[..3], &[0, 0, 255]);
    }

    #[test]
    fn a_zero_delay_gets_the_default() {
        assert_eq!(delay_ms(0), DEFAULT_DELAY_MS);
        assert_eq!(delay_ms(1), 20);
        assert_eq!(delay_ms(500), 2000);
    }

    #[test]
    fn a_bad_file_is_rejected() {
        assert!(decode_gif(b"not a gif").is_err());
    }

    #[test]
    fn sampling_keeps_the_loop_duration() {
        let frames: Vec<GifFrame> = (0..200)
            .map(|_| GifFrame {
                width: 1,
                height: 1,
                rgb: vec![0, 0, 0],
                delay_ms: 30,
            })
            .collect();
        let sampled = sample(frames);
        assert!(sampled.len() <= MAX_FRAMES);
        let total: u32 = sampled.iter().map(|f| f.delay_ms as u32).sum();
        assert_eq!(total, 200 * 30);
    }
}
