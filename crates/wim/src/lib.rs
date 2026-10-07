//! WIM images through an owned Rust API.
//!
//! Use [`Wim`], [`ImageIndex`], and [`OpenOptions`] for image operations.
//! [`ffi`] provides the compatible, unsafe wimlib C interface.
#![deny(missing_docs)]

mod api;
pub use api::{CaptureOptions, Compression, Error, ImageIndex, Info, OpenOptions, Wim};
#[cfg(feature = "disk-capture")]
pub use api::{VolumeCaptureAudit, VolumeCaptureOptions};

/// Low-level implementation modules for engine development and validation.
/// These internals are not the application API; use [`Wim`] for owned operations.
#[doc(hidden)]
pub mod engine;
pub mod ffi;
