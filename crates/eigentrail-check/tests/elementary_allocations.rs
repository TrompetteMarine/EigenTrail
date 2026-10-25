use eigentrail_core::scalar::{self, point};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct Measured;
thread_local! {
    static ACTIVE: Cell<bool> = const {Cell::new(false)};
    static BYTES: Cell<usize> = const {Cell::new(0)};
}
fn record(bytes: usize) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = BYTES.try_with(|count| count.set(count.get() + bytes));
        }
    });
}
unsafe impl GlobalAlloc for Measured {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Measured = Measured;

#[test]
fn valid_bounded_elementary_calls_allocate_no_rust_heap() {
    BYTES.with(|c| c.set(0));
    ACTIVE.with(|a| a.set(true));
    for x in [
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        0.5,
        1.,
        4040.,
        f64::MAX,
    ] {
        std::hint::black_box(scalar::ln(point(x)).unwrap());
    }
    for x in [-1., -0.5, 0., f64::from_bits(1), 0.5, 1f64.next_down(), 1.] {
        std::hint::black_box(scalar::asin(point(x)).unwrap());
        std::hint::black_box(scalar::acos(point(x)).unwrap());
    }
    ACTIVE.with(|a| a.set(false));
    assert_eq!(BYTES.with(|c| c.get()), 0);
}
