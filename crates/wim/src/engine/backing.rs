//! Retained seekable input with independent logical read cursors.

use crate::engine::collections::FallibleCollections as _;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::OnceLock,
};
use wim_format::{Header, ParseError, file_archive::FileArchive};

/// Shared input file; ordinary readers never materialize its whole contents.
#[derive(Debug)]
pub struct Backing {
    file: File,
    size: usize,
    header: Header,
    lookup: OnceLock<std::sync::Arc<wim_format::lookup::LookupTable>>,
    snapshot: OnceLock<Vec<u8>>,
    cache: OnceLock<std::sync::Arc<std::sync::Mutex<wim_format::file_resource::ChunkCache>>>,
}
impl Backing {
    /// Retain an already opened file without reading payload resources.
    pub fn new(mut file: File) -> Result<Self, ParseError> {
        let size = usize::try_from(file.metadata().map_err(|_| ParseError::Read)?.len())
            .map_err(|_| ParseError::Read)?;
        let header = wim_format::file_archive::read_header(&mut file)?;
        Ok(Self {
            file,
            size,
            header,
            lookup: OnceLock::new(),
            snapshot: OnceLock::new(),
            cache: OnceLock::new(),
        })
    }
    /// Whether the retained input is empty.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    /// Original enclosing file length.
    pub fn len(&self) -> usize {
        self.size
    }
    /// Stable origin identity for resource ordering.
    pub fn as_ptr(&self) -> *const Self {
        self
    }
    /// Independent cursor using positional reads, including across callbacks.
    pub fn reader(&self) -> Reader<'_> {
        Reader {
            backing: self,
            offset: 0,
        }
    }
    /// Read the fixed selected header only.
    pub fn header(&self) -> Result<Header, ParseError> {
        Ok(self.header.clone())
    }
    /// Parse a bounded descriptor view without loading payloads.
    pub fn archive(&self) -> Result<FileArchive<Reader<'_>>, ParseError> {
        let cache = self.chunk_cache_shared()?.clone();
        if let Some(lookup) = self.lookup.get() {
            return Ok(FileArchive::with_lookup(
                self.reader(),
                self.header.clone(),
                lookup.clone(),
            )
            .with_chunk_cache(cache));
        }
        let archive = FileArchive::open_with_header(self.reader(), self.header.clone())?;
        let _ = self.lookup.set(archive.lookup.clone());
        Ok(archive.with_chunk_cache(cache))
    }
    fn chunk_cache_shared(
        &self,
    ) -> Result<&std::sync::Arc<std::sync::Mutex<wim_format::file_resource::ChunkCache>>, ParseError>
    {
        if self.cache.get().is_none() {
            let cache = std::sync::Arc::new(std::sync::Mutex::new(
                wim_format::file_resource::ChunkCache::default(),
            ));
            let _ = self.cache.set(cache);
        }
        self.cache.get().ok_or(ParseError::Nomem)
    }
    pub(crate) fn chunk_cache(
        &self,
    ) -> Result<&std::sync::Mutex<wim_format::file_resource::ChunkCache>, ParseError> {
        Ok(self.chunk_cache_shared()?)
    }
    /// Compatibility snapshot for existing mutation/writer paths; fallible and
    /// explicit so resource consumers cannot accidentally trigger this copy.
    pub fn bytes(&self) -> Result<&[u8], ParseError> {
        if self.snapshot.get().is_none() {
            let mut bytes = Vec::new();
            bytes
                .try_reserve(self.size)
                .map_err(|_| ParseError::Nomem)?;
            let mut reader = self.reader();
            let mut chunk = [0; 65536];
            let mut remaining = self.size;
            while remaining != 0 {
                let length = remaining.min(chunk.len());
                reader
                    .read_exact(&mut chunk[..length])
                    .map_err(|e| wim_format::file_archive::read_error(e.into()))?;
                bytes
                    .try_extend_from_slice(&chunk[..length])
                    .map_err(|_| ParseError::Nomem)?;
                remaining -= length;
            }
            let _ = self.snapshot.set(bytes);
        }
        self.snapshot
            .get()
            .map(|b| b.as_slice())
            .ok_or(ParseError::Nomem)
    }
}
/// Read/seek cursor which never relies on the shared descriptor position.
pub struct Reader<'a> {
    backing: &'a Backing,
    offset: u64,
}
impl Read for Reader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(unix)]
        let count = {
            use std::os::unix::fs::FileExt;
            self.backing.file.read_at(bytes, self.offset)?
        };
        #[cfg(windows)]
        let count = {
            use std::os::windows::fs::FileExt;
            self.backing.file.seek_read(bytes, self.offset)?
        };
        self.offset = self
            .offset
            .checked_add(count as u64)
            .ok_or(std::io::ErrorKind::InvalidInput)?;
        Ok(count)
    }
}
impl Seek for Reader<'_> {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let offset = match from {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(delta) => {
                i128::from(self.backing.file.metadata()?.len()) + i128::from(delta)
            }
            SeekFrom::Current(delta) => i128::from(self.offset) + i128::from(delta),
        };
        self.offset = u64::try_from(offset).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        Ok(self.offset)
    }
}
