mod common;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use wim::ffi::{WimResourceEntry, wimlib_free, wimlib_iterate_lookup_table, wimlib_open_wim};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                COUNT.with(|count| count.set(count.get() + 1));
            }
        });
        // SAFETY: Forward the original valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Forward the matching allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                COUNT.with(|count| count.set(count.get() + 1));
            }
        });
        // SAFETY: Forward the original allocation and new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

unsafe extern "C" fn count(entry: *const WimResourceEntry, context: *mut c_void) -> i32 {
    // SAFETY: Iterator supplies a valid entry and test passes writable counter.
    unsafe {
        if (*entry).uncompressed_size != 0 {
            *context.cast::<usize>() += 1;
        }
    }
    0
}

#[test]
fn repeated_lookup_iteration_allocates_nothing() {
    let path = common::text(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../wim-format/tests/fixtures/integrity.wim"
    ));
    let mut handle = std::ptr::null_mut();
    // SAFETY: Path is terminated and output storage writable.
    assert_eq!(unsafe { wimlib_open_wim(path.as_ptr(), 0, &mut handle) }, 0);
    let mut seen = 0usize;
    COUNT.with(|count| count.set(0));
    COUNTING.with(|flag| flag.set(true));
    let mut result = 0;
    for _ in 0..3 {
        // SAFETY: Handle is live, callback uses the supplied counter only.
        result |= unsafe {
            wimlib_iterate_lookup_table(handle, 0, Some(count), (&mut seen as *mut usize).cast())
        };
    }
    COUNTING.with(|flag| flag.set(false));
    // SAFETY: Test releases its sole handle ownership.
    unsafe {
        wimlib_free(handle);
    }
    assert_eq!(result, 0);
    assert!(seen > 0);
    assert_eq!(COUNT.with(Cell::get), 0);
}
