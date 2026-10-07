//! Explicit pending capture ownership, independent of hashes and platforms.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
};
use wim_format::{ParseError, metadata_write::OwnedMetadata};
/// Original source identity recorded at scan; reads reopen its pathname.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureIdentity {
    /// Update transaction identity; inode grouping never crosses capture calls.
    pub session: u64,
    /// Device number.
    pub device: u64,
    /// Filesystem inode number.
    pub inode: u64,
}
/// Captured content location; regular files are never read during scanning.
pub enum CapturedSource {
    /// Deferred filesystem file contents.
    File(PathBuf),
    /// Delete-on-close temporary stream containing EFS raw ciphertext.
    #[cfg(windows)]
    Temporary(File),
    /// Small translated symbolic-link reparse payload.
    Inline(Vec<u8>),
    /// Deferred logical NTFS stream retaining its immutable disk source.
    #[cfg(feature = "disk-capture")]
    Volume(Arc<dyn windows_disk::ReadAt>),
}
impl std::fmt::Debug for CapturedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Inline(data) => f.debug_struct("Inline").field("size", &data.len()).finish(),
            #[cfg(windows)]
            Self::Temporary(file) => f.debug_tuple("Temporary").field(file).finish(),
            #[cfg(feature = "disk-capture")]
            Self::Volume(source) => f
                .debug_struct("Volume")
                .field("size", &source.len())
                .finish(),
        }
    }
}
/// A real owned stream, independent of its eventual SHA-1 digest.
#[derive(Debug)]
pub struct CapturedStream {
    /// Fixed prefix length from the scan, as in upstream resource.c.
    pub size: u64,
    /// Source inode for hard-link grouping and diagnostics.
    pub identity: CaptureIdentity,
    /// Content source retained until its last image owner releases it.
    pub source: CapturedSource,
}
/// One inode's binding to an unhashed stream. Hard-link aliases share the Arc.
#[derive(Debug, Clone)]
pub struct CaptureBinding {
    /// Node in the owned capture graph.
    pub node: usize,
    /// Stream slot; zero denotes the main stream.
    pub slot: usize,
    /// Actual pending stream ownership, never a fabricated hash.
    pub stream: Arc<CapturedStream>,
}
/// Scanned metadata and deferred content bindings.
#[derive(Debug, Default, Clone)]
pub struct CapturePlan {
    /// Scanned tree; excluded roots have no nodes.
    pub tree: OwnedMetadata,
    /// Explicit pending stream references, including hard-link aliases.
    pub bindings: Vec<CaptureBinding>,
    /// Per-node source identity; absent for loaded metadata and synthetic parents.
    pub identities: Vec<Option<CaptureIdentity>>,
}
impl CapturePlan {
    /// Translate serialized metadata node indices back to authoritative graph
    /// nodes. The metadata encoder emits sorted depth-first children.
    pub fn metadata_order(&self) -> Result<Vec<usize>, ParseError> {
        let mut order = Vec::new();
        order
            .try_reserve_exact(self.tree.nodes.len())
            .map_err(|_| ParseError::Nomem)?;
        let mut seen = vec![false; self.tree.nodes.len()];
        let mut stack = if seen.is_empty() { Vec::new() } else { vec![0] };
        while let Some(index) = stack.pop() {
            let node = self
                .tree
                .nodes
                .get(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            if seen[index] {
                return Err(ParseError::InvalidMetadataResource);
            }
            seen[index] = true;
            order.push(index);
            let mut children = node.children.clone();
            children.sort_by(|&a, &b| {
                wim_format::ntfs_upcase::compare_names(
                    &self.tree.nodes[a].name,
                    &self.tree.nodes[b].name,
                )
            });
            stack.extend(children.into_iter().rev());
        }
        if order.len() != self.tree.nodes.len() {
            return Err(ParseError::InvalidMetadataResource);
        }
        Ok(order)
    }
}
/// Reader opened only when a captured stream is consumed.
pub enum CapturedReader<'a> {
    /// Reopened regular file, reading only the scanned prefix.
    File(File),
    /// Borrowed raw EFS spool read at explicit offsets.
    #[cfg(windows)]
    Temporary(&'a File),
    /// Already captured small reparse data.
    Inline(&'a [u8]),
    /// Retained positional logical-volume stream.
    #[cfg(feature = "disk-capture")]
    Volume(&'a dyn windows_disk::ReadAt),
}
impl CapturedStream {
    /// Open the content source without imposing a new inode-identity check.
    /// Upstream deliberately reads whatever file now occupies the saved path.
    pub fn open(&self) -> Result<CapturedReader<'_>, ParseError> {
        match &self.source {
            CapturedSource::File(path) => {
                #[cfg(windows)]
                let opened = {
                    use std::os::windows::fs::OpenOptionsExt;
                    // Read the stream belonging to the captured entry, including
                    // protected files and ADS on links, without following its target.
                    std::fs::OpenOptions::new()
                        .read(true)
                        .custom_flags(0x02000000 | 0x00200000)
                        .open(path)
                };
                #[cfg(not(windows))]
                let opened = File::open(path);
                opened
                    .map(CapturedReader::File)
                    .map_err(|_| ParseError::Open)
            }
            #[cfg(windows)]
            CapturedSource::Temporary(file) => Ok(CapturedReader::Temporary(file)),
            CapturedSource::Inline(bytes) => Ok(CapturedReader::Inline(bytes)),
            #[cfg(feature = "disk-capture")]
            CapturedSource::Volume(source) => Ok(CapturedReader::Volume(source.as_ref())),
        }
    }
}
impl CapturedReader<'_> {
    /// Read a bounded range. A source shortened after scan reports read error.
    pub fn read_range(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), ParseError> {
        match self {
            Self::File(file) => {
                file.seek(SeekFrom::Start(offset))
                    .map_err(|_| ParseError::Read)?;
                file.read_exact(bytes).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::UnexpectedEof {
                        #[cfg(target_os = "linux")]
                        // SAFETY: errno is writable calling-thread storage.
                        unsafe {
                            *libc::__errno_location() = libc::EINVAL;
                        }
                        ParseError::ConcurrentModificationDetected
                    } else {
                        ParseError::Read
                    }
                })
            }
            #[cfg(windows)]
            Self::Temporary(file) => {
                use std::os::windows::fs::FileExt;
                let mut remaining = bytes;
                let mut position = offset;
                while !remaining.is_empty() {
                    match file.seek_read(remaining, position) {
                        Ok(0) => return Err(ParseError::UnexpectedEndOfFile),
                        Ok(count) => {
                            position =
                                position.checked_add(count as u64).ok_or(ParseError::Read)?;
                            remaining = &mut remaining[count..];
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => return Err(ParseError::Read),
                    }
                }
                Ok(())
            }
            Self::Inline(source) => {
                let start = usize::try_from(offset).map_err(|_| ParseError::Read)?;
                let end = start.checked_add(bytes.len()).ok_or(ParseError::Read)?;
                bytes.copy_from_slice(source.get(start..end).ok_or(ParseError::Read)?);
                Ok(())
            }
            #[cfg(feature = "disk-capture")]
            Self::Volume(source) => source
                .read_exact_at(offset, bytes)
                .map_err(|_| ParseError::Read),
        }
    }
}
