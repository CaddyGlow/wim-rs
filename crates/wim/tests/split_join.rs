mod common;
use wim::ffi::{wimlib_create_new_wim, wimlib_free, wimlib_join, wimlib_split};
#[test]
fn split_ignores_orphan_solid_descriptors_after_duplicate_blob_resolution() {
    const SOURCE: &[u8] = include_bytes!("fixtures/orphan-solid.wim");
    let archive = wim_format::archive::Archive::open(SOURCE).unwrap();
    assert!(
        archive
            .lookup
            .resources
            .iter()
            .any(|resource| resource.solid)
    );
    assert!(
        archive
            .lookup
            .blobs
            .iter()
            .all(|blob| !archive.lookup.resources[blob.resource_index].solid)
    );
    let directory =
        std::env::temp_dir().join(format!("wim-orphan-solid-split-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.wim");
    let target = directory.join("part.swm");
    std::fs::write(&source, SOURCE).unwrap();
    let source_name = common::text(source.to_str().unwrap());
    let target_name = common::text(target.to_str().unwrap());
    let mut handle = std::ptr::null_mut();
    // SAFETY: The test owns its handle and terminated paths throughout each call.
    unsafe {
        assert_eq!(
            wim::ffi::wimlib_open_wim(source_name.as_ptr(), 0, &mut handle),
            0
        );
        assert_eq!(wimlib_split(handle, target_name.as_ptr(), 190817, 1), 0);
        wimlib_free(handle);
        assert_eq!(
            wim::ffi::wimlib_open_wim(target_name.as_ptr(), 1, &mut handle),
            0
        );
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        wimlib_free(handle);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn split_and_join_validate_empty_names_sizes_and_part_counts() {
    let mut handle = std::ptr::null_mut();
    let path = common::text("/tmp/wim-split-unused.swm");
    // SAFETY: Live owned handle and terminated strings; failure paths do not inspect null arrays.
    unsafe {
        assert_eq!(wimlib_create_new_wim(1, &mut handle), 0);
        assert_eq!(wimlib_split(handle, path.as_ptr(), 0, 0), 24);
        assert_eq!(wimlib_split(handle, std::ptr::null(), 1, 0), 24);
        assert_eq!(wimlib_join(std::ptr::null(), 0, path.as_ptr(), 0, 0), 24);
        assert_eq!(
            wimlib_join(std::ptr::null(), 65536, path.as_ptr(), 0, 0),
            24
        );
        wimlib_free(handle);
    }
}

#[test]
fn split_filenames_and_reverse_join_preserve_content_and_current_xml() {
    use wim_format::archive::Archive;
    const SOURCE: &[u8] = include_bytes!("../../wim-format/tests/fixtures/xpress-resource.wim");
    let directory = std::env::temp_dir().join(format!("wim-split-join-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.wim");
    std::fs::write(&source, SOURCE).unwrap();
    let path = |p: &std::path::Path| common::text(p.to_str().unwrap());
    let source_path = path(&source);
    let first = directory.join("archive.name.swm");
    let second = directory.join("archive.name2.swm");
    let joined = directory.join("joined.wim");
    let first_path = path(&first);
    let second_path = path(&second);
    let joined_path = path(&joined);
    let mut handle = std::ptr::null_mut();
    let name = common::text("edited");
    // SAFETY: Owned handle and terminated paths are live; pointer arrays have exact count.
    unsafe {
        assert_eq!(
            wim::ffi::wimlib_open_wim(source_path.as_ptr(), 0, &mut handle),
            0
        );
        assert_eq!(wim::ffi::wimlib_set_image_name(handle, 1, name.as_ptr()), 0);
        assert_eq!(wimlib_split(handle, first_path.as_ptr(), 1, 1 | 0x800), 0);
        assert!(first.exists() && second.exists());
        let paths = [second_path.as_ptr(), first_path.as_ptr()];
        assert_eq!(
            wimlib_join(paths.as_ptr(), 2, joined_path.as_ptr(), 1, 1),
            0
        );
        let bytes = std::fs::read(&joined).unwrap();
        let actual = Archive::open(&bytes).unwrap();
        let original = Archive::open(SOURCE).unwrap();
        assert_eq!(actual.header.total_parts, 1);
        assert_eq!(actual.header.guid, original.header.guid);
        assert_eq!(actual.xml().unwrap().name(1), Some("edited"));
        actual.check_integrity().unwrap();
        for blob in &original.lookup.blobs {
            assert_eq!(
                actual.read_blob(&blob.hash).unwrap(),
                original.read_blob(&blob.hash).unwrap()
            );
        }
        assert_eq!(std::fs::read(&source).unwrap(), SOURCE);
        assert_eq!(
            wimlib_join(paths.as_ptr(), 1, joined_path.as_ptr(), 0, 0),
            62
        );
        let duplicate = [first_path.as_ptr(), first_path.as_ptr()];
        assert_eq!(
            wimlib_join(duplicate.as_ptr(), 2, joined_path.as_ptr(), 0, 0),
            62
        );
        wimlib_free(handle);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
