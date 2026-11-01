//! Model data for the unit under test.
//!
//! Model: AJAZZ AKP-153E, VID 0x0300, PID 0x3010.
//! The layout and the key IDs come from the public Bitfocus Companion
//! surface driver. The key table below is row major, left to right.
//!
//! Verified on hardware: the USB IDs, the product string, the two HID
//! collections and their report sizes.
//! Not verified on hardware: the key IDs and the image rotation.

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

/// Key IDs in row major order. Key 1 is the top left key.
const KEY_IDS: [u8; KEY_COUNT as usize] = [
    0x0d, 0x0a, 0x07, 0x04, 0x01, // row 0
    0x0e, 0x0b, 0x08, 0x05, 0x02, // row 1
    0x0f, 0x0c, 0x09, 0x06, 0x03, // row 2
    0x10, 0x11, 0x12, // strip keys 1 to 3
];

/// Return the hardware key ID for a key number in the range 1..=18.
pub fn key_id(key: u8) -> u8 {
    KEY_IDS[(key - 1) as usize]
}

/// Return the key number for a hardware key ID, if the ID is known.
pub fn key_number(id: u8) -> Option<u8> {
    KEY_IDS.iter().position(|&k| k == id).map(|p| p as u8 + 1)
}

/// Image size in pixels for a key number.
pub fn key_size(key: u8) -> (u16, u16) {
    if key > 15 {
        (80, 80)
    } else {
        (96, 96)
    }
}
