//! meld#427 fixture — the RE-EXPORTER, and the component this fixture was
//! missing.
//!
//! meld established that handle tables are allocated only for components that
//! re-export a resource interface (`allocate_handle_tables` walks re-exporters),
//! which is why the two-component version of this fixture allocated ZERO tables
//! — and therefore why "two components sharing a memory domain have mutually
//! addressable handle tables" was read off allocation code rather than observed.
//!
//! This component imports `caps` and exports it again, so a table must exist. It
//! also holds a handle of its own in `mid_run`, so the table is populated rather
//! than merely present.
//!
//! In gust's terms this is the shape that matters: a supervisor hands a
//! capability to tenant A, tenant A passes a derived capability to tenant B, and
//! the question is whether A and B sharing a fusion domain lets either reach the
//! other's table. That is the configuration gale's syscall seam (gale#408)
//! depends on being unforgeable.
#![no_std]
#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }

use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "middle", path: "wit", generate_all });

struct M;

/// The re-exported resource. Each instance OWNS an imported handle and forwards
/// to it — so a handle held by a downstream tenant is backed by a handle this
/// component holds, which is exactly the two-level ownership gust's seam would
/// have between supervisor, tenant A and tenant B.
pub struct Task {
    inner: gale::capfix::caps::Task,
}

impl exports::gale::capfix::caps::GuestTask for Task {
    fn new(entry: u32) -> Self {
        // Derive rather than pass through, so the forwarding is observable in
        // the output: a value that came back unchanged would not distinguish a
        // real re-export from an elided one.
        Task { inner: gale::capfix::caps::Task::new(entry ^ 0x00A5_0000) }
    }
    fn tick(&self, ticks: u32) -> u32 {
        self.inner.tick(ticks)
    }
    fn peek(&self) -> u32 {
        self.inner.peek()
    }
}

impl exports::gale::capfix::caps::Guest for M {
    type Task = Task;
    fn now() -> u32 {
        gale::capfix::caps::now()
    }
}

impl Guest for M {
    /// The middle tenant using a handle of its OWN, so its table is populated.
    /// The drop at the end of this scope is the one whose fate across a domain
    /// boundary gale measured as a link obligation (`U [resource-drop]task`).
    fn mid_run() -> u32 {
        let t = gale::capfix::caps::Task::new(0x11);
        let a = t.tick(3);
        let b = t.peek();
        a ^ b
        // `t` drops here.
    }
}

export!(M);
