// SPDX-License-Identifier: LGPL-2.1-or-later
//! Owned Rust access to typed engine operations, independent of the C adapters.
#![forbid(unsafe_code)]
use std::{
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
};

pub use wim_format::Compression;

/// Policy for capturing a host filesystem into a new image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureOptions {
    /// Fail if Windows security descriptors cannot be captured completely.
    /// The caller must arrange the required privileges; capture does not
    /// silently omit SACLs or descriptors when this option is enabled.
    pub strict_security: bool,
}

/// Offline volume reparse policy for a complete filesystem capture.
#[cfg(feature = "disk-capture")]
#[derive(Debug, Clone, Default)]
pub struct VolumeCaptureOptions {
    /// Verified NT object aliases of the captured volume, for example `\\??\\C:`.
    /// Obtain these from offline MountedDevices or explicit preparation evidence.
    /// Internal absolute links are relocated to the destination image root.
    pub volume_aliases: Vec<String>,
    /// Preserve absolute links outside those aliases. Defaults to rejection.
    pub preserve_external_links: bool,
    /// Explicit fresh Windows Setup destination. Currently only verified `C:`
    /// is supported. Internal links retain this drive rather than being
    /// relocated to Setup's temporary staging directory. Defaults to portable
    /// destination-root relocation; this option requires eventual installation
    /// on C: and cannot be combined with external-link preservation.
    pub installation_system_drive: Option<String>,
}

/// Metadata policy decisions made while constructing an offline image.
#[cfg(feature = "disk-capture")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VolumeCaptureAudit {
    /// Kernel-managed EA records excluded under Microsoft's portable /EA policy.
    pub omitted_kernel_eas: usize,
    /// Nodes containing excluded kernel-managed EAs.
    pub nodes_with_omitted_kernel_eas: usize,
    /// Nodes carrying a desired storage class.
    pub storage_class_nodes: usize,
    /// Sparse files with a serialized hole map.
    pub sparse_files: usize,
    /// Serialized sparse hole ranges.
    pub sparse_hole_ranges: usize,
}

/// An error from a WIM operation or its input validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// An engine error retaining the original wimlib error code.
    Engine(i32),
    /// An image index outside the supported one-based range.
    InvalidImageIndex(u32),
    /// A path or property contains a NUL character.
    InteriorNul,
    /// The engine reported success without returning an owned handle.
    MissingHandle,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(code) => write!(
                f,
                "WIM operation failed ({code}): {}",
                crate::engine::error_message(*code)
            ),
            Self::InvalidImageIndex(index) => {
                write!(
                    f,
                    "WIM image index {index} must be between 1 and {}",
                    i32::MAX
                )
            }
            Self::InteriorNul => f.write_str("WIM paths and properties cannot contain NUL"),
            Self::MissingHandle => f.write_str("WIM engine returned no handle"),
        }
    }
}
impl std::error::Error for Error {}

/// A validated one-based image index, excluding the ABI's special sentinel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageIndex(i32);
impl TryFrom<u32> for ImageIndex {
    type Error = Error;
    fn try_from(value: u32) -> Result<Self, Error> {
        match i32::try_from(value) {
            Ok(index) if index > 0 => Ok(Self(index)),
            _ => Err(Error::InvalidImageIndex(value)),
        }
    }
}
impl ImageIndex {
    /// Return the one-based index.
    pub fn get(self) -> u32 {
        self.0 as u32
    }
}

/// Options for opening an existing archive.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenOptions {
    /// Validate the archive's integrity table when present.
    pub check_integrity: bool,
    /// Require permission to commit changes to the backing file.
    pub write_access: bool,
}

fn validate_text(value: &OsStr) -> Result<(), Error> {
    #[cfg(not(windows))]
    let has_nul = value.as_encoded_bytes().contains(&0);
    #[cfg(windows)]
    let has_nul = {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().any(|unit| unit == 0)
    };
    if has_nul {
        Err(Error::InteriorNul)
    } else {
        Ok(())
    }
}

fn engine<T>(result: Result<T, wim_format::ParseError>) -> Result<T, Error> {
    result.map_err(|error| Error::Engine(error as i32))
}
fn status<T>(result: Result<T, i32>) -> Result<T, Error> {
    result.map_err(Error::Engine)
}

/// Information about the images and compression in an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    /// Number of images in the archive.
    pub image_count: u32,
    /// One-based boot image index, or zero when none is selected.
    pub boot_index: u32,
    /// Compression used by the input archive.
    pub compression: Compression,
    /// Total uncompressed image bytes recorded in XML.
    pub total_bytes: u64,
}

/// An exclusively owned WIM archive, freed automatically when dropped.
///
/// Methods borrow the archive exclusively where the engine may mutate state.
/// Handles are neither `Send` nor `Sync` and never expose their raw pointer.
/// Committing changes consumes the archive because the engine invalidates it.
///
/// ```
/// use wim::{Compression, Wim};
/// let mut archive = Wim::new(Compression::Lzx)?;
/// assert_eq!(archive.info()?.image_count, 0);
/// # Ok::<(), wim::Error>(())
/// ```
pub struct Wim(Box<crate::engine::handles::WimHandle>);
impl fmt::Debug for Wim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Wim")
            .field("image_count", &self.0.images.len())
            .field("compression", &self.0.output_compression)
            .finish_non_exhaustive()
    }
}
impl Wim {
    /// Capture an offline NTFS manifest using retained positional stream readers.
    ///
    /// Keep the source immutable until all deferred WIM writing/export finishes.
    /// The manifest must include an unnamed directory root at index zero. Failed
    /// metadata validation leaves the archive unchanged. This does not populate
    /// Windows Setup properties, generalize Windows, or supply a recovery image.
    #[cfg(feature = "disk-capture")]
    pub fn capture_ntfs(
        &mut self,
        manifest: disk_capture::Manifest,
        name: &str,
        options: &VolumeCaptureOptions,
    ) -> Result<ImageIndex, Error> {
        self.capture_ntfs_with_audit(manifest, name, options)
            .map(|(index, _)| index)
    }

    /// Capture an offline NTFS manifest and return portable metadata decisions.
    /// Has the same lifetime and immutability requirements as `capture_ntfs`.
    #[cfg(feature = "disk-capture")]
    pub fn capture_ntfs_with_audit(
        &mut self,
        manifest: disk_capture::Manifest,
        name: &str,
        options: &VolumeCaptureOptions,
    ) -> Result<(ImageIndex, VolumeCaptureAudit), Error> {
        let (plan, audit) = crate::engine::capture::offline::plan(manifest, options)
            .map_err(|e| Error::Engine(e as i32))?;
        validate_text(OsStr::new(name))?;
        let index = engine(crate::engine::image_mutation::add_empty(
            &mut self.0,
            Some(name.as_bytes()),
        ))?;
        if let Err(error) =
            crate::engine::capture::offline::attach(&mut self.0, index as usize - 1, plan)
        {
            engine(crate::engine::image_mutation::delete_images(
                &mut self.0,
                index as i32,
            ))?;
            return Err(Error::Engine(error as i32));
        }
        Ok((ImageIndex(index as i32), audit))
    }
    /// Open an archive with optional integrity checking and write access.
    pub fn open(path: &Path, options: OpenOptions) -> Result<Self, Error> {
        validate_text(path.as_os_str())?;
        let flags = i32::from(options.check_integrity) | (i32::from(options.write_access) << 2);
        status(crate::engine::handles::open_path(
            path,
            flags,
            Default::default(),
        ))
        .map(Self)
    }

    /// Open with cancellable integrity checks; no callback is retained on success.
    pub fn open_with_cancel(
        path: &Path,
        options: OpenOptions,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Error> {
        if cancelled() {
            return Err(Error::Engine(76));
        }
        validate_text(path.as_os_str())?;
        let flags = i32::from(options.check_integrity) | (i32::from(options.write_access) << 2);
        let handle = status(crate::engine::handles::open_path_with_cancel(
            path,
            flags,
            &mut cancelled,
        ))?;
        Ok(Self(handle))
    }

    /// Create an empty archive with the selected output compression.
    pub fn new(compression: Compression) -> Result<Self, Error> {
        status(crate::engine::handles::create_handle(compression)).map(Self)
    }

    /// Read archive information into an owned value.
    pub fn info(&self) -> Result<Info, Error> {
        let info = crate::engine::info::info(&self.0);
        Ok(Info {
            image_count: info.image_count,
            boot_index: info.boot_index,
            compression: Compression::from_i32(info.compression_type)
                .map_err(|_| Error::Engine(16))?,
            total_bytes: info.total_bytes,
        })
    }

    /// Attach resources from other archives, checking their integrity when present.
    pub fn reference_resource_files(&mut self, paths: &[PathBuf]) -> Result<(), Error> {
        for path in paths {
            validate_text(path.as_os_str())?;
        }
        let paths: Vec<_> = paths.iter().map(PathBuf::as_path).collect();
        status(crate::engine::references::reference_files(
            &mut self.0,
            &paths,
            0,
            1,
        ))
    }

    /// Verify all image metadata and resource checksums.
    pub fn verify(&mut self) -> Result<(), Error> {
        engine(crate::engine::verify::verify_archive(&mut self.0, 0))
    }

    // The native engine dispatches progress synchronously on the caller thread:
    // write.rs ignores its ABI thread count and the selected write/verify/open/
    // extract paths spawn no workers. Thus a non-Send FnMut context is never
    // invoked concurrently; this must remain true if parallel codecs are added.
    fn cancellable<T>(
        &mut self,
        mut cancelled: impl FnMut() -> bool,
        operation: impl FnOnce(&mut crate::engine::handles::WimHandle) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if cancelled() {
            return Err(Error::Engine(76));
        }
        crate::engine::progress::with_cancellation(&mut self.0, &mut cancelled, operation)
    }

    /// Verify resources with scoped cancellation at engine progress checkpoints.
    pub fn verify_with_cancel(&mut self, cancelled: impl FnMut() -> bool) -> Result<(), Error> {
        self.cancellable(cancelled, |handle| {
            engine(crate::engine::verify::verify_archive(handle, 0))
        })
    }

    /// Write a new archive with scoped cancellation on the calling thread.
    /// Cancellation may leave a partial file at `path`; the caller must stage,
    /// clean up failures, and publish only successful verified output.
    pub fn write_with_cancel(
        &mut self,
        path: &Path,
        cancelled: impl FnMut() -> bool,
    ) -> Result<(), Error> {
        validate_text(path.as_os_str())?;
        self.cancellable(cancelled, |handle| {
            status(crate::engine::write::write_archive(handle, path, -1, 1, 0))
        })
    }

    /// Write all images to a new archive with an integrity table.
    pub fn write(&mut self, path: &Path) -> Result<(), Error> {
        validate_text(path.as_os_str())?;
        status(crate::engine::write::write_archive(
            &mut self.0,
            path,
            -1,
            1,
            0,
        ))
    }

    /// Commit changes with integrity checking and release this archive.
    ///
    /// The archive is consumed on both success and failure. Reopen it to continue.
    /// ```compile_fail
    /// # use wim::{Wim, OpenOptions};
    /// # let mut archive = Wim::open(std::path::Path::new("install.wim"), OpenOptions::default()).unwrap();
    /// archive.overwrite().unwrap();
    /// archive.info().unwrap(); // committing consumes the archive
    /// ```
    pub fn overwrite(mut self) -> Result<(), Error> {
        status(crate::engine::overwrite::overwrite_archive(
            &mut self.0,
            1,
            0,
        ))
    }

    /// Export an image into another archive, preserving properties by default.
    /// Set `boot` to select the newly exported image as the destination's boot image.
    pub fn export_image(
        &mut self,
        index: ImageIndex,
        destination: &mut Self,
        name: Option<&str>,
        boot: bool,
    ) -> Result<(), Error> {
        if let Some(name) = name {
            validate_text(OsStr::new(name))?;
        }
        engine(crate::engine::export::export_archive(
            &mut self.0,
            index.0,
            &mut destination.0,
            name,
            None,
            i32::from(boot),
        ))
    }

    /// Change an image XML property; changes are persisted by writing or committing.
    pub fn set_image_property(
        &mut self,
        index: ImageIndex,
        property: &str,
        value: &str,
    ) -> Result<(), Error> {
        validate_text(OsStr::new(property))?;
        validate_text(OsStr::new(value))?;
        engine(crate::engine::properties::set_property(
            &mut self.0,
            index.0,
            property.as_bytes(),
            Some(value.as_bytes()),
        ))
    }

    /// Select the archive's boot image.
    pub fn set_boot_index(&mut self, index: ImageIndex) -> Result<(), Error> {
        let info = crate::engine::info::WimInfo {
            boot_index: index.get(),
            ..Default::default()
        };
        status(crate::engine::info::set_info(&mut self.0, &info, 4))
    }

    /// Add or replace a file or directory in an image without committing it.
    pub fn add_file(
        &mut self,
        index: ImageIndex,
        source: &Path,
        destination: &str,
    ) -> Result<(), Error> {
        validate_text(source.as_os_str())?;
        validate_text(OsStr::new(destination))?;
        engine(crate::engine::update::add_file(
            &mut self.0,
            index.0,
            source,
            OsStr::new(destination),
            0,
        ))
    }

    /// Capture a filesystem directory as a new image with default capture settings.
    pub fn capture_image(&mut self, source: &Path) -> Result<(), Error> {
        self.capture_image_with_options(source, CaptureOptions::default())
    }

    /// Capture a directory with an explicit metadata policy.
    /// Source files must remain available and unchanged until writing finishes.
    pub fn capture_image_with_options(
        &mut self,
        source: &Path,
        options: CaptureOptions,
    ) -> Result<(), Error> {
        validate_text(source.as_os_str())?;
        engine(crate::engine::capture::capture_image(
            &mut self.0,
            source,
            None,
            if options.strict_security { 0x40 } else { 0 },
        ))
        .map(|_| ())
    }

    /// Extract a complete image to a filesystem directory with default settings.
    pub fn extract_image(&mut self, index: ImageIndex, destination: &Path) -> Result<(), Error> {
        validate_text(destination.as_os_str())?;
        engine(crate::engine::extract::extract_image(
            &self.0,
            index.0,
            destination,
            0,
        ))
    }

    /// Extract a Windows path with local case folding and scoped cancellation.
    pub fn extract_path_case_insensitive_with_cancel(
        &mut self,
        index: ImageIndex,
        path: &str,
        destination: &Path,
        cancelled: impl FnMut() -> bool,
    ) -> Result<PathBuf, Error> {
        self.cancellable(cancelled, |handle| {
            Self::extract_case_insensitive(handle, index, path, destination)
        })
    }

    /// Extract a Windows image path using local NTFS case folding.
    /// Returns the preserved basename under the destination; ambiguous matches fail.
    pub fn extract_path_case_insensitive(
        &mut self,
        index: ImageIndex,
        path: &str,
        destination: &Path,
    ) -> Result<PathBuf, Error> {
        Self::extract_case_insensitive(&self.0, index, path, destination)
    }

    fn extract_case_insensitive(
        handle: &crate::engine::handles::WimHandle,
        index: ImageIndex,
        path: &str,
        destination: &Path,
    ) -> Result<PathBuf, Error> {
        if path.contains('\0') {
            return Err(Error::InteriorNul);
        }
        let actual = {
            let bytes = crate::engine::handles::image_metadata_bytes(handle, index.0 as usize - 1)
                .map_err(|e| Error::Engine(e as i32))?;
            let tree = wim_format::metadata::Metadata::parse(&bytes)
                .map_err(|e| Error::Engine(e as i32))?;
            if tree.nodes.is_empty() {
                return Err(Error::Engine(49));
            }
            let mut node = 0;
            let mut names = Vec::new();
            for component in path.split(['/', '\\']).filter(|s| !s.is_empty()) {
                let entry = tree.inode_entry(node).ok_or(Error::Engine(49))?;
                if entry.attributes & 0x410 != 0x10 {
                    return Err(Error::Engine(49));
                }
                let folded: Vec<_> = component
                    .encode_utf16()
                    .map(wim_format::ntfs_upcase::uppercase)
                    .collect();
                let mut matches = tree.nodes[node].children.iter().copied().filter(|&i| {
                    tree.nodes[i]
                        .entry
                        .name
                        .chunks_exact(2)
                        .map(|b| {
                            wim_format::ntfs_upcase::uppercase(u16::from_le_bytes([b[0], b[1]]))
                        })
                        .eq(folded.iter().copied())
                });
                node = matches.next().ok_or(Error::Engine(49))?;
                if matches.next().is_some() {
                    return Err(Error::Engine(46));
                }
                let units: Vec<_> = tree.nodes[node]
                    .entry
                    .name
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                names.push(String::from_utf16(&units).map_err(|_| Error::Engine(49))?);
            }
            if names.is_empty() {
                return Err(Error::Engine(49));
            }
            names
        };
        validate_text(destination.as_os_str())?;
        let path = format!("/{}", actual.join("/"));
        engine(crate::engine::extract::extract_paths(
            handle,
            index.0,
            destination,
            &[OsStr::new(&path)],
            0x0020_0000,
        ))?;
        Ok(destination.join(actual.last().ok_or(Error::Engine(49))?))
    }

    /// Extract one image path to its basename beneath the destination.
    pub fn extract_path(
        &mut self,
        index: ImageIndex,
        path: &OsStr,
        destination: &Path,
    ) -> Result<(), Error> {
        validate_text(path)?;
        validate_text(destination.as_os_str())?;
        engine(crate::engine::extract::extract_paths(
            &self.0,
            index.0,
            destination,
            &[path],
            0x0020_0000,
        ))
    }
}
