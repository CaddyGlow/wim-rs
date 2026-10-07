#![cfg(unix)]
mod common;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    ptr,
};
thread_local! {
    static LIMITED: Cell<bool> = const { Cell::new(false) };
    static MAXIMUM: Cell<usize> = const { Cell::new(0) };
}
struct TrackingAllocator;
// SAFETY: All allocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if LIMITED.with(Cell::get) {
            MAXIMUM.with(|maximum| maximum.set(maximum.get().max(layout.size())));
        }
        // SAFETY: The caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Pointer and layout originate from System.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if LIMITED.with(Cell::get) {
            MAXIMUM.with(|maximum| maximum.set(maximum.get().max(size)));
        }
        // SAFETY: Pointer and layout originate from System.
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;
#[test]
fn sparse_large_input_opens_with_bounded_allocations_and_reports_later_truncation() {
    let path = std::env::temp_dir().join(format!("wim-file-backed-{}.wim", std::process::id()));
    const INPUT: &[u8] = include_bytes!("fixtures/wim-format/xpress-resource.wim");
    std::fs::write(&path, INPUT).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(8 * 1024 * 1024 * 1024).unwrap();
    let name = common::text(path.to_str().unwrap());
    let mut handle = ptr::null_mut();
    // SAFETY: Terminated path and output storage stay live throughout the calls.
    unsafe {
        LIMITED.with(|limited| limited.set(true));
        let status = wim::ffi::wimlib_open_wim(name.as_ptr(), 0, &mut handle);
        LIMITED.with(|limited| limited.set(false));
        assert_eq!(status, 0);
        assert!(MAXIMUM.with(Cell::get) < 1024 * 1024);
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 0);
        file.set_len(208).unwrap();
        assert_eq!(wim::ffi::wimlib_verify_wim(handle, 0), 65);
        wim::ffi::wimlib_free(handle);
    }
    std::fs::remove_file(path).unwrap();
}
