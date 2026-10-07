//! External data-resource references with lazy independently owned snapshots.

use crate::engine::blob_index::{BlobCursor, BlobOwner};
use crate::engine::collections::FallibleMap as _;
use crate::engine::{
    TChar,
    handles::{OwnedBlob, OwnedResource, WimHandle},
};
use std::collections::HashMap;
use std::ffi::c_int;
use std::vec::Vec;
use wim_format::ParseError;

struct Staged {
    blobs: HashMap<[u8; 20], OwnedBlob>,
    order: Vec<[u8; 20]>,
}
impl Staged {
    fn new() -> Self {
        Self {
            blobs: HashMap::new(),
            order: Vec::new(),
        }
    }
    fn contains_key(&self, hash: &[u8; 20]) -> bool {
        self.blobs.contains_key(hash)
    }
    fn try_reserve(&mut self, count: usize) -> Result<(), ParseError> {
        self.blobs
            .try_reserve(count)
            .map_err(|_| ParseError::Nomem)?;
        self.order.try_reserve(count).map_err(|_| ParseError::Nomem)
    }
    fn insert(&mut self, hash: [u8; 20], blob: OwnedBlob) {
        self.order.push(hash);
        self.blobs.insert(hash, blob);
    }
}

fn already_present(destination: &WimHandle, staged: &Staged, hash: &[u8; 20]) -> bool {
    staged.contains_key(hash) || destination.blob_index.owner(hash).is_some()
}
fn stage_source(
    destination: &WimHandle,
    source: &WimHandle,
    staged: &mut Staged,
) -> Result<(), ParseError> {
    let mut snapshot = None;
    let mut cursor = BlobCursor::default();
    while let Some((hash, owner)) = source.blob_index.next(&mut cursor) {
        if source.removed_blobs.contains(&hash) || already_present(destination, staged, &hash) {
            continue;
        }
        let retained = match owner {
            BlobOwner::Stored => {
                let table = source.lookup.as_ref().ok_or(ParseError::ResourceNotFound)?;
                let blob = table.find(&hash).ok_or(ParseError::ResourceNotFound)?;
                let resource = table
                    .resources
                    .get(blob.resource_index)
                    .ok_or(ParseError::InvalidLookupTableEntry)?
                    .clone();
                if snapshot.is_none() {
                    snapshot = Some(
                        source
                            .backing
                            .as_ref()
                            .ok_or(ParseError::ResourceNotFound)?
                            .clone(),
                    );
                }
                OwnedBlob {
                    bytes: crate::engine::handles::OwnedBlobData::Decoded(Vec::new()),
                    backing: snapshot.clone(),
                    captured: None,
                    descriptor: OwnedResource {
                        blob: blob.clone(),
                        resource,
                        part: source.header.part_number,
                    },
                }
            }
            BlobOwner::Owned => {
                let blob = source
                    .owned_blobs
                    .get(&hash)
                    .ok_or(ParseError::ResourceNotFound)?;
                OwnedBlob {
                    bytes: blob.bytes.try_clone()?,
                    backing: blob.backing.clone(),
                    captured: blob.captured.clone(),
                    descriptor: blob.descriptor.clone(),
                }
            }
            BlobOwner::Captured => {
                let resource = crate::engine::lookup::captured_resources(source)?
                    .into_iter()
                    .find(|resource| resource.hash == hash)
                    .ok_or(ParseError::ResourceNotFound)?;
                OwnedBlob {
                    bytes: crate::engine::handles::OwnedBlobData::Decoded(Vec::new()),
                    backing: None,
                    captured: Some(resource.stream.clone()),
                    descriptor: OwnedResource {
                        blob: wim_format::lookup::LookupBlob {
                            hash,
                            resource_index: 0,
                            offset: 0,
                            size: resource.stream.size,
                            reference_count: resource.references,
                            flags: 0,
                        },
                        resource: wim_format::lookup::LookupResource {
                            header: wim_format::ResourceHeader::default(),
                            uncompressed_size: resource.stream.size,
                            compression_code: 0,
                            chunk_size: 0,
                            solid: false,
                        },
                        part: 0,
                    },
                }
            }
        };
        staged.try_reserve(1)?;
        staged.insert(hash, retained);
    }

    Ok(())
}
fn commit(destination: &mut WimHandle, mut staged: Staged) -> Result<(), ParseError> {
    destination
        .owned_blobs
        .try_reserve(staged.blobs.len())
        .map_err(|_| ParseError::Nomem)?;
    let index = destination.blob_index.try_clone()?;
    for hash in &staged.order {
        index.insert(*hash, BlobOwner::Owned)?;
    }
    for &hash in &staged.order {
        let blob = staged.blobs.remove(&hash).expect("staged ordered blob");
        destination.removed_blobs.remove(&hash);
        destination
            .owned_blobs
            .try_insert_reserved(hash, blob)
            .map_err(|_| ParseError::Nomem)?;
    }
    destination.blob_index = index;
    Ok(())
}

/// Reference unique data blobs from opened resource handles without importing
/// metadata/images. Source handles may be released after this call.
///
/// # Safety
/// `destination` must be live and exclusively accessed. With nonzero `count`,
/// `sources` must contain `count` live handle pointers. Destination/self entries
/// are accepted; other source handles must remain valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_reference_resources(
    destination: *mut WimHandle,
    sources: *const *mut WimHandle,
    count: u32,
    flags: c_int,
) -> c_int {
    if destination.is_null() || count != 0 && sources.is_null() || flags & !3 != 0 {
        return 24;
    }
    let sources = if count == 0 {
        &[]
    } else {
        // SAFETY: Caller guarantees readable pointer-array storage.
        unsafe { std::slice::from_raw_parts(sources, count as usize) }
    };
    if sources.iter().any(|source| source.is_null()) {
        return 24;
    }
    // SAFETY: Destination validity and exclusive access are caller requirements.
    let destination_handle = unsafe { &mut *destination };
    let mut staged = Staged::new();
    for source in sources {
        if *source == destination {
            continue;
        }
        // SAFETY: Source pointers were checked nonnull/nonaliasing destination;
        // all source handles are live for the duration required by caller.
        if let Err(error) = stage_source(destination_handle, unsafe { &**source }, &mut staged) {
            return error as c_int;
        }
    }
    commit(destination_handle, staged).map_or_else(|error| error as c_int, |()| 0)
}

fn stage_path(
    destination: &WimHandle,
    path: &std::path::Path,
    flags: c_int,
    staged: &mut Staged,
) -> Result<(), c_int> {
    let source = crate::engine::handles::open_path(path, flags, destination.progress.get())?;
    stage_source(destination, &source, staged).map_err(|error| error as c_int)
}

#[cfg(unix)]
struct Glob(libc::glob_t);
#[cfg(unix)]
impl Drop for Glob {
    fn drop(&mut self) {
        // SAFETY: glob initialized this zeroed POSIX glob_t, including partial
        // error results; globfree accepts its valid path-vector ownership state.
        unsafe {
            libc::globfree(&mut self.0);
        }
    }
}
#[cfg(unix)]
fn stage_glob(
    destination: &WimHandle,
    pattern: &std::path::Path,
    flags: c_int,
    open_flags: c_int,
    staged: &mut Staged,
) -> Result<(), c_int> {
    use std::os::unix::ffi::OsStrExt;
    let encoded_pattern = std::ffi::CString::new(pattern.as_os_str().as_bytes()).map_err(|_| 24)?;
    // SAFETY: glob_t consists of integers/pointers/function pointers; zero is
    // valid initialization for POSIX glob() with no APPEND/DOOFFS flags.
    let mut glob = Glob(unsafe { std::mem::zeroed() });
    // SAFETY: Encoded pattern is terminated; output points to initialized
    // writable glob_t storage and no error callback is supplied.
    let result = unsafe {
        libc::glob(
            encoded_pattern.as_ptr(),
            libc::GLOB_ERR | libc::GLOB_NOSORT,
            None,
            &mut glob.0,
        )
    };
    if result == libc::GLOB_NOMATCH {
        if flags & 2 != 0 {
            return Err(8);
        }
        // Original no-match behavior retries the pattern as a literal filename.
        // SAFETY: Forwarded readable platform path requirement.
        return stage_path(destination, pattern, open_flags, staged);
    }
    if result == libc::GLOB_NOSPACE {
        return Err(39);
    }
    if result != 0 {
        return Err(50);
    }
    let paths = if glob.0.gl_pathc == 0 {
        &[]
    } else {
        // SAFETY: Successful glob owns exactly gl_pathc readable string pointers.
        unsafe { std::slice::from_raw_parts(glob.0.gl_pathv, glob.0.gl_pathc) }
    };
    for path in paths {
        // SAFETY: Glob retains each valid NUL-terminated path through this call.
        let path = unsafe { std::ffi::CStr::from_ptr(*path) };
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
        stage_path(destination, path, open_flags, staged)?;
    }
    Ok(())
}

/// Open and reference literal resource filenames, or POSIX globs with flag 1.
/// Flag 2 makes unmatched globs return error 8; otherwise the unmatched pattern
/// is retried as a literal path. Failure rolls back all newly referenced blobs.
///
/// # Safety
/// `destination` must be live and exclusively accessed. Nonzero `count` requires
/// a readable array of `count` valid NUL-terminated platform path pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_reference_resource_files(
    destination: *mut WimHandle,
    paths: *const *const TChar,
    count: u32,
    flags: c_int,
    open_flags: c_int,
) -> c_int {
    if flags & !3 != 0 {
        return 24;
    }
    if count == 0 {
        return 0;
    }
    if destination.is_null() || paths.is_null() {
        return 24;
    }
    // SAFETY: Readable path-array validity is a caller requirement.
    let paths = unsafe { std::slice::from_raw_parts(paths, count as usize) };
    if paths.iter().any(|path| path.is_null()) {
        return 24;
    }
    let mut decoded_paths = Vec::new();
    if decoded_paths.try_reserve(paths.len()).is_err() {
        return 39;
    }
    for path in paths {
        // SAFETY: Caller provides readable terminated platform paths.
        match unsafe { crate::engine::handles::path_from_pointer(*path) } {
            Ok(path) => decoded_paths.push(path),
            // Empty filenames fail during staging, preserving per-file error order.
            Err(24) => decoded_paths.push(std::path::PathBuf::new()),
            Err(error) => return error,
        }
    }
    let mut borrowed_paths = Vec::new();
    if borrowed_paths.try_reserve(decoded_paths.len()).is_err() {
        return 39;
    }
    borrowed_paths.extend(decoded_paths.iter().map(std::path::PathBuf::as_path));
    // SAFETY: Caller keeps the destination live. Staging only shares its state,
    // allowing progress registration to be replaced through its interior Cell.
    let staged = match stage_files(unsafe { &*destination }, &borrowed_paths, flags, open_flags) {
        Ok(staged) => staged,
        Err(error) => return error,
    };
    // SAFETY: Staging and callbacks have finished; destination mutation is exclusive.
    commit(unsafe { &mut *destination }, staged).map_or_else(|error| error as c_int, |()| 0)
}

/// Reference files atomically, retaining source data without importing images.
pub(crate) fn reference_files(
    destination: &mut WimHandle,
    paths: &[&std::path::Path],
    flags: c_int,
    open_flags: c_int,
) -> Result<(), c_int> {
    if flags & !3 != 0 {
        return Err(24);
    }
    if paths.is_empty() {
        return Ok(());
    }
    let staged = stage_files(destination, paths, flags, open_flags)?;
    commit(destination, staged).map_err(|error| error as c_int)
}

fn stage_files(
    destination: &WimHandle,
    paths: &[&std::path::Path],
    flags: c_int,
    open_flags: c_int,
) -> Result<Staged, c_int> {
    let mut staged = Staged::new();
    for path in paths {
        if flags & 1 == 0 {
            stage_path(destination, path, open_flags, &mut staged)?;
        } else {
            #[cfg(unix)]
            stage_glob(destination, path, flags, open_flags, &mut staged)?;
            #[cfg(not(unix))]
            return Err(68);
        }
    }
    Ok(staged)
}
