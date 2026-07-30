//! Execute the fused `gust:os` component on a host engine and OBSERVE the
//! one-scheduler / one-clock properties (REQ-OS-COMPOSITE-EXEC-001,
//! VER-OS-COMPOSITE-EXEC-001).
//!
//! ## Why this exists
//!
//! v0.6.0 shipped "exactly one scheduler and one clock" for the composite built by
//! `build-fused-gustos.sh`. Both claims were established STRUCTURALLY: by import
//! routing across the unbundled core modules (that script's gate 4) and by which
//! crate is allowed to `#[path]`-include `plain/src/executor.rs`
//! (`build-gustos-components.sh`). Nobody had ever CALLED the thing. A structural
//! gate can only see where an operation is routed; it cannot see whether the handle
//! `spawn.start` hands back names the same task to `timer.sleep` and `exec.state`,
//! because that is a runtime fact about one shared `Tasks` table.
//!
//! So this harness is a falsification attempt, not a demo. Every check below is
//! written to be capable of FAILING against a composite whose providers each kept a
//! private table or a private clock, and the observed values are printed
//! unconditionally — a check that only prints on failure is a check nobody has
//! watched run.
//!
//! ## What the host supplies
//!
//! The composite's world has exactly two residual imports, so the engine must
//! provide exactly two things:
//!   * `gust:hal/mmio` — a fake register file. Fidelity is irrelevant; what matters
//!     is that the host ANSWERS every read itself, so the value the composite's
//!     clock sees is host-chosen and every clock read is observable in a trace.
//!     Reads of TIM2_CNT return `Fake::clock`; `Fake::step` optionally advances it
//!     per read, which is what makes two clock reads distinguishable (see phase 4).
//!   * `gust:os/taskdisp.poll-task` — the app-side task body. It records the `id` it
//!     was dispatched with (the load-bearing observation for "one scheduler": the
//!     dispatched id must be the handle `spawn.start` returned) and reports "done"
//!     after a configured number of polls, so a task can actually complete.
//!
//! Scope: HOST-ENGINE execution only. This says nothing about the dissolved/native
//! path — a synth-lowered image is a separate rung of REQ-OS-COMPOSITE-EXEC-001.

use anyhow::{Context, Result};
use std::collections::HashMap;
use wasmtime::component::{
    Component, ComponentNamedList, Instance, Lift, Linker, Lower, TypedFunc,
};
use wasmtime::{Config, Engine, Store, StoreContextMut};

/// The register the time provider reads for `time.now()` (time-provider/src/lib.rs).
const TIM2_CNT: u32 = 0x4000_0024;
/// The register the log provider writes each byte to (log-provider/src/lib.rs).
const UART_DR: u32 = 0x4001_3804;
/// The all-ones sentinel every gust:os interface uses for "invalid handle".
const INVALID: u32 = 0xFFFF_FFFF;
/// `plain/src/executor.rs` — the ONE table's slot count. The joint-exhaustion check
/// in phase 5 is only meaningful against this number.
const MAX_TASKS: u32 = 8;

// ───────────────────────────── host-side fake world ─────────────────────────────

/// The two imports, plus the traces that make the composite's internal behaviour
/// observable from outside. Nothing here models real hardware; it models an ORACLE.
#[derive(Default)]
struct Fake {
    /// Value returned for the next read of TIM2_CNT.
    clock: u32,
    /// Added to `clock` AFTER each TIM2_CNT read. 0 = frozen clock (the default, so
    /// sweeps are repeatable); non-zero makes every read return a distinct value,
    /// which is how phase 4 tells one clock read from two.
    step: u32,
    /// (addr, value) of every read32 the composite performed.
    reads: Vec<(u32, u32)>,
    /// (addr, value) of every write32 the composite performed.
    writes: Vec<(u32, u32)>,
    /// The task ids `taskdisp.poll-task` was dispatched with, in order.
    polls: Vec<u32>,
    /// id -> number of polls after which the task reports completion.
    done_after: HashMap<u32, u32>,
}

impl Fake {
    fn read32(&mut self, addr: u32) -> u32 {
        let v = if addr == TIM2_CNT { self.clock } else { 0 };
        if addr == TIM2_CNT {
            self.clock = self.clock.wrapping_add(self.step);
        }
        self.reads.push((addr, v));
        v
    }
    fn write32(&mut self, addr: u32, val: u32) {
        self.writes.push((addr, val));
    }
    /// The trusted dispatch seam's contract: poll task `id` once, 1 = completed.
    fn poll_task(&mut self, id: u32) -> u32 {
        self.polls.push(id);
        let n = self.polls.iter().filter(|&&x| x == id).count() as u32;
        match self.done_after.get(&id) {
            Some(&k) if n >= k => 1,
            _ => 0,
        }
    }
    /// Reads performed since a mark — used to assert HOW MANY clock reads a single
    /// seam call performed, and with what answers.
    fn reads_since(&self, mark: usize) -> &[(u32, u32)] {
        &self.reads[mark..]
    }
}

// ─────────────────────────────── check bookkeeping ──────────────────────────────

/// Two buckets on purpose. `claim` failures refute REQ-OS-COMPOSITE-EXEC-001;
/// `contract` failures are deviations from the WIT's documented return contract that
/// do not bear on the one-scheduler/one-clock property. Conflating them would let a
/// docs-level bug masquerade as a refutation, or vice versa.
#[derive(Default)]
struct Report {
    claim_pass: u32,
    claim_fail: Vec<String>,
    contract_pass: u32,
    contract_fail: Vec<String>,
}

impl Report {
    fn claim(&mut self, id: &str, ok: bool, observed: String) {
        println!(
            "  [{}] {:<34} {}",
            if ok { "PASS" } else { "FAIL" },
            id,
            observed
        );
        if ok {
            self.claim_pass += 1;
        } else {
            self.claim_fail.push(format!("{id}: {observed}"));
        }
    }
    fn contract(&mut self, id: &str, ok: bool, observed: String) {
        println!(
            "  [{}] {:<34} {}",
            if ok { "pass" } else { "DEVIATION" },
            id,
            observed
        );
        if ok {
            self.contract_pass += 1;
        } else {
            self.contract_fail.push(format!("{id}: {observed}"));
        }
    }
    /// An observation with no pass/fail verdict — printed so the run is auditable.
    fn note(&self, text: String) {
        println!("  [note] {text}");
    }
}

// ────────────────────────────── the composite's exports ─────────────────────────

/// Every export of the composite, typed. Fields are `f_`-prefixed so the call
/// wrappers below can carry the interface's own names.
struct Os {
    f_now: TypedFunc<(), (u64,)>,
    f_resolution: TypedFunc<(), (u64,)>,
    f_deadline: TypedFunc<(u64, u64), (u64,)>,
    f_elapsed: TypedFunc<(u64, u64), (bool,)>,
    f_log_line: TypedFunc<(Vec<u8>,), ()>,
    f_spawn_start: TypedFunc<(u32,), (u32,)>,
    f_spawn_poll: TypedFunc<(u32,), (u32,)>,
    f_exec_admit: TypedFunc<(u32, u32, u32), (u32,)>,
    f_exec_poll_round: TypedFunc<(u32, u32), ()>,
    f_exec_state: TypedFunc<(u32,), (u32,)>,
    f_timer_sleep: TypedFunc<(u32, u32), (u32,)>,
    f_timer_slept: TypedFunc<(u32,), (u32,)>,
}

/// Look a function up by (interface, name) exactly as the composite exports it. A
/// missing name is a hard error, not a skipped check.
fn typed_fn<P, R>(
    store: &mut Store<Fake>,
    inst: &Instance,
    iface: &str,
    name: &str,
) -> Result<TypedFunc<P, R>>
where
    P: ComponentNamedList + Lower,
    R: ComponentNamedList + Lift,
{
    let i = inst
        .get_export_index(&mut *store, None, iface)
        .with_context(|| format!("composite exports no instance `{iface}`"))?;
    let f = inst
        .get_export_index(&mut *store, Some(&i), name)
        .with_context(|| format!("`{iface}` exports no func `{name}`"))?;
    // wasmtime has its own Error type (not a std::error::Error), so its results are
    // re-wrapped by hand rather than through anyhow's Context extension trait.
    inst.get_typed_func::<P, R>(&mut *store, &f)
        .map_err(|e| anyhow::anyhow!("`{iface}#{name}` has an unexpected signature: {e:?}"))
}

impl Os {
    fn link(store: &mut Store<Fake>, inst: &Instance) -> Result<Os> {
        const TIME: &str = "gust:os/time@0.1.0";
        const LOG: &str = "gust:os/log@0.1.0";
        const SPAWN: &str = "gust:os/spawn@0.1.0";
        const EXEC: &str = "gust:os/exec@0.1.0";
        const TIMER: &str = "gust:os/timer@0.1.0";
        Ok(Os {
            f_now: typed_fn(store, inst, TIME, "now")?,
            f_resolution: typed_fn(store, inst, TIME, "resolution")?,
            f_deadline: typed_fn(store, inst, TIME, "deadline")?,
            f_elapsed: typed_fn(store, inst, TIME, "elapsed")?,
            f_log_line: typed_fn(store, inst, LOG, "line")?,
            f_spawn_start: typed_fn(store, inst, SPAWN, "start")?,
            f_spawn_poll: typed_fn(store, inst, SPAWN, "poll")?,
            f_exec_admit: typed_fn(store, inst, EXEC, "admit")?,
            f_exec_poll_round: typed_fn(store, inst, EXEC, "poll-round")?,
            f_exec_state: typed_fn(store, inst, EXEC, "state")?,
            f_timer_sleep: typed_fn(store, inst, TIMER, "sleep")?,
            f_timer_slept: typed_fn(store, inst, TIMER, "slept")?,
        })
    }

    fn now(&self, s: &mut Store<Fake>) -> Result<u64> {
        let (v,) = self.f_now.call(&mut *s, ())?;
        Ok(v)
    }
    fn resolution(&self, s: &mut Store<Fake>) -> Result<u64> {
        let (v,) = self.f_resolution.call(&mut *s, ())?;
        Ok(v)
    }
    fn deadline(&self, s: &mut Store<Fake>, now: u64, ticks: u64) -> Result<u64> {
        let (v,) = self.f_deadline.call(&mut *s, (now, ticks))?;
        Ok(v)
    }
    fn elapsed(&self, s: &mut Store<Fake>, now: u64, deadline: u64) -> Result<bool> {
        let (v,) = self.f_elapsed.call(&mut *s, (now, deadline))?;
        Ok(v)
    }
    fn log_line(&self, s: &mut Store<Fake>, msg: &[u8]) -> Result<()> {
        self.f_log_line.call(&mut *s, (msg.to_vec(),))?;
        Ok(())
    }
    fn spawn_start(&self, s: &mut Store<Fake>, entry: u32) -> Result<u32> {
        let (v,) = self.f_spawn_start.call(&mut *s, (entry,))?;
        Ok(v)
    }
    fn spawn_poll(&self, s: &mut Store<Fake>, h: u32) -> Result<u32> {
        let (v,) = self.f_spawn_poll.call(&mut *s, (h,))?;
        Ok(v)
    }
    fn exec_admit(&self, s: &mut Store<Fake>, prio: u32, deadline: u64) -> Result<u32> {
        let (v,) = self
            .f_exec_admit
            .call(&mut *s, (prio, deadline as u32, (deadline >> 32) as u32))?;
        Ok(v)
    }
    fn exec_poll_round(&self, s: &mut Store<Fake>, now: u64) -> Result<()> {
        self.f_exec_poll_round
            .call(&mut *s, (now as u32, (now >> 32) as u32))?;
        Ok(())
    }
    fn exec_state(&self, s: &mut Store<Fake>, h: u32) -> Result<u32> {
        let (v,) = self.f_exec_state.call(&mut *s, (h,))?;
        Ok(v)
    }
    fn timer_sleep(&self, s: &mut Store<Fake>, h: u32, ticks: u32) -> Result<u32> {
        let (v,) = self.f_timer_sleep.call(&mut *s, (h, ticks))?;
        Ok(v)
    }
    fn timer_slept(&self, s: &mut Store<Fake>, h: u32) -> Result<u32> {
        let (v,) = self.f_timer_slept.call(&mut *s, (h,))?;
        Ok(v)
    }
}

/// Human-readable `exec.state` / `gust:sched.state` encoding.
fn state_name(v: u32) -> &'static str {
    match v {
        0 => "free",
        1 => "pending",
        2 => "done",
        INVALID => "INVALID",
        _ => "?",
    }
}

/// The exact instant at which `timer.slept(h)` flips 0 -> 1, found by moving the
/// FAKE CLOCK under a pure query (`slept_status` does not mutate the table), so the
/// probe cannot perturb what it measures. Requires p(lo)=0 and p(hi)=1.
fn deadline_boundary(
    os: &Os,
    s: &mut Store<Fake>,
    h: u32,
    lo: u32,
    hi: u32,
) -> Result<Option<u32>> {
    let saved = (s.data().clock, s.data().step);
    s.data_mut().step = 0; // freeze: the sweep must not advance the clock itself
    let probe = |s: &mut Store<Fake>, c: u32| -> Result<u32> {
        s.data_mut().clock = c;
        os.timer_slept(s, h)
    };
    let (a, b) = (probe(s, lo)?, probe(s, hi)?);
    let out = if a != 0 || b != 1 {
        None // not a clean 0->1 step over [lo,hi]; caller reports the raw endpoints
    } else {
        let (mut l, mut r) = (lo, hi);
        while r - l > 1 {
            let m = l + (r - l) / 2;
            if probe(s, m)? == 1 {
                r = m;
            } else {
                l = m;
            }
        }
        Some(r)
    };
    s.data_mut().clock = saved.0;
    s.data_mut().step = saved.1;
    Ok(out)
}

// ───────────────────────────────────── main ─────────────────────────────────────

fn main() -> Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../gustos-components/fused-gustos.component.wasm"
        )
        .to_string()
    });
    println!("gustos-hostrun — executing the fused gust:os component on wasmtime");
    println!("  composite : {path}");
    let bytes = std::fs::read(&path).with_context(|| format!("cannot read {path}"))?;
    println!("  size      : {} B", bytes.len());
    println!("  engine    : wasmtime {}\n", env!("WASMTIME_VERSION"));

    // ── phase 0: instantiate ────────────────────────────────────────────────────
    // "The composite cannot be instantiated" is itself a result, so this phase
    // reports its error verbatim rather than unwrapping.
    println!("== phase 0: instantiate ==");
    let engine = Engine::new(Config::new().wasm_component_model(true))?;
    let component = Component::from_binary(&engine, &bytes)
        .map_err(|e| anyhow::anyhow!("Component::from_binary failed: {e:?}"))?;
    let mut store = Store::new(&engine, Fake::default());
    let mut linker: Linker<Fake> = Linker::new(&engine);

    // gust:hal/mmio — only read32/write32 are in the composite's residual import
    // (its world lists exactly those two), so only those two are supplied. If the
    // composite ever grows a read8/write8 use, instantiation FAILS here rather than
    // silently trapping later, which is the behaviour we want from a gate.
    let mut mmio = linker.instance("gust:hal/mmio@0.1.0")?;
    mmio.func_wrap(
        "read32",
        |mut c: StoreContextMut<'_, Fake>, (addr,): (u32,)| Ok((c.data_mut().read32(addr),)),
    )?;
    mmio.func_wrap(
        "write32",
        |mut c: StoreContextMut<'_, Fake>, (addr, val): (u32, u32)| {
            c.data_mut().write32(addr, val);
            Ok(())
        },
    )?;
    let mut td = linker.instance("gust:os/taskdisp@0.1.0")?;
    td.func_wrap(
        "poll-task",
        |mut c: StoreContextMut<'_, Fake>, (id,): (u32,)| Ok((c.data_mut().poll_task(id),)),
    )?;

    let instance = match linker.instantiate(&mut store, &component) {
        Ok(i) => {
            println!("  ok: instantiated (both residual imports resolved by the host)");
            i
        }
        Err(e) => {
            println!("  COULD NOT INSTANTIATE: {e:?}");
            std::process::exit(2);
        }
    };
    let os = Os::link(&mut store, &instance).context("linking the composite's exports")?;
    println!("  ok: all 12 functions across the 5 exported gust:os interfaces resolved\n");

    let mut r = Report::default();

    // ── phase 1: the composite runs at all ──────────────────────────────────────
    println!("== phase 1: the composite executes, and the host answers its clock ==");
    store.data_mut().clock = 1000;
    store.data_mut().step = 0;
    let n0 = os.now(&mut store)?;
    r.claim(
        "time.now reads the host clock",
        n0 == 1000,
        format!("time.now() = {n0} (host clock = 1000)"),
    );
    let res = os.resolution(&mut store)?;
    r.note(format!("time.resolution() = {res} Hz"));
    let dl = os.deadline(&mut store, 1000, 500)?;
    let el_before = os.elapsed(&mut store, 1499, dl)?;
    let el_at = os.elapsed(&mut store, 1500, dl)?;
    r.note(format!(
        "time.deadline(1000,500) = {dl}; time.elapsed(1499,{dl}) = {el_before}, elapsed(1500,{dl}) = {el_at}"
    ));
    let wmark = store.data().writes.len();
    os.log_line(&mut store, b"gustos-hostrun")?;
    let w: Vec<u32> = store.data().writes[wmark..]
        .iter()
        .filter(|(a, _)| *a == UART_DR)
        .map(|(_, v)| *v)
        .collect();
    let want: Vec<u32> = b"gustos-hostrun".iter().map(|&b| b as u32).collect();
    r.claim(
        "log.line moves real bytes",
        w == want,
        format!(
            "{} write32 to UART_DR = {:?}",
            w.len(),
            String::from_utf8_lossy(&w.iter().map(|&v| v as u8).collect::<Vec<_>>())
        ),
    );
    println!();

    // ── phase 2: one handle, three interfaces ───────────────────────────────────
    // The core of REQ-OS-COMPOSITE-EXEC-001: a handle minted by spawn must be
    // ACCEPTED and understood by exec and timer.
    println!("== phase 2: the handle spawn.start returns, seen by exec and timer ==");
    let h = os.spawn_start(&mut store, 0xA5)?;
    r.claim(
        "2a. spawn.start returns a handle",
        h != INVALID,
        format!("spawn.start(0xA5) = {h} (invalid sentinel = 0x{INVALID:08X})"),
    );
    let st = os.exec_state(&mut store, h)?;
    r.claim(
        "2b. exec.state recognises it",
        st != INVALID && st == 1,
        format!("exec.state({h}) = {st} ({})", state_name(st)),
    );
    let sl0 = os.timer_slept(&mut store, h)?;
    r.claim(
        "2c. timer.slept accepts it",
        sl0 != INVALID,
        format!("timer.slept({h}) = {sl0} (0 = pending)"),
    );
    let rc = os.timer_sleep(&mut store, h, 500)?;
    r.claim(
        "2d. timer.sleep accepts the same h",
        rc != INVALID && rc == 0,
        format!("timer.sleep({h}, 500) = {rc} (0 = success)"),
    );
    let st = os.exec_state(&mut store, h)?;
    r.claim(
        "2e. exec still agrees after arming",
        st == 1,
        format!("exec.state({h}) = {st} ({})", state_name(st)),
    );
    println!();

    // ── phase 3: one lifecycle, observed through all three ──────────────────────
    // The dispatch id is the load-bearing observation. A second task table would
    // have handed `poll-task` an index into ITS numbering, not spawn's handle.
    println!("== phase 3: drive the lifecycle; the three interfaces must agree ==");
    store.data_mut().done_after.insert(h, 3); // completes on its 3rd poll
    let p0 = store.data().polls.len();
    os.exec_poll_round(&mut store, 1000)?;
    let d1 = store.data().polls[p0..].to_vec();
    r.claim(
        "3a. dispatched id == spawn's handle",
        d1 == vec![h],
        format!("poll-round(1000) dispatched poll-task{d1:?}, spawn.start returned {h}"),
    );
    let st = os.exec_state(&mut store, h)?;
    r.claim(
        "3b. still pending after 1 poll",
        st == 1,
        format!("exec.state({h}) = {st} ({})", state_name(st)),
    );

    let p1 = store.data().polls.len();
    os.exec_poll_round(&mut store, 1400)?;
    let d2 = store.data().polls[p1..].to_vec();
    store.data_mut().clock = 1400;
    let sl = os.timer_slept(&mut store, h)?;
    r.claim(
        "3c. before the deadline: no dispatch",
        d2.is_empty() && sl == 0,
        format!("poll-round(1400) dispatched {d2:?}; timer.slept({h}) @1400 = {sl}"),
    );

    let p2 = store.data().polls.len();
    os.exec_poll_round(&mut store, 1500)?;
    let d3 = store.data().polls[p2..].to_vec();
    store.data_mut().clock = 1500;
    let sl = os.timer_slept(&mut store, h)?;
    let st = os.exec_state(&mut store, h)?;
    r.claim(
        "3d. at the deadline: timer armed SPAWN's task",
        d3 == vec![h] && sl == 1 && st == 1,
        format!(
            "poll-round(1500) dispatched poll-task{d3:?}; timer.slept({h}) = {sl} (1 = elapsed); exec.state({h}) = {st} ({})",
            state_name(st)
        ),
    );

    let p3 = store.data().polls.len();
    os.exec_poll_round(&mut store, 1600)?;
    let d4 = store.data().polls[p3..].to_vec();
    let st = os.exec_state(&mut store, h)?;
    let sp = os.spawn_poll(&mut store, h)?;
    let sl = os.timer_slept(&mut store, h)?;
    r.claim(
        "3e. completion agreed by all three",
        d4 == vec![h] && st == 2 && sp == 1 && sl == 1,
        format!(
            "3rd poll{d4:?} returned done -> exec.state({h}) = {st} ({}), spawn.poll({h}) = {sp} (1 = done), timer.slept({h}) = {sl}",
            state_name(st)
        ),
    );
    r.note(format!(
        "full dispatch trace for handle {h}: {:?}",
        store.data().polls
    ));
    println!();

    // ── phase 4: one clock ──────────────────────────────────────────────────────
    // Vacuous version of this check: "time.now() moved, so the clock is shared".
    // That passes even with two independent clocks reading the same register. What
    // actually distinguishes them: make every read return a DIFFERENT value
    // (step != 0), then show (a) the arming call performed exactly ONE clock read,
    // and (b) the deadline it installed equals THAT read's answer + ticks — not the
    // next read's answer, and not any value the host never handed out.
    println!("== phase 4: one clock (the timer's deadline derives from time's read) ==");
    let h2 = os.spawn_start(&mut store, 0xB0)?;
    r.claim(
        "4a. second handle from the same space",
        h2 != INVALID && h2 == h + 1,
        format!("spawn.start(0xB0) = {h2} (previous handle was {h})"),
    );

    store.data_mut().clock = 4000;
    store.data_mut().step = 7; // every read now answers a distinct value
    let rmark = store.data().reads.len();
    let rc = os.timer_sleep(&mut store, h2, 500)?;
    let reads: Vec<(u32, u32)> = store.data().reads_since(rmark).to_vec();
    let one_read = reads.len() == 1 && reads[0].0 == TIM2_CNT;
    r.claim(
        "4b. arming does exactly one clock read",
        one_read && rc == 0,
        format!(
            "timer.sleep({h2},500) = {rc}; reads during the call = {:?} (addr,value)",
            reads
                .iter()
                .map(|(a, v)| (format!("0x{a:08X}"), *v))
                .collect::<Vec<_>>()
        ),
    );
    let served = reads.first().map(|(_, v)| *v).unwrap_or(0);
    let b1 = deadline_boundary(&os, &mut store, h2, 4000, 5200)?;
    r.claim(
        "4c. deadline == THAT read + ticks",
        b1 == Some(served + 500),
        format!(
            "timer.slept flips 0->1 at clock {:?}; the host served {served} to the one read, ticks = 500 -> expected {}; next read would have served {}",
            b1,
            served + 500,
            served + 7
        ),
    );

    // The jump. A clock the composite does not share cannot follow this.
    let jump_to = 900_000u32;
    store.data_mut().clock = jump_to;
    store.data_mut().step = 0;
    let n = os.now(&mut store)?;
    store.data_mut().clock = jump_to;
    let sl = os.timer_slept(&mut store, h2)?;
    r.claim(
        "4d. both views observe the same jump",
        n == jump_to as u64 && sl == 1,
        format!(
            "after jumping the register {} -> {jump_to}: time.now() = {n}; timer.slept({h2}) = {sl} (1 = the pre-jump deadline is now past)",
            served + 7
        ),
    );

    store.data_mut().clock = jump_to;
    store.data_mut().step = 7;
    let rmark = store.data().reads.len();
    let rc = os.timer_sleep(&mut store, h2, 1000)?;
    let reads: Vec<(u32, u32)> = store.data().reads_since(rmark).to_vec();
    let served2 = reads.first().map(|(_, v)| *v).unwrap_or(0);
    let b2 = deadline_boundary(&os, &mut store, h2, jump_to, jump_to + 2000)?;
    r.claim(
        "4e. re-arming tracks the jumped clock",
        rc == 0 && reads.len() == 1 && b2 == Some(served2 + 1000),
        format!(
            "re-armed at the jumped instant: one read served {served2}, ticks = 1000 -> timer.slept flips at {:?} (expected {})",
            b2,
            served2 + 1000
        ),
    );
    let foreign: Vec<String> = store
        .data()
        .reads
        .iter()
        .filter(|(a, _)| *a != TIM2_CNT)
        .map(|(a, _)| format!("0x{a:08X}"))
        .collect();
    r.claim(
        "4f. no second register is ever read",
        foreign.is_empty(),
        format!(
            "{} read32 in the whole run, all at 0x{TIM2_CNT:08X}; foreign addresses = {foreign:?}",
            store.data().reads.len()
        ),
    );
    r.note(
        "NOT covered by phase 4: exec.poll-round's `now` is a CALLER argument — the composite \
         has no internal clock driving expiry, so the app is that time source by design."
            .to_string(),
    );
    println!();

    // ── phase 5: one TABLE, not merely one route ────────────────────────────────
    // Two 8-slot tables would let spawn and exec admit 8 tasks EACH. One table is
    // exhausted jointly. This is the check a per-provider private table fails
    // loudest, and it is independent of the handle-agreement checks above.
    println!("== phase 5: spawn and exec exhaust ONE {MAX_TASKS}-slot table ==");
    store.data_mut().step = 0;
    let mut handles = vec![h, h2];
    let mut alloc_log = format!("spawn={h}, spawn={h2}");
    let mut full_from: Option<&str> = None;
    for i in 0..(MAX_TASKS + 2) {
        let (via, hn) = if i % 2 == 0 {
            ("exec.admit", os.exec_admit(&mut store, 2, u64::MAX)?)
        } else {
            ("spawn.start", os.spawn_start(&mut store, 0xC0 + i)?)
        };
        alloc_log.push_str(&format!(", {}={}", via.split('.').next().unwrap(), hn));
        if hn == INVALID {
            if full_from.is_none() {
                full_from = Some(via);
            }
        } else {
            handles.push(hn);
        }
    }
    let distinct: std::collections::BTreeSet<u32> = handles.iter().copied().collect();
    r.claim(
        "5a. handles are one shared, distinct space",
        distinct.len() == handles.len() && handles.len() == MAX_TASKS as usize,
        format!(
            "{} handles admitted across BOTH interfaces, {} distinct: {alloc_log}",
            handles.len(),
            distinct.len()
        ),
    );
    r.claim(
        "5b. the table is exhausted JOINTLY",
        full_from.is_some() && handles.len() == MAX_TASKS as usize,
        format!(
            "first full-table sentinel came from {:?} after {} total admits (two private tables would allow {} each)",
            full_from,
            handles.len(),
            MAX_TASKS
        ),
    );
    let states: Vec<String> = handles
        .iter()
        .map(|&x| {
            let s = os.exec_state(&mut store, x).unwrap_or(INVALID);
            format!("{x}:{}", state_name(s))
        })
        .collect();
    r.claim(
        "5c. exec knows every handle either minted",
        !states
            .iter()
            .any(|s| s.contains("INVALID") || s.contains("free")),
        format!("exec.state over every handle: {}", states.join(" ")),
    );
    println!();

    // ── phase 7: WIT return-contract observations ───────────────────────────────
    // Separate bucket: these check the documented return values in wit-os/gust-os.wit,
    // not the one-scheduler/one-clock property. A deviation here is a real defect but
    // does NOT refute REQ-OS-COMPOSITE-EXEC-001, and must not be reported as if it did.
    println!("== phase 6: WIT return-contract observations (separate from the claim) ==");
    for bad in [MAX_TASKS, 0xDEAD_BEEF] {
        let v = os.exec_state(&mut store, bad)?;
        r.contract(
            "exec.state rejects a bad handle",
            v == INVALID,
            format!("exec.state({bad}) = 0x{v:08X}"),
        );
        let v = os.spawn_poll(&mut store, bad)?;
        r.contract(
            "spawn.poll rejects a bad handle",
            v == INVALID,
            format!("spawn.poll({bad}) = 0x{v:08X}"),
        );
        let v = os.timer_slept(&mut store, bad)?;
        r.contract(
            "timer.slept rejects a bad handle",
            v == INVALID,
            format!("timer.slept({bad}) = 0x{v:08X}"),
        );
        // WIT: "Returns 0 on success, or 0xFFFF_FFFF if `handle` is invalid / not
        // Pending or `ticks` is out of range."
        let v = os.timer_sleep(&mut store, bad, 10)?;
        r.contract(
            "timer.sleep rejects a bad handle",
            v == INVALID,
            format!("timer.sleep({bad}, 10) = 0x{v:08X}"),
        );
    }
    let v = os.timer_sleep(&mut store, h, 10)?; // h is Done by now
    r.contract(
        "timer.sleep rejects a non-Pending handle",
        v == INVALID,
        format!(
            "timer.sleep({h}, 10) = 0x{v:08X} with exec.state({h}) = {}",
            state_name(os.exec_state(&mut store, h)?)
        ),
    );
    let v = os.timer_sleep(&mut store, h2, 1 << 31)?;
    r.contract(
        "timer.sleep rejects ticks >= 2^31",
        v == INVALID,
        format!("timer.sleep({h2}, 0x80000000) = 0x{v:08X}"),
    );
    println!();

    // ── verdict ─────────────────────────────────────────────────────────────────
    println!("== verdict ==");
    println!(
        "  REQ-OS-COMPOSITE-EXEC-001 checks : {} passed, {} FAILED",
        r.claim_pass,
        r.claim_fail.len()
    );
    for f in &r.claim_fail {
        println!("    FAILED: {f}");
    }
    println!(
        "  WIT return-contract checks       : {} passed, {} deviating",
        r.contract_pass,
        r.contract_fail.len()
    );
    for f in &r.contract_fail {
        println!("    DEVIATION: {f}");
    }
    if !r.claim_fail.is_empty() {
        println!("\nRESULT: the executed composite REFUTES a shipped claim (see FAILED above).");
        std::process::exit(1);
    }
    if !r.contract_fail.is_empty() {
        println!(
            "\nRESULT: one-scheduler and one-clock HELD under execution; \
             {} WIT return-contract deviation(s) observed (not a refutation of \
             REQ-OS-COMPOSITE-EXEC-001).",
            r.contract_fail.len()
        );
        std::process::exit(3);
    }
    println!("\nRESULT: one-scheduler and one-clock HELD under execution; no deviations.");
    Ok(())
}
