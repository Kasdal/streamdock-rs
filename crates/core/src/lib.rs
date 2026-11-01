//! streamdock-core - Stream Dock / AJAZZ keypad support.
//!
//! The crate owns the HID channel to one panel. It holds no policy and no UI.
//! A daemon, a test tool or a desktop application uses the same types.
//!
//! ```no_run
//! use std::time::Duration;
//! use streamdock_core::{Device, Poll};
//!
//! let mut device = Device::open_first()?;
//! device.start_session()?;
//! device.set_brightness(60)?;
//! match device.poll_event(Duration::from_millis(200))? {
//!     Poll::Event(e) => println!("key {} pressed={}", e.key, e.pressed),
//!     _ => {}
//! }
//! # Ok::<(), streamdock_core::Error>(())
//! ```
//!
//! Facts that this crate depends on are documented in `proto` and `model`.
//! Measured hardware evidence is in `handoff/HANDOFF.md`.

pub mod animation;
pub mod device;
pub mod error;
pub mod hid;
pub mod image;
pub mod model;
pub mod proto;
pub mod session;
pub mod text;
pub mod tile;

pub use device::{Device, Poll};
pub use error::{Error, Result};
pub use image::Rgb;
pub use proto::KeyEvent;
pub use session::Session;
