//! Filesystem capture plans retain source paths and defer regular stream reads.
mod model;
#[cfg(any(windows, test))]
mod ntfs_streams;
#[cfg(feature = "disk-capture")]
pub(crate) mod offline;
#[cfg(any(windows, test, feature = "disk-capture"))]
pub(crate) mod reparse;
pub use model::*;
mod common;
pub use common::{CaptureConfig, ScanEvent};
mod checksum;
pub(crate) use checksum::checksum_pending;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(any(unix, windows))]
mod abi;
#[cfg(any(unix, windows))]
mod overlay;
#[cfg(any(unix, windows))]
pub use abi::*;
#[cfg(any(unix, windows))]
mod api;
#[cfg(any(unix, windows))]
pub use api::*;

#[cfg(any(unix, windows))]
pub(crate) use api::capture_image;
