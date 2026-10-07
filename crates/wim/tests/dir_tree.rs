#![cfg(not(windows))]
use std::ffi::{CStr, c_int, c_void};
use wim::engine::handles::{HandleImage, OwnedImage};
use wim::ffi::{
    DirTreeCallback, WimDirEntry, WimHandle, WimStreamEntry, wimlib_create_new_wim, wimlib_free,
    wimlib_iterate_dir_tree,
};
use wim_format::{
    metadata_write::{OwnedDentry, OwnedMetadata},
    xml::XmlInfo,
};
fn name(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
unsafe fn fixture(missing_root: bool) -> *mut WimHandle {
    let mut handle = std::ptr::null_mut();
    // SAFETY: Writable pointer output storage is supplied.
    assert_eq!(unsafe { wimlib_create_new_wim(0, &mut handle) }, 0);
    let mut root = OwnedDentry::new(Vec::new(), 16);
    root.children = vec![1];
    if missing_root {
        root.main_hash = [1; 20];
    }
    let mut directory = OwnedDentry::new(name("dir"), 16);
    directory.children = vec![2];
    let file = OwnedDentry::new(name("file"), 128);
    let bytes = OwnedMetadata {
        security_descriptors: Vec::new(),
        nodes: vec![root, directory, file],
    }
    .encode()
    .unwrap();
    // SAFETY: Newly created handle is exclusively owned and live.
    unsafe {
        (*handle).header.image_count = 1;
        *(*handle).xml = XmlInfo::parse("<WIM><IMAGE INDEX=\"1\"/></WIM>").unwrap();
        (*handle).images.push(HandleImage::Owned(OwnedImage {
            metadata: bytes,
            hash: [0; 20],
            descriptor: None,
            identity: (1, 1),
            pending: None,
        }));
    }
    handle
}
#[derive(Default)]
struct Context {
    paths: Vec<Vec<u8>>,
    stop: usize,
}
unsafe extern "C" fn callback(entry: *const WimDirEntry, context: *mut c_void) -> c_int {
    // SAFETY: Library supplies a live borrowed entry and caller supplies Context.
    let (entry, context) = unsafe { (&*entry, &mut *context.cast::<Context>()) };
    // SAFETY: Callback path storage is terminated and readable during the call.
    context.paths.push(
        unsafe { CStr::from_ptr(entry.full_path) }
            .to_bytes()
            .to_vec(),
    );
    // SAFETY: Every callback entry includes at least one flexible-array stream.
    let default = unsafe {
        &*std::ptr::from_ref(entry)
            .byte_add(std::mem::size_of::<WimDirEntry>())
            .cast::<WimStreamEntry>()
    };
    assert!(default.stream_name.is_null());
    assert_eq!(default.reserved, [0; 4]);
    if context.stop != 0 && context.paths.len() == context.stop {
        -123
    } else {
        0
    }
}
unsafe fn visit(
    handle: *mut WimHandle,
    path: *const std::ffi::c_char,
    flags: c_int,
    context: &mut Context,
) -> c_int {
    let cb: DirTreeCallback = callback;
    // SAFETY: Live fixture handle, terminated path, callback and context are supplied.
    unsafe {
        wimlib_iterate_dir_tree(
            handle,
            1,
            path,
            flags,
            Some(cb),
            std::ptr::from_mut(context).cast(),
        )
    }
}
#[test]
fn children_and_recursive_flags_preserve_absolute_paths_and_preorder() {
    // SAFETY: The fixture and context remain live and are accessed sequentially.
    unsafe {
        let handle = fixture(false);
        let mut context = Context::default();
        assert_eq!(visit(handle, c"\\\\dir/".as_ptr(), 1, &mut context), 0);
        assert_eq!(context.paths, vec![b"/dir".to_vec(), b"/dir/file".to_vec()]);
        context.paths.clear();
        assert_eq!(visit(handle, std::ptr::null(), 2, &mut context), 0);
        assert_eq!(context.paths, vec![b"/dir".to_vec()]);
        context.paths.clear();
        assert_eq!(visit(handle, c"/".as_ptr(), 3, &mut context), 0);
        assert_eq!(context.paths, vec![b"/dir".to_vec(), b"/dir/file".to_vec()]);
        wimlib_free(handle);
    }
}
#[test]
fn callback_nonzero_result_stops_before_later_children() {
    // SAFETY: The fixture and context remain live and are accessed sequentially.
    unsafe {
        let handle = fixture(false);
        let mut context = Context {
            stop: 2,
            ..Context::default()
        };
        assert_eq!(visit(handle, c"/".as_ptr(), 1, &mut context), -123);
        assert_eq!(context.paths, vec![b"/".to_vec(), b"/dir".to_vec()]);
        wimlib_free(handle);
    }
}
#[test]
fn resources_needed_checks_skipped_parent_before_children_callback() {
    // SAFETY: The fixture and context remain live and are accessed sequentially.
    unsafe {
        let handle = fixture(true);
        let mut context = Context::default();
        assert_eq!(visit(handle, c"/".as_ptr(), 6, &mut context), 55);
        assert!(context.paths.is_empty());
        assert_eq!(visit(handle, c"/".as_ptr(), 2, &mut context), 0);
        assert_eq!(context.paths, vec![b"/dir".to_vec()]);
        wimlib_free(handle);
    }
}
