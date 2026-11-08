//! SPIKE — gust:switch thin-seam partition-switch FSM.
//! Bodies lifted VERBATIM from plain/src/partition_switch.rs (Verus+Kani verified).
//! The three trusted seams arrive as WIT-typed component imports.
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

wit_bindgen::generate!({ world: "switch-thin", path: "wit", generate_all });

mod sw {
    // the three seams: what were `unsafe extern "C"` are now typed imports
    pub unsafe fn ctx_save(p: u32) -> u32 { crate::gust::switch::ctx::ctx_save(p) }
    pub unsafe fn region_swap(p: u32) -> u32 { crate::gust::switch::ctx::region_swap(p) }
    pub unsafe fn ctx_resume(p: u32) -> u32 { crate::gust::switch::ctx::ctx_resume(p) }

    pub const MAX_WINDOWS: usize = 4;
    
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum SwPhase {
        /// A partition is executing inside its window.
        Running,
        /// Window boundary hit: the outgoing partition's context is being saved.
        SaveCtx,
        /// Memory-protection regions are being programmed for the incoming
        /// partition. Strictly after SaveCtx, strictly before Resume.
        ProgramRegions,
        /// The incoming partition is being resumed. Reachable ONLY through
        /// ProgramRegions (proven: `inv`'s `swapped` conjunct + `k2`).
        Resume,
    }
    
    #[derive(Clone, Copy)]
    pub struct MajorFrame {
        /// Which partition owns each window.
        pub partition_id: [u32; MAX_WINDOWS],
        /// Start tick of each window on the major-frame timeline.
        pub offset: [u32; MAX_WINDOWS],
        /// Length (in ticks) of each window. Always > 0 under `frame_inv`.
        pub budget: [u32; MAX_WINDOWS],
        /// Total major-frame length: the exclusive end of the last window.
        pub frame_len: u32,
    }
    
    pub struct Switcher {
        /// The static major frame (validated once via `MajorFrame::check`).
        pub frame: MajorFrame,
        /// Current window index, always < MAX_WINDOWS.
        pub cur: u32,
        /// Where the switch is in its preemption sequence.
        pub phase: SwPhase,
        /// True IFF the memory-protection regions for the incoming partition
        /// have been programmed during the in-flight switch. Cleared on entering
        /// SaveCtx, set only by `mark_swapped` (ProgramRegions → Resume).
        pub swapped: bool,
    }
    
    impl MajorFrame {
        /// Exec validator: true IFF `frame_inv` holds. The integration seam calls
        /// this once on the static frame table before constructing a `Switcher`
        /// — after that, every proof rides on the established invariant. All
        /// sums are computed in u64 so the check itself can never overflow, even
        /// on a hostile table.
        pub fn check(&self) -> bool {
            self.offset[0] == 0 && self.budget[0] > 0 && self.budget[1] > 0
                && self.budget[2] > 0 && self.budget[3] > 0
                && (self.offset[0] as u64) + (self.budget[0] as u64)
                    == (self.offset[1] as u64)
                && (self.offset[1] as u64) + (self.budget[1] as u64)
                    == (self.offset[2] as u64)
                && (self.offset[2] as u64) + (self.budget[2] as u64)
                    == (self.offset[3] as u64)
                && (self.offset[3] as u64) + (self.budget[3] as u64)
                    == (self.frame_len as u64)
        }
        /// The unique window containing tick `t`. Containment AND uniqueness are
        /// both ensured — for every `t < frame_len` there is exactly one window
        /// (coverage-without-overlap, the temporal-isolation core). Straight-line
        /// (no loop): with 4 windows the scan is three ordered comparisons
        /// against the window start offsets.
        pub fn current_window(&self, t: u32) -> u32 {
            let w: u32 = if t < self.offset[1] {
                0
            } else if t < self.offset[2] {
                1
            } else if t < self.offset[3] {
                2
            } else {
                3
            };
            w
        }
    }
    
    impl Switcher {
        /// Start of the major frame: window 0, Running.
        pub fn new(frame: MajorFrame) -> Switcher {
            Switcher {
                frame,
                cur: 0,
                phase: SwPhase::Running,
                swapped: false,
            }
        }
        /// S1 — NON-MASKABLE window-end preemption. One timer tick: from Running
        /// at the current window's end boundary, the FSM ALWAYS enters SaveCtx —
        /// the postcondition is an unconditional implication over EVERY state and
        /// EVERY input; there is no transition, flag, or argument that suppresses
        /// it, because none exists in the code (mirroring wdg-thin's
        /// cannot-un-start: no disable path is provided at all). Off-boundary
        /// ticks and non-Running phases are no-ops (a total function — the
        /// boundary test is in the body, not in a strippable precondition, so
        /// the shipped code is exactly as defensive as the verified code).
        pub fn tick(&mut self, t: u32) -> bool {
            if matches!(self.phase, SwPhase::Running) {
                let end = self.frame.offset[self.cur as usize]
                    + self.frame.budget[self.cur as usize];
                if t == end - 1 {
                    self.phase = SwPhase::SaveCtx;
                    self.swapped = false;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        /// One-way step: SaveCtx → ProgramRegions (the outgoing partition's
        /// context is now saved). No-op from any other phase.
        pub fn mark_saved(&mut self) -> bool {
            if matches!(self.phase, SwPhase::SaveCtx) {
                self.phase = SwPhase::ProgramRegions;
                true
            } else {
                false
            }
        }
        /// One-way step: ProgramRegions → Resume, setting the `swapped` ledger
        /// bit. This is the ONLY place `swapped` becomes true, which is why
        /// `inv`'s Resume conjunct proves S2 (region-swap-before-resume): no
        /// other edge can discharge it. No-op from any other phase.
        pub fn mark_swapped(&mut self) -> bool {
            if matches!(self.phase, SwPhase::ProgramRegions) {
                self.phase = SwPhase::Resume;
                self.swapped = true;
                true
            } else {
                false
            }
        }
        /// One-way step: Resume → Running, advancing the window index by exactly
        /// one (mod MAX_WINDOWS) — together with `lemma_no_skip`, the frame is
        /// followed with no window skipped or repeated (S3). No-op from any
        /// other phase.
        pub fn mark_resumed(&mut self) -> bool {
            if matches!(self.phase, SwPhase::Resume) {
                self.phase = SwPhase::Running;
                self.cur = if self.cur + 1 == MAX_WINDOWS as u32 { 0 } else { self.cur + 1 };
                true
            } else {
                false
            }
        }
        /// The trusted seam, wrapped to the minimum trusted surface (cf.
        /// executor.rs's `dispatch_one`): save the outgoing partition's context.
        /// `#[verifier::external_body]` — no ensures at all, so no proof ever
        /// leans on what the hardware did; the FSM's ordering guarantees rest
        /// exclusively on the verified `mark_*` steps around these calls.
        #[allow(unsafe_code)]
        fn seam_ctx_save(part: u32) -> u32 {
            unsafe { ctx_save(part) }
        }
        /// Trusted seam wrapper: program the incoming partition's regions. Wired
        /// to the isolation core's partition programmer at integration.
        #[allow(unsafe_code)]
        fn seam_region_swap(part: u32) -> u32 {
            unsafe { region_swap(part) }
        }
        /// Trusted seam wrapper: resume the incoming partition's context.
        #[allow(unsafe_code)]
        fn seam_ctx_resume(part: u32) -> u32 {
            unsafe { ctx_resume(part) }
        }
        /// Drive one full switch after a boundary preemption: SaveCtx →
        /// ProgramRegions → Resume → Running, crossing each trusted seam in
        /// order. The `swapped` invariant machine-checks the FSM ordering:
        /// `mark_swapped` strictly precedes `mark_resumed` (S2 at the FSM
        /// level). The binding of `seam_region_swap` to `mark_swapped` — that
        /// the seam call is actually issued on that edge — is trusted code
        /// order in this body, not machine-checked (see the trusted-seam note
        /// at the top of this file). Ends back in Running with the window
        /// index advanced by exactly one (mod MAX_WINDOWS).
        pub fn run_switch(&mut self) {
            let outgoing = self.frame.partition_id[self.cur as usize];
            let next = if self.cur + 1 == MAX_WINDOWS as u32 { 0 } else { self.cur + 1 };
            let incoming = self.frame.partition_id[next as usize];
            let _ = Self::seam_ctx_save(outgoing);
            let _ = self.mark_saved();
            let _ = Self::seam_region_swap(incoming);
            let _ = self.mark_swapped();
            let _ = Self::seam_ctx_resume(incoming);
            let _ = self.mark_resumed();
        }
    }}

use sw::{MajorFrame, SwPhase, Switcher, MAX_WINDOWS};

static mut SW: Option<Switcher> = None;
static mut PENDING: Option<MajorFrame> = None;
fn pending() -> &'static mut MajorFrame {
    unsafe {
        if PENDING.is_none() {
            PENDING = Some(MajorFrame { partition_id: [0; MAX_WINDOWS],
                                        offset: [0; MAX_WINDOWS],
                                        budget: [0; MAX_WINDOWS], frame_len: 0 });
        }
        PENDING.as_mut().unwrap()
    }
}
fn sw() -> &'static mut Switcher {
    unsafe {
        if SW.is_none() { SW = Some(Switcher::new(*pending())); }
        SW.as_mut().unwrap()
    }
}

struct P;
impl exports::gust::switch::fsm::Guest for P {
    fn set_window(idx: u32, pid: u32, offset: u32, budget: u32) -> bool {
        if idx as usize >= MAX_WINDOWS { return false; }
        let f = pending();
        f.partition_id[idx as usize] = pid;
        f.offset[idx as usize] = offset;
        f.budget[idx as usize] = budget;
        true
    }
    fn seal_frame(frame_len: u32) -> bool {
        pending().frame_len = frame_len;
        let ok = pending().check();
        if ok { unsafe { SW = Some(Switcher::new(*pending())); } }
        ok
    }
    fn frame_check() -> bool { sw().frame.check() }
    fn current_window(t: u32) -> u32 { sw().frame.current_window(t) }
    fn tick(t: u32) -> bool { sw().tick(t) }
    fn mark_saved() -> bool { sw().mark_saved() }
    fn mark_swapped() -> bool { sw().mark_swapped() }
    fn mark_resumed() -> bool { sw().mark_resumed() }
    fn run_switch() { sw().run_switch() }
}
export!(P);
