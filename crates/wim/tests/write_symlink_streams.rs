#![cfg(unix)]
//! Original dentry.c serialization includes implicit empty data for file symlinks.
use std::{
    ffi::CString,
    os::unix::{ffi::OsStrExt, fs::symlink},
};
use wim::ffi::{WimHandle, wimlib_add_image, wimlib_create_new_wim, wimlib_free, wimlib_write};
use wim_format::{
    archive::Archive,
    metadata::{Metadata, StreamType},
};
#[test]
fn captured_file_symlink_keeps_empty_data_and_binding_on_repeated_write() {
    let root = std::env::temp_dir().join(format!("wim-symlink-streams-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(source.join("directory")).unwrap();
    symlink("missing", source.join("file_link")).unwrap();
    symlink("directory", source.join("dir_link")).unwrap();
    let path = CString::new(source.as_os_str().as_bytes()).unwrap();
    let mut wim: *mut WimHandle = std::ptr::null_mut();
    // SAFETY: The handles and terminated strings remain live for each call.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut wim), 0);
        assert_eq!(
            wimlib_add_image(
                wim,
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0x210
            ),
            0
        );
        for (iteration, flags) in [0, 0, 4, 0x1000].into_iter().enumerate() {
            let target = root.join(format!("out{iteration}.wim"));
            let name = CString::new(target.as_os_str().as_bytes()).unwrap();
            assert_eq!(wimlib_write(wim, name.as_ptr(), -1, flags, 1), 0);
            let bytes = std::fs::read(target).unwrap();
            let archive = Archive::open(&bytes).unwrap();
            let raw = archive.read_metadata(1).unwrap();
            let metadata = Metadata::parse(&raw).unwrap();
            for node in &metadata.nodes {
                let name = String::from_utf16(
                    &node
                        .entry
                        .name
                        .chunks_exact(2)
                        .map(|u| u16::from_le_bytes([u[0], u[1]]))
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                if name == "file_link" || name == "dir_link" {
                    let rp = node
                        .entry
                        .streams
                        .iter()
                        .find(|s| s.kind == StreamType::ReparsePoint)
                        .unwrap();
                    assert!(!archive.read_blob(&rp.hash).unwrap().is_empty());
                    let data = node
                        .entry
                        .streams
                        .iter()
                        .find(|s| s.kind == StreamType::Data);
                    if name == "file_link" {
                        assert_eq!(data.unwrap().hash, [0; 20]);
                    } else {
                        assert!(data.is_none());
                    }
                }
            }
        }
        wimlib_free(wim);
    }
    std::fs::remove_dir_all(root).unwrap();
}
