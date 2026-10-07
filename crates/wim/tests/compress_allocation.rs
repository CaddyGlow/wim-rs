use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use wim::engine::compress::*;
thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static FAIL_AT: Cell<Option<usize>> = const { Cell::new(None) };
}
struct FaultAllocator;
unsafe impl GlobalAlloc for FaultAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let fail = ENABLED.with(|enabled| {
            if !enabled.get() {
                return false;
            }
            COUNT.with(|count| {
                count.set(count.get() + 1);
                FAIL_AT.with(|failure| failure.get() == Some(count.get()))
            })
        });
        if fail {
            return std::ptr::null_mut();
        }
        // SAFETY: Delegate caller's allocation layout unchanged.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && ENABLED.with(Cell::get) {
            LIVE.with(|live| live.set(live.get() + 1));
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ENABLED.with(Cell::get) {
            LIVE.with(|live| live.set(live.get() - 1));
        }
        // SAFETY: Matching pointer and layout originated from System.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: FaultAllocator = FaultAllocator;

#[test]
fn factory_maps_workspace_allocation_failures_to_nomem() {
    let sentinel = std::ptr::dangling_mut::<c_void>();
    for codec in 1..=3 {
        let mut handle = sentinel;
        COUNT.with(|count| count.set(0));
        ENABLED.with(|enabled| enabled.set(true));
        // SAFETY: Output points to writable local storage.
        let result = unsafe { wimlib_create_compressor(codec, 32768, 50, &mut handle) };
        ENABLED.with(|enabled| enabled.set(false));
        assert_eq!(result, 0);
        let rust_allocations = COUNT.with(Cell::get);
        assert!(rust_allocations > 0, "codec workspaces use Rust allocation");
        let input = vec![b'A'; 32768];
        let mut output = vec![0; 65536];
        let mut decoder = ms_compress::context::Decompressor::new(codec, input.len()).unwrap();
        let mut decoded = vec![0; input.len()];
        for _ in 0..3 {
            COUNT.with(|count| count.set(0));
            ENABLED.with(|enabled| enabled.set(true));
            // SAFETY: Buffers are disjoint and the live compressor belongs to this test.
            let size = unsafe {
                wimlib_compress(
                    input.as_ptr().cast(),
                    input.len(),
                    output.as_mut_ptr().cast(),
                    output.len(),
                    handle,
                )
            };
            ENABLED.with(|enabled| enabled.set(false));
            assert!(size > 0);
            assert_eq!(
                COUNT.with(Cell::get),
                0,
                "reused compression must not allocate"
            );
            decoder.decompress(&output[..size], &mut decoded).unwrap();
            assert_eq!(decoded, input);
        }
        // SAFETY: The successful factory returned sole ownership of a live handle.
        unsafe {
            wimlib_free_compressor(handle);
        }
        // Codec workspace allocations remain fallible and must not publish
        // a partially constructed handle.
        for index in 1..=rust_allocations {
            handle = sentinel;
            COUNT.with(|count| count.set(0));
            FAIL_AT.with(|failure| failure.set(Some(index)));
            LIVE.with(|live| live.set(0));
            ENABLED.with(|enabled| enabled.set(true));
            // SAFETY: Output points to writable local storage.
            let result = unsafe { wimlib_create_compressor(codec, 32768, 50, &mut handle) };
            ENABLED.with(|enabled| enabled.set(false));
            FAIL_AT.with(|failure| failure.set(None));
            assert_eq!(result, 39, "codec {codec} Rust allocation {index}");
            assert_eq!(
                LIVE.with(Cell::get),
                0,
                "partial Rust workspace must be released"
            );
            assert_eq!(handle, sentinel);
        }
    }
}
