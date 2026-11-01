//! Model data for the unit under test.
//!
//! Model: AJAZZ AKP-153E, VID 0x0300, PID 0x3010.
//! The layout and the key IDs come from the public Bitfocus Companion
//! surface driver. The key table below is row major, left to right.
//!
//! Verified on hardware: the USB IDs, the product string, the two HID
//! collections, their report sizes, the key IDs of keys 1 to 15, the image
//! transfer and the rotation. Not verified on hardware: the key IDs of the
//! strip keys 16 to 18.

/// Product ID of the unit under test.
pub const PID: u16 = 0x3010;

/// Product name shown in logs.
pub const NAME: &str = "AJAZZ AKP-153E";

/// Rotation in degrees that the host must apply to every image before it
/// sends the JPEG. Measured on the unit under test: 90 turns the image
/// upside-down, 270 shows it upright.
pub const ROTATION: u16 = 270;

/// Number of keys, including the three strip keys.
pub const KEY_COUNT: u8 = 18;

/// Number of keys that send a key event.
///
/// Measured on hardware: the side strip is a display only. A finger on the
/// strip does not press a key and sends no report. Keys 16 to 18 exist as
/// displays and have no button.
pub const BUTTON_COUNT: u8 = 15;

/// Number of keys in the 5x3 grid.
pub const GRID_KEYS: u8 = 15;

/// Columns in the grid.
pub const COLUMNS: u8 = 5;

/// Rows in the grid.
pub const ROWS: u8 = 3;

/// Side of a grid key image in pixels.
pub const GRID_KEY_PIXELS: u16 = 96;

/// Side of a strip key image in pixels.
pub const STRIP_KEY_PIXELS: u16 = 80;

/// Width of the side strip display in pixels.
///
/// Measured on the unit under test: the strip is one display, not three.
/// Images are drawn at their own size; a taller image is squeezed to fit.
pub const STRIP_WIDTH: u16 = 96;

/// Height of the side strip display in pixels.
///
/// Measured: 480 cuts the top label, 460 shows every label and fills the
/// strip.
pub const STRIP_HEIGHT: u16 = 460;

/// Key IDs in row major order. Key 1 is the top left key.
const KEY_IDS: [u8; KEY_COUNT as usize] = [
    0x0d, 0x0a, 0x07, 0x04, 0x01, // row 0
    0x0e, 0x0b, 0x08, 0x05, 0x02, // row 1
    0x0f, 0x0c, 0x09, 0x06, 0x03, // row 2
    0x10, 0x11, 0x12, // strip keys 1 to 3
];

/// Return the hardware key ID for a key number in the range 1..=18.
///
/// # Panics
///
/// Panics when `key` is 0 or above `KEY_COUNT`. Use `Device::key_id` for a
/// checked form.
pub fn key_id(key: u8) -> u8 {
    assert!(
        key >= 1 && key <= KEY_COUNT,
        "key {key} is outside 1..={KEY_COUNT}"
    );
    KEY_IDS[(key - 1) as usize]
}

/// Return the key number for a hardware key ID, if the ID is known.
pub fn key_number(id: u8) -> Option<u8> {
    KEY_IDS.iter().position(|&k| k == id).map(|p| p as u8 + 1)
}

/// Image size in pixels for a key number.
pub fn key_size(key: u8) -> (u16, u16) {
    if key > GRID_KEYS {
        (STRIP_KEY_PIXELS, STRIP_KEY_PIXELS)
    } else {
        (GRID_KEY_PIXELS, GRID_KEY_PIXELS)
    }
}

/// True when the key number is inside the layout.
pub fn is_valid_key(key: u8) -> bool {
    key >= 1 && key <= KEY_COUNT
}

/// True when the key number sends a key event.
pub fn is_button(key: u8) -> bool {
    key >= 1 && key <= BUTTON_COUNT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn key_ids_are_unique() {
        let set: BTreeSet<u8> = KEY_IDS.iter().copied().collect();
        assert_eq!(set.len(), KEY_COUNT as usize);
    }

    #[test]
    fn key_number_is_the_inverse_of_key_id() {
        for key in 1..=KEY_COUNT {
            assert_eq!(key_number(key_id(key)), Some(key), "key {key}");
        }
    }

    #[test]
    fn unknown_ids_have_no_number() {
        assert_eq!(key_number(0x00), None);
        assert_eq!(key_number(0x13), None);
        assert_eq!(key_number(0xFF), None);
    }

    #[test]
    fn grid_is_five_by_three() {
        assert_eq!(GRID_KEYS, COLUMNS * ROWS);
        assert_eq!(KEY_COUNT - GRID_KEYS, 3);
    }

    #[test]
    fn sizes_match_the_layout() {
        for key in 1..=GRID_KEYS {
            assert_eq!(key_size(key), (96, 96), "key {key}");
        }
        for key in (GRID_KEYS + 1)..=KEY_COUNT {
            assert_eq!(key_size(key), (80, 80), "key {key}");
        }
    }

    #[test]
    fn validity_matches_the_table() {
        assert!(!is_valid_key(0));
        assert!(is_valid_key(1));
        assert!(is_valid_key(KEY_COUNT));
        assert!(!is_valid_key(KEY_COUNT + 1));
    }

    #[test]
    fn the_strip_has_displays_but_no_buttons() {
        assert_eq!(BUTTON_COUNT, GRID_KEYS);
        assert!(is_button(1));
        assert!(is_button(BUTTON_COUNT));
        assert!(!is_button(BUTTON_COUNT + 1));
        assert!(!is_button(KEY_COUNT));
        assert!(is_valid_key(KEY_COUNT));
    }

    #[test]
    fn the_strip_is_one_tall_display() {
        // Trial size while the exact width is still open.
        // Wider than one strip key, and longer than three of them.
        assert!(STRIP_WIDTH > STRIP_KEY_PIXELS);
        assert!(STRIP_HEIGHT > 3 * STRIP_KEY_PIXELS);
        assert_eq!((STRIP_WIDTH, STRIP_HEIGHT), (96, 460));
    }
}
