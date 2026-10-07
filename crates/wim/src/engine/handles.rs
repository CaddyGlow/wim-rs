// SPDX-License-Identifier: LGPL-2.1-or-later
//! Native opaque WIM ownership with retained seekable file input.

use crate::engine::TChar;
use crate::engine::collections::FallibleCollections as _;
use std::collections::HashMap;
use std::ffi::c_int;
use std::path::{Path, PathBuf};
use wim_format::xml::XmlInfo;

/// Owned state of one current image, independent of its index in the input WIM.
#[derive(Debug, Clone)]
pub enum HandleImage {
    /// One-based image index in the original retained backing bytes.
    Source(u32),
    /// A newly added image with an owned security table and null directory root.
    Empty(EmptyImage),
    /// Independently retained metadata exported from another handle.
    Owned(OwnedImage),
}

/// Retained platform path whose actual encoded bytes use the Rust global allocator.
#[derive(Debug)]
pub struct RetainedPath(Vec<u8>);
impl RetainedPath {
    /// Copy a valid platform path into globally allocated owned storage.
    pub fn new(path: &std::path::Path) -> Result<Self, ParseError> {
        Ok(Self(retain_archive_bytes(
            path.as_os_str().as_encoded_bytes(),
        )?))
    }
    /// Borrow the path without decoding or allocating platform text.
    pub fn as_path(&self) -> &std::path::Path {
        self
    }
    /// Clone actual retained bytes fallibly using the current allocation strategy.
    pub fn try_clone(&self) -> Result<Self, ParseError> {
        Self::new(self)
    }
}
impl std::ops::Deref for RetainedPath {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        // SAFETY: Bytes come only from as_encoded_bytes of a valid same-platform
        // Path and are copied unchanged; private storage cannot invalidate them.
        let value = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(&self.0) };
        std::path::Path::new(value)
    }
}
impl AsRef<std::path::Path> for RetainedPath {
    fn as_ref(&self) -> &std::path::Path {
        self
    }
}

/// Original descriptor information retained with an independently owned resource.
#[derive(Debug, Clone)]
pub struct OwnedResource {
    /// Blob identity and reference count.
    pub blob: wim_format::lookup::LookupBlob,
    /// Original ordinary or solid resource location.
    pub resource: wim_format::lookup::LookupResource,
    /// Original backing WIM part number.
    pub part: u16,
}

/// Independently retained exported image metadata.
#[derive(Debug, Clone)]
pub struct OwnedImage {
    /// Decoded original metadata bytes when `pending` is absent.
    pub metadata: Vec<u8>,
    /// Original digest when `pending` is absent.
    pub hash: [u8; 20],
    /// Shared pending metadata, retained across live exported owners.
    pub pending: Option<std::sync::Arc<std::sync::Mutex<PendingMetadata>>>,
    /// Original source descriptor; absent for pending rootless images.
    pub descriptor: Option<OwnedResource>,
    /// Stable image identity for original duplicate-export checks.
    pub identity: (u64, u32),
}

/// Actual ownership of a decoded or generated in-memory resource.
#[derive(Debug)]
pub enum OwnedBlobData {
    /// Existing independently decoded Rust-backed payload.
    Decoded(Vec<u8>),
    /// Original BLOB_IN_MEMORY semantics with shared retained allocation.
    Memory(std::sync::Arc<Vec<u8>>),
}
impl OwnedBlobData {
    /// Borrow the complete decoded payload; never a serialized source WIM.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Decoded(bytes) => bytes,
            Self::Memory(bytes) => bytes.as_slice(),
        }
    }
    /// Whether this descriptor owns generated BLOB_IN_MEMORY bytes.
    pub fn is_memory(&self) -> bool {
        matches!(self, Self::Memory(_))
    }
    /// Retain actual immutable memory ownership or copy legacy decoded bytes.
    pub fn try_clone(&self) -> Result<Self, ParseError> {
        match self {
            Self::Memory(bytes) => Ok(Self::Memory(bytes.clone())),
            Self::Decoded(bytes) => {
                let mut copied = Vec::new();
                copied
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| ParseError::Nomem)?;
                copied.extend_from_slice(bytes);
                Ok(Self::Decoded(copied))
            }
        }
    }
    /// Retain generated payload bytes using the current Rust global allocator.
    pub fn memory(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut retained = Vec::new();
        retained
            .try_extend_from_slice(bytes)
            .map_err(|_| ParseError::Nomem)?;
        Ok(Self::Memory(std::sync::Arc::new(retained)))
    }
}
impl AsRef<[u8]> for OwnedBlobData {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}
impl std::ops::Deref for OwnedBlobData {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

/// Independently retained exported data blob.
#[derive(Debug)]
pub struct OwnedBlob {
    /// Complete decompressed bytes when both backing and captured are absent.
    pub bytes: OwnedBlobData,
    /// Shared retained input file for deferred decoding and checksum validation.
    pub backing: Option<std::sync::Arc<crate::engine::backing::Backing>>,
    /// Independently retained filesystem or inline captured content.
    pub captured: Option<std::sync::Arc<crate::engine::capture::CapturedStream>>,
    /// Original descriptor and current destination reference count.
    pub descriptor: OwnedResource,
}

pub(crate) fn new_identity() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn image_identity(handle: &WimHandle, image: &HandleImage) -> (u64, u32) {
    match image {
        HandleImage::Source(index) => (handle.identity, *index),
        HandleImage::Empty(empty) => (empty.identity, 0),
        HandleImage::Owned(owned) => owned.identity,
    }
}

/// Shared materialization state of a pending image, including its first hash.
#[derive(Debug)]
pub struct PendingMetadata {
    /// Authoritative captured tree and stream bindings; payload remains deferred.
    pub capture: Option<crate::engine::capture::CapturePlan>,
    /// Security table and current root directory serialization.
    pub metadata: Vec<u8>,
    /// Zero until the metadata resource is hashed for writing.
    pub hash: [u8; 20],
}

pub(crate) fn pending_metadata(
    image: &HandleImage,
) -> Option<&std::sync::Arc<std::sync::Mutex<PendingMetadata>>> {
    match image {
        HandleImage::Empty(empty) => Some(&empty.shared),
        HandleImage::Owned(owned) => owned.pending.as_ref(),
        HandleImage::Source(_) => None,
    }
}

pub(crate) fn image_metadata_hash(
    handle: &WimHandle,
    image: &HandleImage,
) -> Result<[u8; 20], ParseError> {
    if let Some(pending) = pending_metadata(image) {
        return pending
            .lock()
            .map(|metadata| metadata.hash)
            .map_err(|_| ParseError::InvalidParam);
    }
    match image {
        HandleImage::Source(index) => handle
            .lookup
            .as_ref()
            .and_then(|lookup| lookup.metadata.get((*index - 1) as usize))
            .map(|blob| blob.hash)
            .ok_or(ParseError::MetadataNotFound),
        HandleImage::Owned(owned) => Ok(owned.hash),
        HandleImage::Empty(_) => Err(ParseError::InvalidParam),
    }
}

pub(crate) fn image_metadata_bytes(
    handle: &WimHandle,
    image_index: usize,
) -> Result<std::borrow::Cow<'_, [u8]>, ParseError> {
    if image_index >= handle.header.image_count as usize {
        return Err(ParseError::InvalidImage);
    }
    let image = handle
        .images
        .get(image_index)
        .ok_or(ParseError::MetadataNotFound)?;
    if let Some(pending) = pending_metadata(image) {
        let pending = pending.lock().map_err(|_| ParseError::InvalidParam)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(pending.metadata.len())
            .map_err(|_| ParseError::Nomem)?;
        bytes.extend_from_slice(&pending.metadata);
        return Ok(std::borrow::Cow::Owned(bytes));
    }
    match image {
        HandleImage::Source(index) => {
            let bytes = handle
                .backing
                .as_deref()
                .ok_or(ParseError::MetadataNotFound)?;
            let metadata = bytes.archive()?.read_metadata(*index).map_err(|error| {
                if error == ParseError::InvalidResourceHash {
                    ParseError::InvalidMetadataResource
                } else {
                    error
                }
            })?;
            Ok(std::borrow::Cow::Owned(metadata))
        }
        HandleImage::Owned(owned) => Ok(std::borrow::Cow::Borrowed(&owned.metadata)),
        HandleImage::Empty(_) => Err(ParseError::InvalidParam),
    }
}

/// Clone a captured graph without reading its deferred file contents.
pub(crate) fn image_capture_plan(
    image: &HandleImage,
) -> Result<Option<crate::engine::capture::CapturePlan>, ParseError> {
    pending_metadata(image)
        .map(|pending| {
            pending
                .lock()
                .map(|p| p.capture.clone())
                .map_err(|_| ParseError::InvalidParam)
        })
        .transpose()
        .map(Option::flatten)
}

/// Real shared metadata for an empty image before its first resource write.
#[derive(Debug, Clone)]
pub struct EmptyImage {
    /// Stable pending-image identity, preserved when exported.
    pub identity: u64,
    /// Shared root materialization and digest visible to every exported owner.
    pub shared: std::sync::Arc<std::sync::Mutex<PendingMetadata>>,
}
use wim_format::{Compression, Header, ParseError, ResourceHeader, WIM_MAGIC};

/// Opaque native counterpart of the public header's `WIMStruct`.
pub struct WimHandle {
    /// Caller-owned callback registration; replacement is allowed from callbacks.
    pub progress: std::cell::Cell<crate::engine::progress::ProgressRegistration>,
    /// Stable origin identity used for duplicate-export detection.
    pub identity: u64,
    /// Current fixed header.
    pub header: Header,
    /// Owned XML image information.
    pub xml: Box<XmlInfo>,
    /// Shared retained input file; absent for a newly created handle.
    pub backing: Option<std::sync::Arc<crate::engine::backing::Backing>>,
    /// Parsed original lookup descriptors, retained for allocation-free iteration.
    pub lookup: Option<wim_format::lookup::LookupTable>,
    /// Retained bucket chains and insertion history for hashed resources.
    pub(crate) blob_index: crate::engine::blob_index::BlobIndex,
    /// Canonical input filename, when opened from disk.
    pub filename: Option<RetainedPath>,
    /// Stable platform strings retained for getter pointers until handle release.
    pub xml_strings: HashMap<Vec<u8>, Vec<TChar>>,
    /// Current image order, preserving lazily loaded original metadata identity.
    pub images: Vec<HandleImage>,
    /// One ownership token per current image, shared across exported metadata owners.
    pub image_owners: Vec<std::sync::Arc<()>>,
    /// Image identities whose edited trees require XML statistics refresh on writing.
    pub dirty_images: hashbrown::HashSet<(u64, u32)>,
    /// Sticky image deletion state used to select overwrite rebuild policy.
    pub image_deletion_occurred: bool,
    /// Future in-memory blobs removed when their last image reference is deleted.
    /// Original WIM-backed blobs remain retained even at reference count zero.
    pub removed_blobs: hashbrown::HashSet<[u8; 20]>,
    /// Exported data independently owned after the source handle is released.
    pub owned_blobs: Box<hashbrown::HashMap<[u8; 20], OwnedBlob>>,
    /// Output compression, independent of the input header.
    pub output_compression: Compression,
    /// Output ordinary resource chunk size.
    pub output_chunk_size: u32,
    /// Output solid compression.
    pub output_solid_compression: Compression,
    /// Output solid resource chunk size.
    pub output_solid_chunk_size: u32,
}

pub(crate) unsafe fn handle_ref<'a>(pointer: *const WimHandle) -> Option<&'a WimHandle> {
    // SAFETY: Caller guarantees a live handle when nonnull.
    unsafe { pointer.as_ref() }
}
pub(crate) unsafe fn handle_mut<'a>(pointer: *mut WimHandle) -> Option<&'a mut WimHandle> {
    // SAFETY: Caller guarantees an exclusively accessed live handle when nonnull.
    unsafe { pointer.as_mut() }
}

pub(crate) fn can_modify(handle: &WimHandle) -> bool {
    handle.header.total_parts == 1
        && handle.header.flags & 4 == 0
        && handle.filename.as_ref().is_none_or(|path| writable(path))
}
#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn access(path: *const std::ffi::c_char, mode: c_int) -> c_int;
    }
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: Path is NUL terminated and access only reads the string.
    unsafe { access(path.as_ptr(), 2) == 0 }
}
#[cfg(not(unix))]
fn writable(path: &Path) -> bool {
    std::fs::OpenOptions::new().write(true).open(path).is_ok()
}

pub(crate) struct HandleStorage(Box<std::mem::MaybeUninit<WimHandle>>);
impl HandleStorage {
    pub(crate) fn new() -> Option<Self> {
        Some(Self(Box::new(std::mem::MaybeUninit::uninit())))
    }
    pub(crate) unsafe fn publish(
        mut self,
        handle: WimHandle,
        output: *mut *mut WimHandle,
    ) -> c_int {
        self.0.write(handle);
        let pointer = Box::into_raw(self.0).cast::<WimHandle>();
        // SAFETY: The initialized Box owns the handle; output is caller-writable.
        unsafe {
            output.write(pointer);
        }
        0
    }
}

/// Create an empty WIM, leaving output storage untouched on failure.
///
/// # Safety
/// Nonnull `output` must point to writable pointer storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_create_new_wim(
    compression: c_int,
    output: *mut *mut WimHandle,
) -> c_int {
    let initialization = crate::engine::runtime::initialize(0);
    if initialization != 0 {
        return initialization;
    }
    if output.is_null() {
        return 24;
    }
    let compression = match Compression::from_i32(compression) {
        Ok(c) => c,
        Err(error) => return error as c_int,
    };
    match create_handle(compression) {
        Ok(handle) => {
            // SAFETY: Caller guarantees writable output storage.
            unsafe {
                output.write(Box::into_raw(handle));
            }
            0
        }
        Err(error) => error,
    }
}

/// Construct an independently owned archive using typed Rust ownership.
pub(crate) fn create_handle(compression: Compression) -> Result<Box<WimHandle>, c_int> {
    let initialization = crate::engine::runtime::initialize(0);
    if initialization != 0 {
        return Err(initialization);
    }
    let empty = ResourceHeader {
        size_in_wim: 0,
        flags: 0,
        offset_in_wim: 0,
        uncompressed_size: 0,
    };
    let header = Header {
        magic: WIM_MAGIC,
        version: 0x10d00,
        flags: 0,
        chunk_size: 0,
        guid: [0; 16],
        part_number: 1,
        total_parts: 1,
        image_count: 0,
        blob_table: empty,
        xml_data: empty,
        boot_metadata: empty,
        boot_index: 0,
        integrity_table: empty,
        reserved: [0; 60],
    };
    let xml = match XmlInfo::parse_bytes(b"<WIM/>") {
        Ok(xml) => Box::new(xml),
        Err(error) => return Err(error as c_int),
    };
    let chunk_size = match compression {
        Compression::None => 0,
        Compression::Lzms => 131072,
        _ => 32768,
    };
    let owned_blobs = match new_owned_blob_table() {
        Ok(table) => table,
        Err(_) => return Err(39),
    };
    let blob_index = match crate::engine::blob_index::BlobIndex::new(64) {
        Ok(index) => index,
        Err(_) => return Err(39),
    };
    let handle = WimHandle {
        progress: std::cell::Cell::new(crate::engine::progress::ProgressRegistration::default()),
        identity: new_identity(),
        header,
        xml,
        backing: None,
        lookup: None,
        blob_index,
        filename: None,
        xml_strings: HashMap::new(),
        images: Vec::new(),
        image_owners: Vec::new(),
        dirty_images: hashbrown::HashSet::new(),
        image_deletion_occurred: false,
        removed_blobs: hashbrown::HashSet::new(),
        owned_blobs,
        output_compression: compression,
        output_chunk_size: chunk_size,
        output_solid_compression: Compression::Lzms,
        output_solid_chunk_size: 67108864,
    };
    Ok(Box::new(handle))
}

/// Release an owned WIM handle. Null is accepted.
///
/// # Safety
/// Nonnull `handle` must be an unfreed handle allocated by this library, with no
/// outstanding references or operations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_free(handle: *mut WimHandle) {
    if !handle.is_null() {
        // SAFETY: Caller transfers exclusive ownership of this allocation.
        unsafe {
            drop(Box::from_raw(handle));
        }
    }
}

/// Borrow a Unix platform path from caller storage without an intermediate allocation.
/// # Safety
/// Pointer must reference a readable terminated string that outlives the result.
#[cfg(unix)]
pub(crate) unsafe fn borrowed_path_from_pointer<'a>(
    pointer: *const TChar,
) -> Result<&'a std::path::Path, c_int> {
    use std::os::unix::ffi::OsStrExt;
    if pointer.is_null() {
        return Err(24);
    }
    // SAFETY: Caller guarantees readable terminated bytes and result lifetime.
    let bytes = unsafe { std::ffi::CStr::from_ptr(pointer) }.to_bytes();
    if bytes.is_empty() {
        return Err(24);
    }
    Ok(std::path::Path::new(std::ffi::OsStr::from_bytes(bytes)))
}
pub(crate) unsafe fn path_from_pointer(pointer: *const TChar) -> Result<PathBuf, c_int> {
    if pointer.is_null() {
        return Err(24);
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: Caller supplies a valid NUL-terminated platform path.
        let path = unsafe { std::ffi::CStr::from_ptr(pointer) }.to_bytes();
        if path.is_empty() {
            return Err(24);
        }
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path)))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut length = 0;
        // SAFETY: Caller supplies a valid NUL-terminated wide path.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        if length == 0 {
            return Err(24);
        }
        // SAFETY: Length identifies the valid portion before the terminating NUL.
        Ok(
            std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(pointer, length) })
                .into(),
        )
    }
}

/// Open a seekable WIM file without modifying its bytes.
/// Reads only the fixed headers, XML and resource descriptors on ordinary open.
///
/// # Safety
/// `filename` must be a NUL-terminated platform string and nonnull `output` must
/// point to writable pointer storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_open_wim(
    filename: *const TChar,
    flags: c_int,
    output: *mut *mut WimHandle,
) -> c_int {
    // SAFETY: Caller supplies the filename and writable output; no callback is installed.
    unsafe { wimlib_open_wim_with_progress(filename, flags, output, None, std::ptr::null_mut()) }
}

/// Open a WIM with a caller-owned progress callback, retained on success.
/// Integrity checking emits real initial and completed-chunk events; callback
/// cancellation leaves the output pointer untouched and releases pending state.
///
/// # Safety
/// Filename and output follow `wimlib_open_wim` requirements. Callback code and
/// context must remain valid until replaced or the successful handle is freed.
/// The callback must not mutate the active resource state or free the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_open_wim_with_progress(
    filename: *const TChar,
    flags: c_int,
    output: *mut *mut WimHandle,
    callback: Option<crate::engine::progress::ProgressCallback>,
    context: *mut std::ffi::c_void,
) -> c_int {
    if flags & !7 != 0 || output.is_null() {
        return 24;
    }
    // SAFETY: Platform string validity is a caller requirement.
    let path = match unsafe { path_from_pointer(filename) } {
        Ok(path) => path,
        Err(error) => return error,
    };
    let registration = crate::engine::progress::ProgressRegistration::new(callback, context);
    match open_path(&path, flags, registration) {
        Ok(handle) => {
            // SAFETY: Caller guarantees writable output storage.
            unsafe {
                output.write(Box::into_raw(handle));
            }
            0
        }
        Err(error) => error,
    }
}

/// Open a filesystem archive with an optional retained progress registration.
pub(crate) fn open_path(
    path: &Path,
    flags: c_int,
    registration: crate::engine::progress::ProgressRegistration,
) -> Result<Box<WimHandle>, c_int> {
    if flags & !7 != 0 || path.as_os_str().is_empty() {
        return Err(24);
    }
    let initialization = crate::engine::runtime::initialize(0);
    if initialization != 0 {
        return Err(initialization);
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            let mut message = b"Can't open \"".to_vec();
            message.extend_from_slice(path.as_os_str().as_encoded_bytes());
            message.extend_from_slice(b"\" read-only");
            #[cfg(target_os = "linux")]
            if let Some(errno) = error.raw_os_error() {
                // SAFETY: Restore the current thread's OS failure before diagnostics.
                unsafe {
                    *libc::__errno_location() = errno;
                }
            }
            #[cfg(not(target_os = "linux"))]
            let _ = error;
            crate::engine::diagnostics::message(false, &message, true);
            return Err(47);
        }
    };
    let backing = match crate::engine::backing::Backing::new(file) {
        Ok(backing) => backing,
        Err(error) => return Err(error as c_int),
    };
    let canonical = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(_) => return Err(45),
    };
    open_handle(backing, canonical, flags, registration)
        .map(Box::new)
        .map_err(|error| error as c_int)
}

/// Open with a scoped native cancellation closure, retaining no registration.
pub(crate) fn open_path_with_cancel(
    path: &Path,
    flags: c_int,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Box<WimHandle>, c_int> {
    // SAFETY: Opening invokes cancellation synchronously on this thread. The
    // borrowed closure stays live through opening and its registration is
    // removed before an owned handle can escape. On failure or unwinding,
    // opening drops its partial state while this closure is still borrowed.
    let registration =
        unsafe { crate::engine::progress::ProgressRegistration::cancellation(cancelled) };
    let handle = open_path(path, flags, registration)?;
    handle.progress.set(Default::default());
    Ok(handle)
}

fn open_handle(
    backing: crate::engine::backing::Backing,
    filename: PathBuf,
    flags: c_int,
    registration: crate::engine::progress::ProgressRegistration,
) -> Result<WimHandle, ParseError> {
    let mut header = backing.header()?;
    if flags & 4 != 0 && (header.total_parts != 1 || header.flags & 4 != 0 || !writable(&filename))
    {
        return Err(ParseError::WimIsReadonly);
    }
    if flags & 2 != 0 && header.total_parts != 1 {
        return Err(ParseError::IsSplitWim);
    }
    if header.boot_index > header.image_count {
        header.boot_index = 0;
    }
    let compression = header.validate_compression()?;
    if flags & 1 != 0 {
        let progress_filename = crate::engine::progress::filename_buffer(Some(&filename))?;
        let status = wim_format::file_archive::check_integrity_with_progress(
            &mut backing.reader(),
            &header,
            |state| {
                let mut info = crate::engine::progress::ProgressInfo::zeroed();
                info.integrity = crate::engine::progress::IntegrityProgress {
                    total_bytes: state.total_bytes,
                    completed_bytes: state.completed_bytes,
                    total_chunks: state.total_chunks,
                    completed_chunks: state.completed_chunks,
                    chunk_size: state.chunk_size,
                    filename: progress_filename
                        .as_ref()
                        .map_or(std::ptr::null(), |s| s.as_ptr()),
                };
                // SAFETY: The FFI caller owns callback/context lifetime. Event
                // storage and the canonical filename remain live through dispatch.
                unsafe { registration.call(16, &mut info) }
            },
        )?;
        if status == wim_format::integrity::IntegrityStatus::Mismatch {
            return Err(ParseError::Integrity);
        }
    }
    if header.blob_table.uncompressed_size == 0 && header.xml_data.uncompressed_size == 0 {
        return Err(ParseError::WimIsIncomplete);
    }
    let xml_bytes =
        wim_format::file_archive::read_resource(&mut backing.reader(), &header, &header.xml_data)?;
    let xml = Box::new(XmlInfo::parse_utf16le(&xml_bytes)?);
    if xml.image_count() != header.image_count as usize {
        return Err(ParseError::ImageCount);
    }
    let lookup = backing.archive()?.lookup.try_clone()?;
    let backing = std::sync::Arc::new(backing);
    let chunk_size = header.chunk_size;
    let mut images = Vec::new();
    if header.part_number == 1 {
        images
            .try_reserve(header.image_count as usize)
            .map_err(|_| ParseError::Nomem)?;
        images
            .try_extend((1..=header.image_count).map(HandleImage::Source))
            .map_err(|_| ParseError::Nomem)?;
    }
    let mut image_owners = Vec::new();
    image_owners
        .try_reserve(images.len())
        .map_err(|_| ParseError::Nomem)?;
    for _ in &images {
        image_owners
            .try_push(new_image_owner()?)
            .map_err(|_| ParseError::Nomem)?;
    }
    let filename = RetainedPath::new(&filename)?;
    let owned_blobs = new_owned_blob_table()?;
    let raw_entries =
        usize::try_from(header.blob_table.uncompressed_size / 50).map_err(|_| ParseError::Nomem)?;
    let blob_index = crate::engine::blob_index::BlobIndex::new(raw_entries)?;
    for blob in &lookup.blobs {
        blob_index.insert(blob.hash, crate::engine::blob_index::BlobOwner::Stored)?;
    }
    Ok(WimHandle {
        progress: std::cell::Cell::new(registration),
        identity: new_identity(),
        header,
        xml,
        backing: Some(backing),
        lookup: Some(lookup),
        blob_index,
        filename: Some(filename),
        xml_strings: HashMap::new(),
        images,
        image_owners,
        dirty_images: hashbrown::HashSet::new(),
        image_deletion_occurred: false,
        removed_blobs: hashbrown::HashSet::new(),
        owned_blobs,
        output_compression: compression,
        output_chunk_size: chunk_size,
        output_solid_compression: Compression::Lzms,
        output_solid_chunk_size: 67108864,
    })
}

pub(crate) fn own_xml(xml: XmlInfo) -> Result<Box<XmlInfo>, ParseError> {
    Ok(Box::new(xml))
}
pub(crate) fn new_owned_blob_table()
-> Result<Box<hashbrown::HashMap<[u8; 20], OwnedBlob>>, ParseError> {
    let mut table = hashbrown::HashMap::new();
    table.try_reserve(64).map_err(|_| ParseError::Nomem)?;
    Ok(Box::new(table))
}

pub(crate) fn retain_archive_bytes(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut retained = Vec::new();
    retained
        .try_extend_from_slice(bytes)
        .map_err(|_| ParseError::Nomem)?;
    Ok(retained)
}

pub(crate) fn new_pending_metadata(
    metadata: PendingMetadata,
) -> Result<std::sync::Arc<std::sync::Mutex<PendingMetadata>>, ParseError> {
    Ok(std::sync::Arc::new(std::sync::Mutex::new(metadata)))
}
pub(crate) fn new_image_owner() -> Result<std::sync::Arc<()>, ParseError> {
    Ok(std::sync::Arc::new(()))
}
pub(crate) fn image_owner_count(handle: &WimHandle, index: usize) -> usize {
    std::sync::Arc::strong_count(&handle.image_owners[index])
}

#[cfg(test)]
mod tests {
    use super::create_handle;
    use crate::engine::{image_mutation, info, properties};
    use wim_format::Compression;

    #[test]
    fn typed_archive_mutations_preserve_boot_validation_and_owned_metadata() {
        let mut archive = create_handle(Compression::None).unwrap();
        let image = image_mutation::add_empty(&mut archive, Some(b"Initial")).unwrap();
        properties::set_property(&mut archive, image as i32, b"NAME", Some(b"Renamed")).unwrap();
        let mut state = info::info(&archive);
        state.boot_index = image + 1;
        assert_eq!(info::set_info(&mut archive, &state, 4), Err(18));
        assert_eq!(info::info(&archive).boot_index, 0);
        state.boot_index = image;
        info::set_info(&mut archive, &state, 4).unwrap();
        assert_eq!(
            archive.xml.name_bytes(image as i32),
            Some(b"Renamed".as_slice())
        );
        image_mutation::delete_images(&mut archive, image as i32).unwrap();
        let state = info::info(&archive);
        assert_eq!(state.image_count, 0);
        assert_eq!(state.boot_index, 0);
        assert!(archive.images.is_empty());
    }
}
