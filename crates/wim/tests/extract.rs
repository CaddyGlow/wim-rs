#![cfg(target_os = "linux")]
use std::{
    ffi::CString,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    ptr,
};
use wim::ffi::{
    WimHandle, wimlib_add_empty_image, wimlib_create_new_wim, wimlib_export_image,
    wimlib_extract_image, wimlib_extract_pathlist, wimlib_extract_paths, wimlib_free,
    wimlib_open_wim,
};
use wim_format::{
    Compression,
    image_build::{ImageBuilder, NewImage},
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
    repack::WriteOptions,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "wim-extract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn name(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn cpath(path: &Path) -> CString {
    CString::new(path.as_os_str().as_bytes()).unwrap()
}
fn build(path: &Path) -> Vec<u8> {
    let mut builder = ImageBuilder::new([1; 16]);
    let mut data = vec![0; 128 * 1024];
    data[..5].copy_from_slice(b"hello");
    data[128 * 1024 - 5..].copy_from_slice(b"world");
    let hash = builder.add_blob(&data).unwrap();
    let named_hash = builder.add_blob(b"ignored named stream").unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 3, 4];
    let mut directory = OwnedDentry::new(name("nested"), 0x10);
    directory.children.push(2);
    let mut file = OwnedDentry::new(name("file"), 0x220);
    file.main_hash = hash;
    file.inode_union = 9;
    let mut alias = file.clone();
    alias.name = name("alias");
    let mut named = OwnedDentry::new(name("named"), 0x20);
    named.extra_streams.push(OwnedStream {
        hash: named_hash,
        name: name("alternate"),
        ..OwnedStream::default()
    });
    builder
        .add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: Vec::new(),
                nodes: vec![root, directory, file, alias, named],
            },
            name: Some("Fixture".into()),
            description: None,
            properties: Vec::new(),
        })
        .unwrap();
    let bytes = builder
        .write(WriteOptions {
            compression: Compression::Xpress,
            chunk_size: 32768,
            integrity: false,
        })
        .unwrap();
    std::fs::write(path, &bytes).unwrap();
    data
}

#[test]
fn exported_image_extracts_sparse_hardlinks_after_source_and_backing_are_removed() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.wim");
    let data = build(&source);
    let target = fixture.0.join("output");
    let mut handle = ptr::null_mut::<WimHandle>();
    let mut destination = ptr::null_mut();
    // SAFETY: Strings and handles remain valid, are used exclusively, and are freed once.
    unsafe {
        assert_eq!(wimlib_open_wim(cpath(&source).as_ptr(), 0, &mut handle), 0);
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        assert_eq!(
            wimlib_export_image(handle, 1, destination, ptr::null(), ptr::null(), 0),
            0
        );
        wimlib_free(handle);
        std::fs::remove_file(&source).unwrap();
        assert_eq!(
            wimlib_extract_image(destination, 1, cpath(&target).as_ptr(), 0),
            0
        );
        wimlib_free(destination);
    }
    let file = target.join("nested/file");
    let alias = target.join("alias");
    assert_eq!(std::fs::read(&file).unwrap(), data);
    assert_eq!(
        std::fs::metadata(&file).unwrap().ino(),
        std::fs::metadata(&alias).unwrap().ino()
    );
    assert!(std::fs::metadata(&file).unwrap().blocks() * 512 < data.len() as u64);
    assert_eq!(std::fs::metadata(target.join("named")).unwrap().len(), 0);
    assert!(!target.join("named:alternate").exists());
}

#[test]
fn existing_parent_symlink_is_rejected_without_writing_outside_target() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.wim");
    build(&source);
    let original = std::fs::read(&source).unwrap();
    let target = fixture.0.join("output");
    let outside = fixture.0.join("outside");
    std::fs::create_dir(&target).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"preserved").unwrap();
    std::os::unix::fs::symlink(&outside, target.join("nested")).unwrap();
    let mut handle = ptr::null_mut();
    // SAFETY: Source path and handle are valid and exclusively used for the call.
    unsafe {
        assert_eq!(wimlib_open_wim(cpath(&source).as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wimlib_extract_image(handle, 1, cpath(&target).as_ptr(), 0),
            37
        );
        wimlib_free(handle);
    }
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"preserved"
    );
    assert!(!outside.join("file").exists());
}

#[test]
fn selected_paths_preserve_ancestors_and_flatten_selected_hardlink_aliases() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.wim");
    let data = build(&source);
    let original = std::fs::read(&source).unwrap();
    let preserve = fixture.0.join("preserve");
    let flatten = fixture.0.join("flatten");
    let file = CString::new("nested/file").unwrap();
    let alias = CString::new("alias").unwrap();
    let paths = [file.as_ptr(), alias.as_ptr()];
    let mut handle = ptr::null_mut();
    // SAFETY: Every path and pointer-array entry remains live; handle is used exclusively.
    unsafe {
        assert_eq!(wimlib_open_wim(cpath(&source).as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wimlib_extract_paths(handle, 1, cpath(&preserve).as_ptr(), paths.as_ptr(), 1, 0),
            0
        );
        assert_eq!(
            wimlib_extract_paths(
                handle,
                1,
                cpath(&flatten).as_ptr(),
                paths.as_ptr(),
                2,
                0x200000
            ),
            0
        );
        wimlib_free(handle);
    }
    assert_eq!(std::fs::read(preserve.join("nested/file")).unwrap(), data);
    assert!(!preserve.join("alias").exists());
    assert!(!preserve.join("named").exists());
    assert_eq!(std::fs::read(flatten.join("file")).unwrap(), data);
    assert_eq!(
        std::fs::metadata(flatten.join("file")).unwrap().ino(),
        std::fs::metadata(flatten.join("alias")).unwrap().ino()
    );
    assert!(!flatten.join("nested").exists());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn utf16_pathlist_selects_quoted_paths_and_ignores_comments() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.wim");
    let data = build(&source);
    let list = fixture.0.join("paths.txt");
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(
        "# comment\n 'nested/file'\r\n; ignored\n\"named\""
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    std::fs::write(&list, &bytes).unwrap();
    let target = fixture.0.join("target");
    let mut handle = ptr::null_mut();
    // SAFETY: Source, list and target strings are live terminated paths; handle is exclusive.
    unsafe {
        assert_eq!(wimlib_open_wim(cpath(&source).as_ptr(), 0, &mut handle), 0);
        assert_eq!(
            wimlib_extract_pathlist(handle, 1, cpath(&target).as_ptr(), cpath(&list).as_ptr(), 0),
            0
        );
        wimlib_free(handle);
    }
    assert_eq!(std::fs::read(target.join("nested/file")).unwrap(), data);
    assert!(target.join("named").is_file());
    assert!(!target.join("alias").exists());
    assert_eq!(std::fs::read(list).unwrap(), bytes);
}

#[test]
fn pending_rootless_image_fails_path_lookup_while_empty_selection_has_no_output() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target");
    let name = CString::new("Pending").unwrap();
    let root = CString::new("/").unwrap();
    let paths = [root.as_ptr()];
    let mut handle = ptr::null_mut();
    // SAFETY: All pointer storage and strings are live; the handle is freed exactly once.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_empty_image(handle, name.as_ptr(), ptr::null_mut()),
            0
        );
        assert_eq!(
            wimlib_extract_image(handle, 1, cpath(&target).as_ptr(), 0),
            49
        );
        assert_eq!(
            wimlib_extract_paths(handle, 1, cpath(&target).as_ptr(), paths.as_ptr(), 1, 0),
            49
        );
        assert_eq!(
            wimlib_extract_paths(handle, 1, cpath(&target).as_ptr(), ptr::null(), 0, 0),
            0
        );
        assert!(!target.exists());
        assert_eq!(
            wimlib_extract_paths(handle, 1, cpath(&target).as_ptr(), ptr::null(), 0, 0x200000),
            0
        );
        assert!(target.is_dir());
        wimlib_free(handle);
    }
}
