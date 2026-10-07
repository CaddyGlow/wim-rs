#![cfg(any(unix, windows))]
#![forbid(unsafe_code)]
use std::{ffi::OsStr, path::PathBuf};
use wim::{Compression, ImageIndex, Wim};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("wim-rust-capture-rollback-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_native_capture_and_add_preserve_image_and_allow_retry() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("file"), b"original").unwrap();
    let missing = fixture.0.join("missing");
    let mut archive = Wim::new(Compression::None).unwrap();
    assert!(archive.capture_image(&missing).is_err());
    assert_eq!(archive.info().unwrap().image_count, 0);
    archive.capture_image(&source).unwrap();
    let image = ImageIndex::try_from(1).unwrap();
    assert!(archive.add_file(image, &missing, "/file").is_err());
    assert_eq!(archive.info().unwrap().image_count, 1);
    let before = fixture.0.join("before");
    archive
        .extract_path(image, OsStr::new("/file"), &before)
        .unwrap();
    assert_eq!(std::fs::read(before.join("file")).unwrap(), b"original");
    std::fs::write(&missing, b"replacement").unwrap();
    archive.add_file(image, &missing, "/file").unwrap();
    let after = fixture.0.join("after");
    archive
        .extract_path(image, OsStr::new("/file"), &after)
        .unwrap();
    assert_eq!(std::fs::read(after.join("file")).unwrap(), b"replacement");
}
