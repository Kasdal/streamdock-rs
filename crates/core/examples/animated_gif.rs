//! Write a small animated GIF for a hardware test.
//!
//! Usage:
//!   cargo run --release --example animated_gif -- test-animation.gif
//!
//! The file has four frames of solid colour at 100 ms each. Drop it on a key
//! in the editor, or set `{ "kind": "animation", "path": "assets/..." }` in a
//! profile.

use std::fs::File;
use std::io::BufWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-animation.gif".to_string());

    let width: u16 = 96;
    let height: u16 = 96;
    let colours: [[u8; 4]; 4] = [
        [220, 40, 40, 255],
        [40, 200, 90, 255],
        [50, 120, 255, 255],
        [240, 240, 240, 255],
    ];

    let file = File::create(&path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), width, height, &[])?;

    for (index, colour) in colours.iter().enumerate() {
        let mut pixels = colour.repeat((width * height) as usize);
        let mut frame = gif::Frame::from_rgba(width, height, &mut pixels);
        frame.delay = 10; // 100 ms
        frame.dispose = gif::DisposalMethod::Background;
        encoder.write_frame(&frame)?;
        println!("frame {index}: {colour:?}");
    }

    println!("wrote {path}");
    Ok(())
}
