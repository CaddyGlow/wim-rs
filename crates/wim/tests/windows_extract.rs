#![cfg(windows)]
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
use wim::ffi::{
    WimHandle, wimlib_extract_image, wimlib_extract_pathlist, wimlib_extract_paths, wimlib_free,
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
        Self::under(&std::env::temp_dir())
    }
    fn under(parent: &Path) -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let root = parent.join(format!(
            "wim-windows-extract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Directory reparse points must be removed without following their targets.
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn name(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
#[ignore = "requires elevated Windows DISM and NTFS"]
fn dism_mount_keeps_empty_directories_writable() {
    let fixture = Fixture::new();
    let image = fixture.0.join("empty-directories.wim");
    let mount = fixture.0.join("mount");
    std::fs::create_dir(&mount).unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 2, 3];
    let mut parent = OwnedDentry::new(name("parent"), 0x10);
    parent.children = vec![4];
    let mut builder = ImageBuilder::new([41; 16]);
    builder
        .add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: vec![],
                nodes: vec![
                    root,
                    OwnedDentry::new(name("empty"), 0x10),
                    OwnedDentry::new(name("empty-file"), 0x20),
                    parent,
                    OwnedDentry::new(name("nested-empty"), 0x10),
                ],
            },
            name: Some("Empty directories".into()),
            description: None,
            properties: vec![],
        })
        .unwrap();
    std::fs::write(
        &image,
        builder
            .write(WriteOptions {
                compression: Compression::Xpress,
                chunk_size: 32768,
                integrity: false,
            })
            .unwrap(),
    )
    .unwrap();
    let result = std::process::Command::new("dism.exe")
        .args([
            "/English".into(),
            "/Mount-Wim".into(),
            format!("/WimFile:{}", image.display()),
            "/Index:1".into(),
            format!("/MountDir:{}", mount.display()),
        ])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    let check = (|| -> std::io::Result<()> {
        if !mount.join("empty").is_dir()
            || !mount.join("parent/nested-empty").is_dir()
            || !mount.join("empty-file").is_file()
        {
            return Err(std::io::Error::other(
                "DISM changed empty directory/file types",
            ));
        }
        std::fs::create_dir(mount.join("empty/session"))?;
        std::fs::write(mount.join("parent/nested-empty/payload"), b"writable")
    })();
    let result = std::process::Command::new("dism.exe")
        .args([
            "/English".into(),
            "/Unmount-Wim".into(),
            format!("/MountDir:{}", mount.display()),
            "/Discard".into(),
        ])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    check.unwrap();
}
fn build(path: &Path, unsupported: bool) {
    let mut builder = ImageBuilder::new([17; 16]);
    let hash = builder.add_blob(b"main payload").unwrap();
    let ads = builder.add_blob(b"named payload").unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 3, 4];
    root.creation_time = 132000000000000000;
    root.extra_streams.push(OwnedStream {
        hash: ads,
        name: name("root-ads"),
        ..OwnedStream::default()
    });
    let mut dir = OwnedDentry::new(name("nested"), 0x10);
    dir.children = vec![2];
    dir.extra_streams.push(OwnedStream {
        hash: ads,
        name: name("directory-ads"),
        ..OwnedStream::default()
    });
    let mut file = OwnedDentry::new(name("file-é漢"), 0x20);
    // A zero implicit hash followed by the real unnamed stream occurs in native WIMs.
    file.extra_streams = vec![
        OwnedStream {
            hash,
            ..OwnedStream::default()
        },
        OwnedStream {
            hash: ads,
            name: name("ads"),
            ..OwnedStream::default()
        },
        OwnedStream {
            name: name("empty"),
            ..OwnedStream::default()
        },
    ];
    file.inode_union = 19;
    let mut alias = file.clone();
    alias.name = name("alias");
    let mut special = OwnedDentry::new(name("special"), 0x20);
    if unsupported {
        special
            .tagged_items
            .extend_from_slice(&0x337dd874u32.to_le_bytes());
        special.tagged_items.extend_from_slice(&0u32.to_le_bytes());
    }
    builder
        .add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: vec![],
                nodes: vec![root, dir, file, alias, special],
            },
            name: Some("Windows streams".into()),
            description: None,
            properties: vec![],
        })
        .unwrap();
    std::fs::write(
        path,
        builder
            .write(WriteOptions {
                compression: Compression::Xpress,
                chunk_size: 32768,
                integrity: false,
            })
            .unwrap(),
    )
    .unwrap();
}
struct Handle(*mut WimHandle);
impl Handle {
    fn open(path: &Path) -> Self {
        let mut handle = ptr::null_mut();
        // SAFETY: Terminated filename and writable handle output remain live.
        assert_eq!(
            unsafe { wimlib_open_wim(wide(path).as_ptr(), 0, &mut handle) },
            0
        );
        Self(handle)
    }
    fn paths(&self, target: &Path, paths: &[&str], flags: i32) -> i32 {
        let names: Vec<_> = paths.iter().map(|p| wide(Path::new(p))).collect();
        let pointers: Vec<_> = names.iter().map(|p| p.as_ptr()).collect();
        // SAFETY: Live exclusive handle, terminated target and all path-array strings.
        unsafe {
            wimlib_extract_paths(
                self.0,
                1,
                wide(target).as_ptr(),
                pointers.as_ptr(),
                pointers.len(),
                flags,
            )
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the live handle.
        unsafe { wimlib_free(self.0) };
    }
}
#[test]
fn whole_image_restores_file_directory_and_empty_named_streams_and_hardlinks() {
    let fixture = Fixture::new();
    let input = fixture.0.join("input.wim");
    build(&input, false);
    let before = std::fs::read(&input).unwrap();
    let handle = Handle::open(&input);
    let output = fixture.0.join("output");
    // SAFETY: Handle and terminated target remain valid throughout extraction.
    assert_eq!(
        unsafe { wimlib_extract_image(handle.0, 1, wide(&output).as_ptr(), 0) },
        0
    );
    assert_eq!(
        std::fs::read(output.join("nested/file-é漢")).unwrap(),
        b"main payload"
    );
    assert_eq!(
        std::fs::read(output.join("alias:ads")).unwrap(),
        b"named payload"
    );
    assert_eq!(std::fs::read(output.join("alias:empty")).unwrap(), b"");
    assert_eq!(
        std::fs::read(output.join("nested:directory-ads")).unwrap(),
        b"named payload"
    );
    assert_eq!(
        std::fs::read(output.with_file_name("output:root-ads")).unwrap(),
        b"named payload"
    );
    std::fs::write(output.join("alias:ads"), b"hardlink stream edit").unwrap();
    assert_eq!(
        std::fs::read(output.join("nested/file-é漢:ads")).unwrap(),
        b"hardlink stream edit"
    );
    assert_eq!(std::fs::read(input).unwrap(), before);
}
#[test]
fn selected_paths_ignore_unselected_unsupported_metadata_and_flatten_aliases() {
    let fixture = Fixture::new();
    let input = fixture.0.join("input.wim");
    build(&input, true);
    let handle = Handle::open(&input);
    let preserve = fixture.0.join("preserve");
    assert_eq!(handle.paths(&preserve, &["nested/file-é漢"], 0), 0);
    assert_eq!(
        std::fs::read(preserve.join("nested/file-é漢:ads")).unwrap(),
        b"named payload"
    );
    assert!(!preserve.join("special").exists());
    assert!(!preserve.join("alias").exists());
    let flat = fixture.0.join("flat");
    assert_eq!(
        handle.paths(&flat, &["nested/file-é漢", "alias"], 0x200000),
        0
    );
    assert!(!flat.join("nested").exists());
    assert!(std::fs::read(flat.with_file_name("flat:root-ads")).is_err());
    use std::os::windows::fs::MetadataExt;
    assert_ne!(
        std::fs::metadata(&flat).unwrap().creation_time(),
        132000000000000000
    );
    std::fs::write(flat.join("alias"), b"hardlink edit").unwrap();
    assert_eq!(
        std::fs::read(flat.join("file-é漢")).unwrap(),
        b"hardlink edit"
    );
    assert_eq!(
        handle.paths(&fixture.0.join("rejected"), &["special"], 0),
        68
    );
    assert!(!fixture.0.join("rejected").exists());
}
#[test]
fn wildcard_and_utf16_pathlist_extract_only_matching_subtrees() {
    let fixture = Fixture::new();
    let input = fixture.0.join("input.wim");
    build(&input, false);
    let handle = Handle::open(&input);
    let output = fixture.0.join("glob");
    assert_eq!(handle.paths(&output, &["nested/*"], 0x40000), 0);
    assert!(output.join("nested/file-é漢").exists());
    assert!(!output.join("alias").exists());
    assert_eq!(
        handle.paths(&fixture.0.join("missing"), &["missing/*"], 0xc0000),
        49
    );
    let list = fixture.0.join("paths.txt");
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(
        "# ignored\n 'nested/file-é漢'\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    std::fs::write(&list, bytes).unwrap();
    let output = fixture.0.join("list");
    // SAFETY: Live handle and terminated target/list strings remain valid.
    assert_eq!(
        unsafe {
            wimlib_extract_pathlist(handle.0, 1, wide(&output).as_ptr(), wide(&list).as_ptr(), 0)
        },
        0
    );
    assert!(output.join("nested/file-é漢:ads").exists());
    assert!(!output.join("alias").exists());
}

#[test]
#[ignore = "requires NTFS with EFS enabled and permission to create an EFS certificate"]
fn encrypted_capture_restores_raw_data_before_and_after_wim_serialization() {
    use std::os::windows::fs::MetadataExt;
    // EFS is forbidden in Windows' SYSTEM temporary directory.
    let public = PathBuf::from(std::env::var_os("PUBLIC").expect("Windows PUBLIC directory"));
    let fixture = Fixture::under(&public);
    let source = fixture.0.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("encrypted"), b"ciphertext round trip").unwrap();
    std::fs::write(source.join("encrypted:ads"), b"encrypted stream").unwrap();
    let encrypt = std::process::Command::new("cipher.exe")
        .args(["/e", "/a"])
        .arg(source.join("encrypted"))
        .output()
        .unwrap();
    assert!(
        encrypt.status.success(),
        "{}",
        format!(
            "{} {}",
            String::from_utf8_lossy(&encrypt.stdout),
            String::from_utf8_lossy(&encrypt.stderr)
        )
    );
    assert_ne!(
        std::fs::metadata(source.join("encrypted"))
            .unwrap()
            .file_attributes()
            & 0x4000,
        0
    );
    std::fs::hard_link(source.join("encrypted"), source.join("alias")).unwrap();
    let mut captured = ptr::null_mut();
    // SAFETY: Writable handle output and supported compression type.
    assert_eq!(
        unsafe { wim::ffi::wimlib_create_new_wim(0, &mut captured) },
        0
    );
    let captured = Handle(captured);
    // SAFETY: Live handle and terminated source pathname; optional arguments are null.
    assert_eq!(
        unsafe {
            wim::ffi::wimlib_add_image(
                captured.0,
                wide(&source).as_ptr(),
                ptr::null(),
                ptr::null(),
                0,
            )
        },
        0
    );
    let check = |handle: &Handle, target: &Path| {
        // SAFETY: Live handle and terminated extraction pathname.
        assert_eq!(
            unsafe { wimlib_extract_image(handle.0, 1, wide(target).as_ptr(), 0) },
            0
        );
        assert_eq!(
            std::fs::read(target.join("encrypted")).unwrap(),
            b"ciphertext round trip"
        );
        assert_eq!(
            std::fs::read(target.join("alias:ads")).unwrap(),
            b"encrypted stream"
        );
        assert_ne!(
            std::fs::metadata(target.join("encrypted"))
                .unwrap()
                .file_attributes()
                & 0x4000,
            0
        );
        std::fs::write(target.join("alias"), b"encrypted alias edit").unwrap();
        assert_eq!(
            std::fs::read(target.join("encrypted")).unwrap(),
            b"encrypted alias edit"
        );
    };
    check(&captured, &fixture.0.join("direct"));
    let output = fixture.0.join("captured.wim");
    // SAFETY: Live captured handle, terminated output and documented all-image selection.
    assert_eq!(
        unsafe { wim::ffi::wimlib_write(captured.0, wide(&output).as_ptr(), -1, 0, 1) },
        0
    );
    drop(captured);
    let reopened = Handle::open(&output);
    check(&reopened, &fixture.0.join("reopened"));
    drop(reopened);
    assert_eq!(
        std::fs::read(source.join("encrypted")).unwrap(),
        b"ciphertext round trip"
    );
    let prefix = format!("wim-efs-{}-", std::process::id());
    assert!(
        !std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&prefix)
            }),
        "EFS spool remains after all owners are released"
    );
}
