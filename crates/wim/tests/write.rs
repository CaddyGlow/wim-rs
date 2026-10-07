mod common;
use wim::ffi::{wimlib_create_new_wim, wimlib_free, wimlib_write};
#[test]
fn write_rejects_invalid_arguments_before_touching_output() {
    let mut handle = std::ptr::null_mut();
    let path = common::text("/tmp/wim-must-not-write.wim");
    // SAFETY: Valid handle and terminated path, all calls sequential.
    unsafe {
        assert_eq!(wimlib_create_new_wim(1, &mut handle), 0);
        assert_eq!(wimlib_write(handle, path.as_ptr(), 1, 0, 1), 18);
        assert_eq!(wimlib_write(handle, path.as_ptr(), -1, 3, 1), 24);
        assert_eq!(wimlib_write(handle, std::ptr::null(), -1, 0, 1), 24);
        assert_eq!(wimlib_write(handle, path.as_ptr(), -1, 0x10000, 1), 24);
        wimlib_free(handle);
    }
}

#[test]
fn buffered_writer_persists_pending_empty_images_with_independent_solid_settings() {
    use wim_format::{Compression, archive::Archive, metadata::Metadata};
    for codec in 0..4 {
        for mode in [0, 4, 0x1000] {
            let mut handle = std::ptr::null_mut();
            let path = std::env::temp_dir().join(format!(
                "wim-write-{}-{codec}-{mode}.wim",
                std::process::id()
            ));
            let path_string = common::text(path.to_str().unwrap());
            let name = common::text("empty");
            // SAFETY: Handle and strings are owned by the test and used sequentially.
            unsafe {
                assert_eq!(wimlib_create_new_wim(codec, &mut handle), 0);
                assert_eq!(
                    wim::ffi::wimlib_add_empty_image(handle, name.as_ptr(), std::ptr::null_mut()),
                    0
                );
                assert_eq!(
                    wimlib_write(handle, path_string.as_ptr(), -1, mode | 0x800 | 1, 1),
                    0
                );
                let bytes = std::fs::read(&path).unwrap();
                let archive = Archive::open(&bytes).unwrap();
                assert_eq!(
                    archive.header.validate_compression().unwrap(),
                    Compression::from_i32(codec).unwrap()
                );
                assert_eq!(archive.xml().unwrap().name(1), Some("empty"));
                archive.check_integrity().unwrap();
                assert_eq!(
                    Metadata::parse(&archive.read_metadata(1).unwrap())
                        .unwrap()
                        .nodes
                        .len(),
                    1
                );
                let wim::engine::handles::HandleImage::Empty(empty) = &(&(*handle).images)[0]
                else {
                    panic!("expected pending owned image")
                };
                let metadata = empty.shared.lock().unwrap();
                assert_ne!(metadata.hash, [0; 20]);
                assert_eq!(archive.lookup.metadata[0].hash, metadata.hash);
                drop(metadata);
                wimlib_free(handle);
            }
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn write_to_nonseekable_descriptor_requires_pipable_and_preserves_descriptor_ownership() {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let (mut reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut handle = std::ptr::null_mut();
    // SAFETY: Live handle, caller-owned socket descriptor, and sequential calls.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wim::ffi::wimlib_write_to_fd(handle, writer.as_raw_fd(), -1, 0, 1),
            24
        );
        assert_eq!(
            wim::ffi::wimlib_write_to_fd(handle, writer.as_raw_fd(), -1, 5, 1),
            24
        );
        assert_eq!(
            wim::ffi::wimlib_write_to_fd(handle, writer.as_raw_fd(), -1, 4, 1),
            0
        );
        writer.shutdown(std::net::Shutdown::Write).unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        let archive = wim_format::archive::Archive::open(&bytes).unwrap();
        assert_eq!(archive.header.magic, wim_format::PIPABLE_MAGIC);
        wimlib_free(handle);
    }
}
