#![cfg(not(windows))]
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr;
use wim::ffi::*;

thread_local! {
    static FAIL_SIZE: Cell<usize> = const { Cell::new(0) };
    static FAILURES: Cell<usize> = const { Cell::new(0) };
}
struct FaultAllocator;
// SAFETY: Successful allocations and all deallocations delegate unchanged to System.
unsafe impl GlobalAlloc for FaultAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if FAIL_SIZE.with(|size| size.get() == layout.size()) {
            FAILURES.with(|count| count.set(count.get() + 1));
            return ptr::null_mut();
        }
        // SAFETY: The allocator receives a valid caller layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Matching live pointer/layout came from System.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: FaultAllocator = FaultAllocator;

#[test]
fn canonical_path_allocation_failure_preserves_image_and_allows_retry() {
    let mut handle = ptr::null_mut();
    let path = std::ffi::CString::new(vec![b'a'; 4097]).unwrap();
    let command = UpdateCommand {
        op: 1,
        data: UpdateCommandData {
            delete: DeleteCommand {
                wim_path: path.as_ptr().cast_mut(),
                delete_flags: 1,
            },
        },
    };
    // SAFETY: Handle, command and terminated strings remain live until free.
    unsafe {
        assert_eq!(wimlib_create_new_wim(0, &mut handle), 0);
        assert_eq!(
            wimlib_add_empty_image(handle, c"Allocation".as_ptr(), ptr::null_mut()),
            0
        );
        FAIL_SIZE.with(|size| size.set(4099));
        let result = wimlib_update_image(handle, 1, &command, 1, 1);
        FAIL_SIZE.with(|size| size.set(0));
        assert_eq!(result, 39);
        assert_eq!(FAILURES.with(Cell::get), 1);
        assert!((*handle).dirty_images.is_empty());
        assert!(matches!((&(*handle).images)[0], HandleImage::Empty(_)));
        assert_eq!(wimlib_update_image(handle, 1, &command, 1, 0), 0);
        wimlib_free(handle);
    }
}

use wim::engine::handles::HandleImage;
