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
///
/// Measured: 15 keys with 9264 bytes each all show a complete image. A single
/// 17868 byte image shows correctly only when it is the first upload of a
/// session. Two or more images above about 10 KB leave the later keys
/// corrupt, with or without a pause between uploads.
///
/// Keep every image at or below 10 KB. The vendor driver uses the same rule.
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
    let take = bytes.len().min(PAYLOAD);
    report[1..1 + take].copy_from_slice(&bytes[..take]);
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

    /// Brightness through the curve that the vendor driver uses:
    /// `round((percent / 100) ^ 0.75 * 100)`.
    ///
    /// The curve is not measured on this unit yet. Compare it with
    /// `brightness` at 25, 50, 75 and 100 before you pick one.
    pub fn brightness_curved(percent: u8) -> Vec<u8> {
        brightness(crate::proto::map_brightness(percent))
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

/// Map a user percentage to the raw value that the vendor driver sends.
pub fn map_brightness(percent: u8) -> u8 {
    let p = percent.min(100) as f32 / 100.0;
    (p.powf(0.75) * 100.0).round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_key(report: &[u8]) -> u8 {
        report[KEY_OFFSET]
    }

    #[test]
    fn frame_starts_with_the_prefix() {
        let report = frame(b"DIS");
        assert_eq!(report.len(), PAYLOAD + 1);
        assert_eq!(report[0], REPORT_ID);
        assert_eq!(&report[1..6], &PREFIX);
        assert_eq!(&report[6..9], b"DIS");
        assert!(report[9..].iter().all(|&b| b == 0));
    }

    #[test]
    fn frame_keeps_the_report_size_for_a_long_command() {
        let long = vec![0xAAu8; PAYLOAD];
        let report = frame(&long);
        assert_eq!(report.len(), PAYLOAD + 1);
        assert_eq!(report[report.len() - 1], 0xAA);
    }

    #[test]
    fn chunk_starts_with_the_report_id() {
        let report = chunk(&[1, 2, 3, 4]);
        assert_eq!(report.len(), PAYLOAD + 1);
        assert_eq!(report[0], REPORT_ID);
        assert_eq!(&report[1..5], &[1, 2, 3, 4]);
        assert!(report[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn chunk_clips_to_one_report() {
        let bytes = vec![0x5Au8; PAYLOAD * 2];
        let report = chunk(&bytes);
        assert_eq!(report.len(), PAYLOAD + 1);
        assert!(report[1..].iter().all(|&b| b == 0x5A));
    }

    #[test]
    fn chunk_of_nothing_is_one_empty_report() {
        let report = chunk(&[]);
        assert_eq!(report.len(), PAYLOAD + 1);
        assert_eq!(report[0], REPORT_ID);
        assert!(report[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn parse_input_reads_key_and_state() {
        let mut report = vec![0u8; 513];
        report[1..4].copy_from_slice(&ACK);
        report[5..7].copy_from_slice(b"OK");
        report[KEY_OFFSET] = 0x0D;
        report[STATE_OFFSET] = 0x01;
        let event = parse_input(&report).expect("valid report");
        assert_eq!(event.key, 0x0D);
        assert!(event.pressed);

        report[STATE_OFFSET] = 0x00;
        let event = parse_input(&report).expect("valid report");
        assert!(!event.pressed);
    }

    #[test]
    fn parse_input_rejects_other_reports() {
        let mut report = vec![0u8; 513];
        assert!(parse_input(&report).is_none(), "no ACK marker");

        report[1..4].copy_from_slice(&ACK);
        assert!(parse_input(&report[..KEY_OFFSET]).is_none(), "too short");
    }

    #[test]
    fn parse_input_accepts_any_non_zero_state() {
        let mut report = vec![0u8; 513];
        report[1..4].copy_from_slice(&ACK);
        report[KEY_OFFSET] = 0x10;
        report[STATE_OFFSET] = 0x7F;
        assert!(parse_input(&report).expect("valid report").pressed);
    }

    #[test]
    fn image_header_carries_size_and_key() {
        let header = cmd::image_header(0x0D, 0x1234);
        assert_eq!(&header[..3], b"BAT");
        assert_eq!(&header[3..7], &[0x00, 0x00, 0x12, 0x34]);
        assert_eq!(header[7], 0x0D);
    }

    #[test]
    fn brightness_is_clamped() {
        assert_eq!(cmd::brightness(0), vec![0x4c, 0x49, 0x47, 0x00, 0x00, 0x00]);
        assert_eq!(
            cmd::brightness(200),
            vec![0x4c, 0x49, 0x47, 0x00, 0x00, 100]
        );
    }

    #[test]
    fn brightness_curve_matches_the_vendor_formula() {
        assert_eq!(map_brightness(0), 0);
        assert_eq!(map_brightness(100), 100);
        // 0.5 ^ 0.75 = 0.5946
        assert_eq!(map_brightness(50), 59);
        assert_eq!(map_brightness(25), 35);
    }

    #[test]
    fn hex_round_trips() {
        let bytes = parse_hex("43 52 54 00 00 44 49 53").expect("valid hex");
        assert_eq!(bytes, vec![0x43, 0x52, 0x54, 0x00, 0x00, 0x44, 0x49, 0x53]);
        assert_eq!(parse_hex("0x4352").expect("prefix form"), vec![0x43, 0x52]);
        assert_eq!(hex(&bytes[..3]), "43 52 54");
    }

    #[test]
    fn hex_rejects_bad_input() {
        assert!(parse_hex("4").is_err());
        assert!(parse_hex("zz").is_err());
    }

    #[test]
    fn a_full_round_trip_report_decodes_again() {
        let mut report = vec![0u8; 513];
        report[1..4].copy_from_slice(&ACK);
        report[KEY_OFFSET] = 0x11;
        report[STATE_OFFSET] = 0x01;
        assert_eq!(report_key(&report), 0x11);
        assert_eq!(
            parse_input(&report),
            Some(KeyEvent {
                key: 0x11,
                pressed: true
            })
        );
    }
}
