#![cfg(unix)]
use std::ptr;
use wim::ffi::{
    WimHandle, wimlib_create_new_wim, wimlib_free, wimlib_open_wim,
    wimlib_reference_resource_files, wimlib_reference_resources, wimlib_verify_wim,
};
use wim_format::{
    Compression,
    image_build::{ImageBuilder, NewImage},
    metadata_write::{OwnedDentry, OwnedMetadata},
    repack::WriteOptions,
};

fn fixture() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "wim-reference-fixture-{}-{id}.wim",
        std::process::id()
    ));
    let mut builder = ImageBuilder::new([7; 16]);
    let hash = builder
        .add_blob(b"independent referenced resource")
        .unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children.push(1);
    let mut file = OwnedDentry::new(vec![b'f', 0], 0x20);
    file.main_hash = hash;
    builder
        .add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: Vec::new(),
                nodes: vec![root, file],
            },
            name: Some("source".into()),
            description: None,
            properties: Vec::new(),
        })
        .unwrap();
    let bytes = builder
        .write(WriteOptions {
            compression: Compression::None,
            chunk_size: 0,
            integrity: false,
        })
        .unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn handle_references_keep_data_after_source_release_and_do_not_import_images() {
    let path = fixture();
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let mut source = ptr::null_mut::<WimHandle>();
    let mut destination = ptr::null_mut::<WimHandle>();
    // SAFETY: Handles, strings and pointer-array storage are valid and exclusively used.
    unsafe {
        assert_eq!(wimlib_open_wim(name.as_ptr(), 0, &mut source), 0);
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        let sources = [source, source];
        assert_eq!(
            wimlib_reference_resources(destination, sources.as_ptr(), 2, 3),
            0
        );
        wimlib_free(source);
        std::fs::remove_file(path).unwrap();
        assert_eq!((*destination).header.image_count, 0);
        assert!((*destination).images.is_empty());
        assert_eq!((*destination).xml.image_count(), 0);
        assert_eq!((*destination).owned_blobs.len(), 1);
        let blob = (*destination).owned_blobs.values().next().unwrap();
        assert_eq!(blob.descriptor.blob.reference_count, 1);
        assert_eq!(wimlib_verify_wim(destination, 0), 0);
        wimlib_free(destination);
    }
}

#[test]
fn null_entry_validation_precedes_any_handle_reference_changes() {
    let path = fixture();
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let mut source = ptr::null_mut();
    let mut destination = ptr::null_mut();
    // SAFETY: All pointers are valid except the explicitly tested null source entry.
    unsafe {
        assert_eq!(wimlib_open_wim(name.as_ptr(), 0, &mut source), 0);
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        let sources = [source, ptr::null_mut()];
        assert_eq!(
            wimlib_reference_resources(destination, sources.as_ptr(), 2, 0),
            24
        );
        assert!((*destination).owned_blobs.is_empty());
        wimlib_free(source);
        wimlib_free(destination);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn file_reference_failure_rolls_back_new_resources_and_success_keeps_snapshot() {
    let path = fixture();
    let original = std::fs::read(&path).unwrap();
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let missing = std::ffi::CString::new(path.with_extension("missing").to_str().unwrap()).unwrap();
    let mut destination = ptr::null_mut();
    // SAFETY: Path strings and pointer arrays are valid; destination is exclusively owned.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        let paths = [name.as_ptr(), missing.as_ptr()];
        assert_eq!(
            wimlib_reference_resource_files(destination, paths.as_ptr(), 2, 0, 0),
            47
        );
        assert!((*destination).owned_blobs.is_empty());
        assert_eq!(
            wimlib_reference_resource_files(destination, paths.as_ptr(), 1, 0, 0),
            0
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        assert_eq!(wimlib_verify_wim(destination, 0), 0);
        wimlib_free(destination);
    }
}
