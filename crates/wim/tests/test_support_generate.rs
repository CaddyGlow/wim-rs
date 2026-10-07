//! Optional generated capture owns true immediately hashed memory resources.
#![cfg(feature = "test-support")]
use std::sync::Mutex;
use wim::ffi::*;
static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn generated_memory_survives_export_and_source_release() {
    let _guard = SERIAL.lock().unwrap();
    let mut source = std::ptr::null_mut();
    let mut destination = std::ptr::null_mut();
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut source), 0);
        assert_eq!(wimlib_create_new_wim(0, &mut destination), 0);
        test_support::wimlib_seed_random(0);
        assert_eq!(
            wimlib_add_image(
                source,
                std::ptr::dangling(),
                std::ptr::null(),
                std::ptr::null(),
                0x08000200
            ),
            0
        );
        assert!(!(*source).owned_blobs.is_empty());
        for (hash, blob) in (*source).owned_blobs.iter() {
            assert!(blob.bytes.is_memory());
            assert!(blob.backing.is_none());
            assert_eq!(
                *hash,
                <[u8; 20]>::from(sha1::Sha1::digest(blob.bytes.as_slice()))
            );
        }
        assert_eq!(wimlib_verify_wim(source, 0), 0);
        assert_eq!(
            wimlib_export_image(
                source,
                1,
                destination,
                std::ptr::null(),
                std::ptr::null(),
                0
            ),
            0
        );
        assert_eq!(
            test_support::wimlib_compare_images(source, 1, destination, 1, 0),
            0
        );
        wimlib_free(source);
        assert_eq!(wimlib_verify_wim(destination, 0), 0);
        for blob in (*destination).owned_blobs.values() {
            assert!(blob.bytes.is_memory());
            assert!(!blob.bytes.is_empty());
        }
        wimlib_free(destination);
    }
}
unsafe extern "C" fn count_resources(
    _entry: *const WimResourceEntry,
    context: *mut std::ffi::c_void,
) -> i32 {
    unsafe {
        *context.cast::<usize>() += 1;
    }
    0
}
struct AbortState {
    handle: *mut WimHandle,
    events: Vec<i32>,
    end_had_memory: bool,
}
unsafe extern "C" fn abort_end(
    event: i32,
    _info: *mut ProgressInfo,
    context: *mut std::ffi::c_void,
) -> i32 {
    let state = unsafe { &mut *context.cast::<AbortState>() };
    state.events.push(event);
    if event == 11 {
        state.end_had_memory = unsafe { !(*state.handle).owned_blobs.is_empty() };
        return 1;
    }
    0
}
#[test]
fn end_abort_observes_hashed_resources_then_rolls_back_image_and_index() {
    let _guard = SERIAL.lock().unwrap();
    let mut handle = std::ptr::null_mut();
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        let mut state = AbortState {
            handle,
            events: Vec::new(),
            end_had_memory: false,
        };
        wimlib_register_progress_function(
            handle,
            Some(abort_end),
            std::ptr::from_mut(&mut state).cast(),
        );
        test_support::wimlib_seed_random(0);
        assert_eq!(
            wimlib_add_image(
                handle,
                std::ptr::dangling(),
                std::ptr::null(),
                std::ptr::null(),
                0x08000200
            ),
            76
        );
        assert_eq!(state.events, [9, 11]);
        assert!(state.end_had_memory);
        assert_eq!((*handle).header.image_count, 0);
        assert!((*handle).owned_blobs.is_empty());
        let mut count = 0usize;
        assert_eq!(
            wimlib_iterate_lookup_table(
                handle,
                0,
                Some(count_resources),
                std::ptr::from_mut(&mut count).cast()
            ),
            0
        );
        assert_eq!(count, 0);
        wimlib_free(handle);
    }
}
use sha1::Digest;

use wim::engine::test_support;
