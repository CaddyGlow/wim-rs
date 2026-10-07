//! Seekable archive composition with bounded resource reads.
use crate::{
    Compression, HEADER_SIZE, Header, PIPABLE_MAGIC, ParseError, ResourceHeader,
    file_resource::{self, FileReadError},
    lookup::{LookupBlob, LookupTable},
    resource::ResourceLayout,
};
use alloc::sync::Arc;
use alloc::vec::Vec;
use sha1::{Digest, Sha1};
use std::{
    cell::RefCell,
    io::{Read, Seek, SeekFrom},
};

/// Translate file-reader failures into the public WIM error vocabulary.
pub fn read_error(error: FileReadError) -> ParseError {
    match error {
        FileReadError::Format(error) => error,
        FileReadError::Io(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            ParseError::UnexpectedEndOfFile
        }
        FileReadError::Io(_) => ParseError::Read,
    }
}
fn io_error(error: std::io::Error) -> ParseError {
    read_error(error.into())
}

/// Read just the first header and, for pipable input, its final copy.
pub fn read_header(reader: &mut (impl Read + Seek)) -> Result<Header, ParseError> {
    let size = reader.seek(SeekFrom::End(0)).map_err(io_error)?;
    let mut bytes = [0; HEADER_SIZE];
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    reader.read_exact(&mut bytes).map_err(io_error)?;
    if bytes[..8] == PIPABLE_MAGIC {
        let offset = size
            .checked_sub(HEADER_SIZE as u64)
            .ok_or(ParseError::UnexpectedEndOfFile)?;
        reader.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        reader.read_exact(&mut bytes).map_err(io_error)?;
        bytes[..8].copy_from_slice(&PIPABLE_MAGIC);
    }
    Header::parse(&bytes, Some(size))
}

/// File-backed archive retaining descriptors, never enclosing payload bytes.
pub struct FileArchive<R> {
    reader: RefCell<R>,
    cache: Option<Arc<std::sync::Mutex<file_resource::ChunkCache>>>,
    /// Selected seekable header.
    pub header: Header,
    /// Retained resource descriptors.
    pub lookup: Arc<LookupTable>,
}
impl<R: Read + Seek> FileArchive<R> {
    /// Parse only headers and lookup resources.
    pub fn open(mut reader: R) -> Result<Self, ParseError> {
        let header = read_header(&mut reader)?;
        Self::open_with_header(reader, header)
    }
    /// Read descriptors using the header retained when this input was opened.
    /// This preserves resource offsets and error precedence if the file changes.
    pub fn open_with_header(mut reader: R, header: Header) -> Result<Self, ParseError> {
        let compression = header.validate_compression()?;
        let layout = layout(&header, false);
        let bytes = file_resource::read_resource_range(
            &mut reader,
            &header.blob_table,
            compression,
            header.chunk_size,
            layout,
            0..header.blob_table.uncompressed_size,
            |kind, input, output| crate::archive::decode(kind, input, output, header.chunk_size),
        )
        .map_err(read_error)?;
        let lookup = LookupTable::parse(&bytes, &header, |offset| {
            let mut bytes = [0; 16];
            reader.seek(SeekFrom::Start(offset)).map_err(io_error)?;
            reader.read_exact(&mut bytes).map_err(io_error)?;
            Ok(bytes)
        })?;
        let lookup = Arc::new(lookup);
        Ok(Self::with_lookup(reader, header, lookup))
    }
    /// Create a resource view using descriptors retained at initial open.
    pub fn with_lookup(reader: R, header: Header, lookup: Arc<LookupTable>) -> Self {
        Self {
            reader: RefCell::new(reader),
            cache: None,
            header,
            lookup,
        }
    }
    /// Reuse one solid chunk across archive views of the same immutable source.
    /// Input bytes are rechecked on cache hits; failed decodes are never retained.
    pub fn with_chunk_cache(
        mut self,
        cache: Arc<std::sync::Mutex<file_resource::ChunkCache>>,
    ) -> Self {
        self.cache = Some(cache);
        self
    }
    /// Read a descriptor's selected bytes and verify its complete SHA-1.
    fn read_descriptor(&self, blob: &LookupBlob) -> Result<Vec<u8>, ParseError> {
        let bytes = self.read_descriptor_range(blob, 0..blob.size, crate::archive::decode)?;
        if <[u8; 20]>::from(Sha1::digest(&bytes)) != blob.hash {
            return Err(ParseError::InvalidResourceHash);
        }
        Ok(bytes)
    }
    /// Read image metadata by its one-based index and verify its hash.
    pub fn read_metadata(&self, index: u32) -> Result<Vec<u8>, ParseError> {
        let index = index.checked_sub(1).ok_or(ParseError::InvalidImage)? as usize;
        self.read_descriptor(
            self.lookup
                .metadata
                .get(index)
                .ok_or(ParseError::InvalidImage)?,
        )
    }
    /// Read and verify a complete data blob.
    pub fn read_blob(&self, hash: &[u8; 20]) -> Result<Vec<u8>, ParseError> {
        self.read_descriptor(self.lookup.find(hash).ok_or(ParseError::ResourceNotFound)?)
    }
    /// Read a bounded blob range without verifying the complete content hash.
    pub fn read_blob_range(
        &self,
        hash: &[u8; 20],
        range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, ParseError> {
        self.read_blob_range_with_decoder(hash, range, crate::archive::decode)
    }
    /// Read a bounded blob range with caller-retained decoder workspace.
    pub fn read_blob_range_with_decoder(
        &self,
        hash: &[u8; 20],
        range: std::ops::Range<u64>,
        decode: impl FnMut(Compression, &[u8], &mut [u8], u32) -> Result<(), ParseError>,
    ) -> Result<Vec<u8>, ParseError> {
        self.read_descriptor_range(
            self.lookup.find(hash).ok_or(ParseError::ResourceNotFound)?,
            range,
            decode,
        )
    }
    fn read_descriptor_range(
        &self,
        blob: &LookupBlob,
        range: std::ops::Range<u64>,
        mut decode: impl FnMut(Compression, &[u8], &mut [u8], u32) -> Result<(), ParseError>,
    ) -> Result<Vec<u8>, ParseError> {
        if range.start > range.end || range.end > blob.size {
            return Err(ParseError::InvalidParam);
        }
        let resource = self
            .lookup
            .resources
            .get(blob.resource_index)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let start = blob
            .offset
            .checked_add(range.start)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let end = blob
            .offset
            .checked_add(range.end)
            .ok_or(ParseError::InvalidLookupTableEntry)?;
        let mut cache = self
            .cache
            .as_ref()
            .map(|cache| cache.lock().map_err(|_| ParseError::Read))
            .transpose()?;
        file_resource::read_resource_range_cached(
            &mut *self.reader.borrow_mut(),
            &resource.header,
            Compression::from_i32(resource.compression_code as i32)?,
            resource.chunk_size,
            layout(&self.header, resource.solid),
            start..end,
            |kind, input, output| decode(kind, input, output, resource.chunk_size),
            cache.as_deref_mut(),
        )
        .map_err(read_error)
    }
}
/// Select ordinary, pipable or solid resource framing.
pub fn layout(header: &Header, solid: bool) -> ResourceLayout {
    if solid {
        ResourceLayout::Solid
    } else if header.magic == PIPABLE_MAGIC {
        ResourceLayout::Pipable
    } else {
        ResourceLayout::Ordinary
    }
}
/// Read a header-selected whole resource, without reading the enclosing file.
pub fn read_resource(
    reader: &mut (impl Read + Seek),
    header: &Header,
    resource: &ResourceHeader,
) -> Result<Vec<u8>, ParseError> {
    file_resource::read_resource_range(
        reader,
        resource,
        header.validate_compression()?,
        header.chunk_size,
        layout(header, false),
        0..resource.uncompressed_size,
        |kind, input, output| crate::archive::decode(kind, input, output, header.chunk_size),
    )
    .map_err(read_error)
}

/// Verify optional archive integrity with bounded scratch and original callback order.
pub fn check_integrity_with_progress(
    reader: &mut (impl Read + Seek),
    header: &Header,
    mut progress: impl FnMut(crate::integrity::IntegrityProgress) -> Result<(), ParseError>,
) -> Result<crate::integrity::IntegrityStatus, ParseError> {
    use crate::integrity::{IntegrityProgress, IntegrityStatus, IntegrityTable};
    if header.integrity_table.offset_in_wim == 0 {
        return Ok(IntegrityStatus::Nonexistent);
    }
    let end = header
        .blob_table
        .offset_in_wim
        .checked_add(header.blob_table.size_in_wim)
        .ok_or(ParseError::InvalidIntegrityTable)?;
    let checked = end
        .checked_sub(HEADER_SIZE as u64)
        .ok_or(ParseError::InvalidIntegrityTable)?;
    if header.integrity_table.uncompressed_size < 12 {
        return Err(ParseError::InvalidIntegrityTable);
    }
    let bytes = read_resource(reader, header, &header.integrity_table)?;
    let table = IntegrityTable::parse(&bytes, checked)?;
    let mut state = IntegrityProgress {
        total_bytes: checked,
        completed_bytes: 0,
        total_chunks: table.digests().len() as u32,
        completed_chunks: 0,
        chunk_size: table.chunk_size(),
    };
    progress(state)?;
    let mut buffer = [0; 65536];
    for expected in table.digests() {
        let length = (checked - state.completed_bytes).min(u64::from(table.chunk_size()));
        reader
            .seek(SeekFrom::Start(HEADER_SIZE as u64 + state.completed_bytes))
            .map_err(io_error)?;
        let mut digest = Sha1::new();
        let mut remaining = length;
        while remaining > 0 {
            let n = remaining.min(buffer.len() as u64) as usize;
            reader.read_exact(&mut buffer[..n]).map_err(io_error)?;
            digest.update(&buffer[..n]);
            remaining -= n as u64;
        }
        if <[u8; 20]>::from(digest.finalize()) != *expected {
            return Ok(IntegrityStatus::Mismatch);
        }
        state.completed_bytes += length;
        state.completed_chunks += 1;
        progress(state)?;
    }
    Ok(IntegrityStatus::Ok)
}
