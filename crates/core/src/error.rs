//! Error type shared by the core library.
//!
//! Every error carries enough text for a log line. The device layer maps a
//! failed open or a failed transfer to these variants and never panics.

use std::fmt;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// One failure from the device layer.
#[derive(Debug)]
pub enum Error {
    /// A Win32 call failed. The handle is usually gone.
    Io(std::io::Error),
    /// No vendor collection with the requested product ID is present.
    NotFound { product_id: u16 },
    /// The key number is outside the model layout.
    BadKey { key: u8, max: u8 },
    /// The JPEG is larger than the firmware accepts.
    ImageTooLarge { size: usize, limit: usize },
    /// The report was accepted by the API but not in full.
    ShortWrite { wrote: usize, expected: usize },
    /// The command or file is not valid.
    Message(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "device I/O failed: {e}"),
            Error::NotFound { product_id } => write!(
                f,
                "no Stream Dock vendor collection with pid 0x{product_id:04X}"
            ),
            Error::BadKey { key, max } => write!(f, "key {key} is outside 1..={max}"),
            Error::ImageTooLarge { size, limit } => {
                write!(f, "image is {size} bytes, the limit is {limit}")
            }
            Error::ShortWrite { wrote, expected } => {
                write!(f, "wrote {wrote} of {expected} bytes")
            }
            Error::Message(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<String> for Error {
    fn from(m: String) -> Self {
        Error::Message(m)
    }
}

impl From<&str> for Error {
    fn from(m: &str) -> Self {
        Error::Message(m.to_string())
    }
}

impl Error {
    /// Make a second value with the same content.
    ///
    /// `std::io::Error` is not `Clone`, so a transport error keeps its kind and
    /// its text but not its raw OS code.
    pub fn duplicate(&self) -> Error {
        match self {
            Error::Io(e) => Error::Io(std::io::Error::new(e.kind(), e.to_string())),
            Error::NotFound { product_id } => Error::NotFound {
                product_id: *product_id,
            },
            Error::BadKey { key, max } => Error::BadKey {
                key: *key,
                max: *max,
            },
            Error::ImageTooLarge { size, limit } => Error::ImageTooLarge {
                size: *size,
                limit: *limit,
            },
            Error::ShortWrite { wrote, expected } => Error::ShortWrite {
                wrote: *wrote,
                expected: *expected,
            },
            Error::Message(m) => Error::Message(m.clone()),
        }
    }
}
