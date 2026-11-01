//! Stream Dock report framing.
//!
//! Measured facts (VID 0x0300 / PID 0x3010, firmware string
//! "HOTSPOTEKUSB HID DEMO", HID caps dump from this crate):
//!
//!   vendor collection   usage page 0xFFA0, usage 0x0001
//!     input report      513 bytes = 1 report ID slot + 512 payload
//!       byte 0            report ID slot, always 0x00 (no numbered reports)
//!       byte 1            usage 0xFFA1:0x0003, 1 byte
//!       byte 2..513       usage 0xFFA1:0x0004, 511 bytes of 8 bit values
//!     output report    1025 bytes = 1 report ID slot + 1024 payload
//!       byte 0            report ID slot, always 0x00
//!       byte 1            usage 0xFFA1:0x0005, 1 byte
//!       byte 2..1025      usage 0xFFA1:0x0006, 1023 bytes of 8 bit values
//!     feature report      0 bytes
//!
//!   keyboard collection usage page 0x0001, usage 0x0006
//!     Windows denies user-mode access to this collection.
//!     Key events arrive on the vendor collection.
//!
//! The command words below come from the public MiraBox Stream Dock driver in
//! the Bitfocus Companion project (file `src/streamdock.ts`). They are a
//! family protocol. This crate verifies them on the unit under test before it
//! claims support.

/// Report ID slot. The descriptor declares no numbered reports, so this byte
/// stays 0x00 for every input and output report.
pub const REPORT_ID: u8 = 0x00;

/// Command payload size in bytes. The report adds 1 report ID byte.
pub const PAYLOAD: usize = 1024;

/// Bytes that start every command payload: `CRT\0\0`.
pub const PREFIX: [u8; 5] = [0x43, 0x52, 0x54, 0x00, 0x00];

/// Offset of the key ID inside an input report.
/// The report starts with the report ID byte, so this is one more than the
/// offset that the Companion driver uses (it reads payloads without that byte).
pub const KEY_OFFSET: usize = 10;

/// Offset of the key state inside an input report.
pub const STATE_OFFSET: usize = 11;

/// A key report starts with `ACK\0\0` at offset 1.
pub const ACK: [u8; 3] = [0x41, 0x43, 0x4b];

/// Largest JPEG the firmware accepts for one key.
pub const MAX_IMAGE_BYTES: usize = 10_240;

/// One key event from the vendor collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: u8,
    pub pressed: bool,
}

/// Read one key event from an input report. Returns `None` for a short report
/// or for a report that does not start with the `ACK` marker.
pub fn parse_input(report: &[u8]) -> Option<KeyEvent> {
    if report.len() <= STATE_OFFSET || report[1..4] != ACK {
        return None;
    }
    Some(KeyEvent {
        key: report[KEY_OFFSET],
        pressed: report[STATE_OFFSET] != 0x00,
    })
}

/// Build one 1025 byte output report that carries `command` after the prefix.
pub fn frame(command: &[u8]) -> Vec<u8> {
    let mut report = vec![0u8; PAYLOAD + 1];
    report[0] = REPORT_ID;
    report[1..1 + PREFIX.len()].copy_from_slice(&PREFIX);
    let start = 1 + PREFIX.len();
    let end = (start + command.len()).min(report.len());
    report[start..end].copy_from_slice(&command[..end - start]);
    report
}

/// Build one 1025 byte output report from raw payload bytes. Image data uses
/// this form and needs no prefix.
pub fn chunk(bytes: &[u8]) -> Vec<u8> {
    let mut report = vec![0u8; PAYLOAD + 1];
    report[0] = REPORT_ID;
    let end = (1 + bytes.len()).min(report.len());
    report[1..end].copy_from_slice(&bytes[..end - 1]);
    report
}

/// Command words. The device answers one command per report.
pub mod cmd {
    /// Keep alive. The vendor driver sends this every 8 seconds.
    pub const HEARTBEAT: &[u8] = b"CONNECT";
    /// Wake the panel.
    pub const WAKE: &[u8] = b"DIS";
    /// Show the images that the host uploaded.
    pub const REFRESH: &[u8] = b"STP";

    /// Brightness value 0 to 100.
    pub fn brightness(percent: u8) -> Vec<u8> {
        vec![0x4c, 0x49, 0x47, 0x00, 0x00, percent.min(100)]
    }

    /// LED ring brightness value 0 to 100.
    pub fn led_brightness(percent: u8) -> Vec<u8> {
        vec![0x4c, 0x42, 0x4c, 0x49, 0x47, percent.min(100)]
    }

    /// Clear every key.
    pub fn clear_panel() -> Vec<u8> {
        vec![0x43, 0x4c, 0x45, 0x00, 0x00, 0x00, 0xff]
    }

    /// Clear one key.
    pub fn clear_key(key: u8) -> Vec<u8> {
        vec![0x43, 0x4c, 0x45, 0x00, 0x00, 0x00, key]
    }

    /// Header for an image. Send the JPEG bytes after it in `MAX_IMAGE_BYTES`
    /// chunks, then send `REFRESH`.
    pub fn image_header(key: u8, size: usize) -> Vec<u8> {
        let s = size as u32;
        vec![
            0x42,
            0x41,
            0x54,
            (s >> 24) as u8,
            (s >> 16) as u8,
            (s >> 8) as u8,
            s as u8,
            key,
        ]
    }

    /// Set an array of LED values.
    pub fn led_array(values: &[u8]) -> Vec<u8> {
        let mut v = vec![0x53, 0x45, 0x54, 0x4c, 0x42];
        v.extend_from_slice(values);
        v
    }

    /// Tell the device that the host closes the channel.
    pub fn close() -> Vec<u8> {
        vec![0x43, 0x4c, 0x45, 0x00, 0x00, 0x44, 0x43]
    }
}

/// Parse a hex string such as "02 03 ff" or "0203ff".
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '-')
        .collect();
    let cleaned = cleaned
        .strip_prefix("0x")
        .map(|s| s.to_string())
        .unwrap_or(cleaned);
    if cleaned.len() % 2 != 0 {
        return Err("hex string must have an even number of digits".into());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16).ok_or("bad hex digit")?;
        let lo = (pair[1] as char).to_digit(16).ok_or("bad hex digit")?;
        out.push((hi * 16 + lo) as u8);
    }
    Ok(out)
}

/// Format bytes as spaced hex for logs.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for b in bytes {
        s.push_str(&format!("{b:02x} "));
    }
    s.trim_end().to_string()
}
