#![cfg(any(unix, windows))]
#![forbid(unsafe_code)]
use std::{ffi::OsStr, path::PathBuf};
use wim::{Compression, Error, ImageIndex, OpenOptions, Wim};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wim-rust-api-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn image_indexes_reject_abi_sentinels_and_overflow() {
    for index in [0, i32::MAX as u32 + 1, u32::MAX] {
        assert_eq!(
            ImageIndex::try_from(index),
            Err(Error::InvalidImageIndex(index))
        );
    }
    assert_eq!(
        ImageIndex::try_from(i32::MAX as u32).unwrap().get(),
        i32::MAX as u32
    );
}

#[test]
fn nul_inputs_fail_without_truncating_paths_or_properties() {
    let mut archive = Wim::new(Compression::Lzx).unwrap();
    let index = ImageIndex::try_from(1).unwrap();
    assert_eq!(
        Wim::open(std::path::Path::new("bad\0.wim"), OpenOptions::default()).unwrap_err(),
        Error::InteriorNul
    );
    assert_eq!(
        archive.capture_image(std::path::Path::new("bad\0source")),
        Err(Error::InteriorNul)
    );
    assert_eq!(
        archive.write(std::path::Path::new("bad\0output")),
        Err(Error::InteriorNul)
    );
    assert_eq!(
        archive.set_image_property(index, "NAME", "bad\0value"),
        Err(Error::InteriorNul)
    );
    assert_eq!(
        archive.extract_path(
            index,
            OsStr::new("bad\0path"),
            std::path::Path::new("output")
        ),
        Err(Error::InteriorNul)
    );
    assert_eq!(archive.info().unwrap().image_count, 0);
}

#[test]
fn exported_resources_survive_source_drop_and_commit_requires_reopen() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("café.txt"), b"original content").unwrap();
    let mut captured = Wim::new(Compression::Lzx).unwrap();
    captured.capture_image(&source).unwrap();
    let input = fixture.0.join("input.wim");
    captured.write(&input).unwrap();
    drop(captured);

    let index = ImageIndex::try_from(1).unwrap();
    let mut exported = Wim::new(Compression::Lzx).unwrap();
    let mut original = Wim::open(
        &input,
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
    )
    .unwrap();
    original
        .export_image(index, &mut exported, Some("Exported image"), true)
        .unwrap();
    drop(original);
    let output = fixture.0.join("output.wim");
    exported.write(&output).unwrap();
    drop(exported);

    let mut archive = Wim::open(
        &output,
        OpenOptions {
            check_integrity: true,
            write_access: true,
        },
    )
    .unwrap();
    let replacement = fixture.0.join("replacement");
    std::fs::write(&replacement, b"replacement content").unwrap();
    archive.add_file(index, &replacement, "/café.txt").unwrap();
    archive
        .set_image_property(index, "NAME", "Updated image")
        .unwrap();
    archive.set_boot_index(index).unwrap();
    archive.overwrite().unwrap();

    let mut reopened = Wim::open(
        &output,
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
    )
    .unwrap();
    reopened.verify().unwrap();
    assert_eq!(reopened.info().unwrap().boot_index, 1);
    let extracted = fixture.0.join("extracted");
    reopened
        .extract_path(index, OsStr::new("/café.txt"), &extracted)
        .unwrap();
    assert_eq!(
        std::fs::read(extracted.join("café.txt")).unwrap(),
        b"replacement content"
    );
    let applied = fixture.0.join("applied");
    std::fs::create_dir(&applied).unwrap();
    reopened.extract_image(index, &applied).unwrap();
    assert_eq!(
        std::fs::read(applied.join("café.txt")).unwrap(),
        b"replacement content"
    );
}

#[test]
fn engine_errors_retain_their_code_and_description() {
    let archive = Wim::new(Compression::Lzx).unwrap();
    let error = archive.overwrite().unwrap_err();
    assert_eq!(error, Error::Engine(45));
    assert!(error.to_string().contains("45"));
    assert!(error.to_string().contains("filename"));
}

#[test]
fn safe_information_returns_typed_input_compression_for_each_codec() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("data"), b"compression metadata").unwrap();
    for compression in [
        Compression::None,
        Compression::Xpress,
        Compression::Lzx,
        Compression::Lzms,
    ] {
        let mut archive = Wim::new(compression).unwrap();
        archive.capture_image(&source).unwrap();
        let path = fixture.0.join(format!("{}.wim", compression.as_i32()));
        archive.write(&path).unwrap();
        drop(archive);
        let input = Wim::open(&path, OpenOptions::default()).unwrap();
        let info = input.info().unwrap();
        assert_eq!(info.compression, compression);
        assert_eq!(info.image_count, 1);
        assert_eq!(info.boot_index, 0);
    }
}

#[test]
fn scoped_cancellation_callbacks_run_on_caller_and_are_removed_after_panic() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source-cancel");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("file"), vec![7; 1024 * 1024]).unwrap();
    let mut archive = Wim::new(Compression::None).unwrap();
    archive.capture_image(&source).unwrap();
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let caller = std::thread::current().id();
    let observer = calls.clone();
    let failed = fixture.0.join("cancelled.wim");
    assert_eq!(
        archive.write_with_cancel(&failed, move || {
            assert_eq!(std::thread::current().id(), caller);
            let count = observer.get() + 1;
            observer.set(count);
            if count > 1 {
                panic!("callback panic becomes cooperative cancellation");
            }
            false
        }),
        Err(Error::Engine(76))
    );
    assert!(calls.get() > 1);
    let previous_calls = calls.get();
    let output = fixture.0.join("after-cancel.wim");
    archive.write(&output).unwrap();
    assert_eq!(calls.get(), previous_calls);
    let mut reopened = Wim::open_with_cancel(
        &output,
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    reopened.verify().unwrap();
    assert_eq!(reopened.verify_with_cancel(|| true), Err(Error::Engine(76)));
    reopened.verify().unwrap();
}
