//! streamdock-store - profile model and JSON file store.
//!
//! A profile is one JSON file per profile name. The model is version tagged,
//! so a later release can migrate an old file.

pub mod file_store;
pub mod profile;

pub use file_store::{Store, StoreError};
pub use profile::{
    parse_color, Action, DeviceSpec, ImageSpec, Key, MediaCommand, Page, Profile,
};
