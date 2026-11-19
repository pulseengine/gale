//! `xtask silicon` — build gust firmware for a physical board, flash it under a bench
//! claim on the host that owns the probe, and take the verdict from the FIRMWARE.
//!
//! Why this is not a shell script: the scripts it replaces each reported success they
//! did not have (gale#397). One exited 0 without flashing (bash 3.2, failed `.` under
//! an EXIT trap). One took its exit from a grep that matched openocd's own
//! "Verified OK". One attached to the wrong board under a claim, because
//! `stlink-hla.cfg` matches every ST-LINK on a four-probe host. The verdict here is
//! three-valued, as for every xtask gate: PASS only on the firmware's `<bin> OK` line,
//! FAIL on its `<bin> FAIL` line, and REFUSED (exit 3) when neither appears — a run
//! that says nothing is not a run that passed. That includes a run that exits 0 with no
//! OK line: the status passes through with-device, a deadline wrapper and openocd, and
//! this runner cannot tell the firmware's semihosting SYS_EXIT 0 from their 0
//! (see [`firmware_verdict`]).
//!
//! Exit codes (also in `xtask silicon --help`):
//!
//! | code | `run`                                   | `matrix`                                        |
//! |------|-----------------------------------------|-------------------------------------------------|
//! | 0    | the firmware's OK line                  | ≥1 row, and every row PASS                      |
//! | 1    | the firmware's FAIL line                | any row FAIL                                    |
//! | 2    | usage                                   | usage, or an unknown `--boards`/`--bins` name   |
//! | 3    | could not run / no verdict (REFUSED)    | any row REFUSED (and none FAIL)                 |
//! | 4    | —                                       | any row NOT-YET (and none FAIL or REFUSED)      |
//! | 5    | —                                       | the filters selected zero rows                  |
//!
//! Only 0 is a pass. A NOT-YET row is a known gap, listed with its reason; it was a
//! silent 0 before, so `matrix --boards esp32c3` read as green.
//!
//! The board table holds only PUBLIC board metadata: the part, its target triple, the
//! openocd interface/target scripts, the claim name, and HOW its probe is pinned. Where a
//! board is plugged in — the probe host, and the USB port path that pins it there — is
//! private bench topology and is read at run time from the environment (see
//! [`resolve`] and os/gust/silicon/README.md). A board with no configured host is
//! REFUSED with the variable to set, never run against a default. Memory maps and
//! register addresses are NOT here either: they come from the measured target models
//! (targets/*.aadl -> generated/).

use crate::verdict::{Verdict, EXIT_FAIL, EXIT_PASS, EXIT_REFUSED, EXIT_USAGE};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Where the probe is plugged in. Resolved from the environment, never from the table.
#[derive(Clone, Debug, PartialEq)]
enum Host {
    /// This machine (`GALE_SILICON_<BOARD>_HOST=local`).
    Local,
    /// Over ssh, to this destination. The claim is taken THERE: a lock on this laptop
    /// would exclude nobody who ssh'd into the probe host from somewhere else.
    Ssh(String),
}

/// How the board's probe is told apart from every other probe on its host.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pin {
    /// The interface args select a VID:PID no other probe on the host has (public, in
    /// the table).
    VidPid,
    /// `adapter usb location <path>`. The port path is host topology, so it comes from
    /// `GALE_SILICON_<BOARD>_USB_LOCATION`; a board pinned this way REFUSES without it,
    /// because an unpinned `interface/stlink.cfg` attached to the wrong board while
    /// holding another board's claim — twice (gale#397).
    UsbLocation,
    /// `adapter serial <s>` from the bench registry, for the claim name. Only applied when
    /// the host is local; over ssh this board is currently NOT pinned (see run()).
    RegistrySerial,
}

/// How to get the core halted at a clean start.
#[derive(Clone, Copy)]
enum Reset {
    /// `reset halt`. Required on the WL55, whose resident firmware drops the debug link
    /// and which only attaches with SRST asserted.
    ResetHalt,
    /// `halt` to attach (the ST-LINK/V1 has no reset line; with `reset_config none` the
    /// later `reset halt` is a SYSRESETREQ).
    HaltSysreset,
}

struct Board {
    name: &'static str,
    what: &'static str,
    /// Registry name on the probe host. Must match it exactly (with-device refuses others).
    claim: &'static str,
    triple: &'static str,
    features: &'static [&'static str],
    /// Stem of targets/generated/{memory-,gust_target_}<stem>.{x,rs}.
    stem: &'static str,
    /// openocd interface args (adapter selection). The pin, if any, goes right after.
    iface: &'static [&'static str],
    /// openocd target args.
    target: &'static [&'static str],
    pin: Pin,
    reset: Reset,
    /// Some(reason) when nothing can run on this board yet.
    not_yet: Option<&'static str>,
}

// Boards have moved between probe hosts more than once, which is why the host is
// configured PER BOARD (alongside its claim) rather than as one bench-wide default:
// a shared default would put a board on a host it is no longer attached to.
const BOARDS: &[Board] = &[
    Board {
        name: "f100",
        what: "STM32VLDISCOVERY — STM32F100, Cortex-M3, no MPU",
        claim: "stlink-v1",
        triple: "thumbv7m-none-eabi",
        features: &["target-f100"],
        stem: "stm32f100",
        iface: &["-f", "interface/stlink-hla.cfg", "-c", "hla_vid_pid 0x0483 0x3744"],
        target: &["-f", "target/stm32f1x.cfg", "-c", "reset_config none separate"],
        pin: Pin::VidPid,
        reset: Reset::HaltSysreset,
        not_yet: None,
    },
    Board {
        name: "wl55jc",
        what: "NUCLEO-WL55JC1 — STM32WL55, Cortex-M4 (no FPU), MPU 8",
        claim: "stlink-v3",
        triple: "thumbv7em-none-eabi",
        features: &["target-wl55jc"],
        stem: "stm32wl55",
        iface: &["-f", "interface/stlink.cfg"],
        target: &["-f", "target/stm32wlx.cfg",
                  "-c", "reset_config srst_only srst_nogate connect_assert_srst"],
        pin: Pin::UsbLocation,
        reset: Reset::ResetHalt,
        not_yet: None,
    },
    Board {
        name: "wb55rg",
        what: "NUCLEO-WB55RG — STM32WB55, Cortex-M4F, MPU 8",
        claim: "nucleo-wb55rg",
        triple: "thumbv7em-none-eabi",
        features: &["target-wb55rg"],
        stem: "stm32wb55",
        iface: &["-f", "interface/stlink.cfg"],
        target: &["-f", "target/stm32wbx.cfg"],
        pin: Pin::UsbLocation,
        reset: Reset::ResetHalt,
        not_yet: None,
    },
    Board {
        name: "g031k8",
        what: "NUCLEO-G031K8 — STM32G031, Cortex-M0+ (ARMv6-M), MPU 8",
        claim: "nucleo-g031k8",
        triple: "thumbv6m-none-eabi",
        features: &["target-g031k8"],
        stem: "stm32g031",
        iface: &["-f", "interface/stlink.cfg"],
        target: &["-f", "target/stm32g0x.cfg"],
        pin: Pin::UsbLocation,
        reset: Reset::ResetHalt,
        not_yet: None,
    },
    Board {
        name: "g474re",
        what: "NUCLEO-G474RE — STM32G474, Cortex-M4F, MPU 8",
        // When this board last moved hosts, the move was verified by the probe's SERIAL
        // (the same serial enumerating on the new host and gone from the old one) rather
        // than inferred, and confirmed end to end through the new host — CPUID 0x410fc241
        // (Cortex-M4 r0p1), MPU_TYPE 0x00000800 (DREGION=8) read under a with-device claim.
        claim: "stlink-v3",
        triple: "thumbv7em-none-eabi",
        // target-g474re selects the generated constants; silicon-g474 is the older
        // per-bin map switch the iso probes still read on this board.
        features: &["target-g474re", "silicon-g474"],
        stem: "stm32g474",
        iface: &["-f", "interface/stlink.cfg"],
        target: &["-f", "target/stm32g4x.cfg"],
        // The probe is selected by the serial in THIS host's registry (see run()).
        pin: Pin::RegistrySerial,
        reset: Reset::ResetHalt,
        not_yet: None,
    },
    Board {
        name: "esp32c3",
        what: "ESP32-C3 — RV32IMC, built-in USB-JTAG",
        claim: "esp-jtag",
        triple: "riscv32imc-unknown-none-elf",
        features: &[],
        stem: "",
        iface: &[],
        target: &[],
        pin: Pin::VidPid,
        reset: Reset::ResetHalt,
        not_yet: Some("gust has no RISC-V silicon target model or runtime yet (REQ-OS-TARGET-RV32-001)"),
    },
];

/// Where one board is plugged in, as configured on this machine.
#[derive(Debug, PartialEq)]
struct Site {
    host: Host,
    usb_location: Option<String>,
}

/// `GALE_SILICON_<BOARD>_<WHAT>`, e.g. `GALE_SILICON_WL55JC_HOST`.
fn env_key(board: &str, what: &str) -> String {
    format!("GALE_SILICON_{}_{what}", board.to_ascii_uppercase())
}

/// Resolve a board's site from `get` (the environment, in production; a fixture in
/// tests). An unset or empty variable is an error naming it — there is no default host.
fn resolve(b: &Board, get: impl Fn(&str) -> Option<String>) -> Result<Site, String> {
    let get = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let hk = env_key(b.name, "HOST");
    let host = match get(&hk).as_deref() {
        None => return Err(format!(
            "{}: {hk} is not set — set it to `local` or to the ssh destination of the host \
             this board's probe is plugged into (os/gust/silicon/README.md)", b.name)),
        Some("local") => Host::Local,
        Some(h) => Host::Ssh(h.to_string()),
    };
    let usb_location = match b.pin {
        Pin::UsbLocation => {
            let lk = env_key(b.name, "USB_LOCATION");
            match get(&lk) {
                Some(l) => Some(l),
                None => return Err(format!(
                    "{}: {lk} is not set — this board's probe shares its VID:PID with others, so \
                     it must be pinned by its USB port path on the probe host (`adapter usb \
                     location` form, e.g. <bus>-<port>.<port>)", b.name)),
            }
        }
        Pin::VidPid | Pin::RegistrySerial => None,
    };
    Ok(Site { host, usb_location })
}

/// What runs where. A pair that cannot run is listed WITH its reason rather than left
/// out, so the matrix shows the gap instead of hiding it.
struct Entry {
    bin: &'static str,
    extra: &'static [&'static str],
    /// An input cargo does not build (an object from a pinned toolchain run), named by the
    /// environment variable that carries it. Unset -> REFUSED with how to produce it, not a
    /// link error reported as FAIL.
    needs_env: Option<(&'static str, &'static str)>,
    boards: &'static [&'static str],
    not_on: &'static [(&'static str, &'static str)],
}

const MATRIX: &[Entry] = &[
    Entry {
        bin: "gust_wdg_silicon",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "dissolved wdg-thin is ARMv7-M; synth has no ARMv6-M target (synth#1301)")],
    },
    Entry {
        bin: "gust_adc_silicon",
        extra: &[],
        needs_env: None,
        boards: &["f100"],
        not_on: &[],
    },
    Entry {
        bin: "gust_iso_unpriv_probe",
        extra: &[],
        needs_env: None,
        boards: &["wl55jc", "wb55rg", "g031k8", "g474re"],
        not_on: &[("f100", "the part has no MPU (MPU_TYPE reads 0) — by hardware")],
    },
    Entry {
        bin: "gust_iso_unpriv_probe",
        extra: &["drop-priv"],
        needs_env: None,
        boards: &["wl55jc", "wb55rg", "g031k8", "g474re"],
        not_on: &[("f100", "the part has no MPU (MPU_TYPE reads 0) — by hardware")],
    },
    // gale#410's privilege axis, as a MATCHED PAIR. Both arms run on every
    // MPU-bearing board; the private arm alone would prove the platform, not the
    // marking, so a run that reports only one of them has not discharged
    // anything. g031k8 is ARMv6-M: its MPU has no unprivileged-distinguishing
    // AP encodings to test, so it is excluded by hardware, not by convenience.
    Entry {
        bin: "gust_priv_region_probe",
        extra: &["priv-private"],
        needs_env: None,
        boards: &["wl55jc", "wb55rg", "g474re"],
        not_on: &[
            ("f100", "the part has no MPU (MPU_TYPE reads 0) — by hardware"),
            ("g031k8", "ARMv6-M PMSA has no AP encodings distinguishing the modes"),
        ],
    },
    Entry {
        bin: "gust_priv_region_probe",
        extra: &["priv-shared"],
        needs_env: None,
        boards: &["wl55jc", "wb55rg", "g474re"],
        not_on: &[
            ("f100", "the part has no MPU (MPU_TYPE reads 0) — by hardware"),
            ("g031k8", "ARMv6-M PMSA has no AP encodings distinguishing the modes"),
        ],
    },
    // --- the OS layer: gust:os nodes, the verified executor, the health monitor ---
    // Each was a qemu-only "LOCAL liveness probe". The dissolved ones link an ARMv7-M
    // object, so they cannot run on the G031 (synth#1301); the pure-Rust ones can.
    Entry {
        bin: "gust_exec_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links exec-cm3.o, an ARMv7-M object (synth#1301)")],
    },
    Entry {
        bin: "gust_os_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links os-time-cm3.o, an ARMv7-M object (synth#1301)")],
    },
    Entry {
        bin: "gust_os_tl_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links an ARMv7-M gust:os object (synth#1301)")],
    },
    Entry {
        bin: "gust_os_ts_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links an ARMv7-M gust:os object (synth#1301)")],
    },
    // --- module COMBINATIONS: composed/fused images, verdict by the firmware's OK/FAIL line ---
    // gust_two_tenant never exits (Renode-gated); it needs a self-check before it can be
    // a silicon row. gust_breadth used to exit SUCCESS unconditionally (Renode reads its
    // UART lines); under target-f100 it now clocks its peripherals and reports a
    // semihosting verdict, and its default Renode image is byte-identical.
    Entry {
        bin: "gust_osfused_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links gustos-dissolved-cm3.o, ARMv7-M (synth#1301)")],
    },
    // gust_stack / gust_fused: wasm-kernel/fused.o touches data bytes at wasm 0x10_000C
    // AND 0x21_0014 through one r11 base — 1.1 MiB apart, more RAM than any board here
    // has. Measured on silicon: gust_stack prints its first line on the G474 and then
    // makes no further semihosting call for 120 s (the fault itself is not yet read out).
    // The same pattern in os-time-cm3.o was a precise BusFault at 0x0010_0008.
    Entry {
        bin: "gust_stack",
        extra: &[],
        needs_env: None,
        boards: &[],
        not_on: &[("f100", FUSED_SPAN), ("wl55jc", FUSED_SPAN), ("wb55rg", FUSED_SPAN), ("g474re", FUSED_SPAN),
                  ("g031k8", "ARMv7-M object (synth#1301)")],
    },
    Entry {
        bin: "gust_fused",
        extra: &[],
        needs_env: None,
        boards: &[],
        not_on: &[("f100", FUSED_SPAN), ("wl55jc", FUSED_SPAN), ("wb55rg", FUSED_SPAN), ("g474re", FUSED_SPAN),
                  ("g031k8", "ARMv7-M object (synth#1301)")],
    },
    Entry {
        bin: "gust_control",
        extra: &[],
        needs_env: None,
        boards: &["wl55jc", "wb55rg", "g474re"],
        not_on: &[("f100", "does not fit: .bss overflows the part's 8 KB SRAM by 2420 bytes (link error, by hardware)"),
                  ("g031k8", "links the dissolved ARMv7-M control_step (synth#1301)")],
    },
    Entry {
        bin: "gust_dma_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g474re"],
        not_on: &[("g031k8", "links dma-own, ARMv7-M (synth#1301)")],
    },
    Entry {
        bin: "gust_breadth",
        extra: &[],
        needs_env: None,
        boards: &["f100"],
        not_on: &[("wl55jc", "STM32F1 peripheral addresses (GPIOC CRH, TIM2, SPI1, USART1) — no model of those bases on this part"),
                  ("wb55rg", "STM32F1 peripheral addresses — no model of those bases on this part"),
                  ("g474re", "STM32F1 peripheral addresses — the G474 model carries no USART/GPIO/TIM/SPI bases"),
                  ("g031k8", "ARMv7-M object (synth#1301) and F1 peripheral addresses")],
    },
    // REQ-OS-MPU-001's kill-criterion (synth#1145) on silicon: two synth memories, the
    // verified region programmer, an unprivileged escape at tenant B's base. The pair is
    // the evidence — CONTAINED with regions, ESCAPED (landing in B) without.
    Entry {
        bin: "gust_two_tenant",
        extra: &[],
        needs_env: Some(("GUST_TWO_TENANT_O", "drivers/run-two-tenant.sh (pinned synth) → /tmp/gale-two-tenant/two_tenant.o")),
        boards: &["wb55rg"],
        not_on: &[("g474re", "96 KiB SRAM cannot hold two 64 KiB wasm pages plus a stack (needs synth's used-extent symbol, v0.69)"),
                  ("wl55jc", "64 KiB SRAM cannot hold two 64 KiB wasm pages"),
                  ("g031k8", "8 KiB SRAM, and ARMv7-M object (synth#1301)"),
                  ("f100", "no MPU")],
    },
    Entry {
        bin: "gust_two_tenant",
        extra: &["no-regions"],
        needs_env: Some(("GUST_TWO_TENANT_O", "drivers/run-two-tenant.sh (pinned synth) → /tmp/gale-two-tenant/two_tenant.o")),
        boards: &["wb55rg"],
        not_on: &[],
    },
    Entry {
        bin: "gust_hm_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g031k8", "g474re"],
        not_on: &[],
    },
    Entry {
        bin: "gust_timer_probe",
        extra: &[],
        needs_env: None,
        boards: &["f100", "wl55jc", "wb55rg", "g031k8", "g474re"],
        not_on: &[],
    },
];

/// Every (bin, extra features) pair the matrix names, on any board. The
/// armv6m-isa gate builds each for the G031K8 from this table rather than a
/// copy of it, so a feature set added here is checked there.
pub(crate) fn variants() -> Vec<(&'static str, &'static [&'static str])> {
    MATRIX.iter().map(|e| (e.bin, e.extra)).collect()
}

const FUSED_SPAN: &str = "fused.o addresses data 1.1 MiB apart (0x10_000C, 0x21_0014) through one r11 base: no board's RAM spans it; needs a packed-memory rebuild";

const DEFAULT_TIMEOUT_S: u64 = 150;

fn board(name: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|b| b.name == name)
}

/// Single-quote for a POSIX shell (the remote side of ssh).
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `pub const NAME: u32 = 0x5800_0094;` -> 0x58000094, from a generated target module.
fn generated_u32(text: &str, name: &str) -> Option<u32> {
    let key = format!("pub const {name}: u32 = ");
    let line = text.lines().find(|l| l.trim_start().starts_with(&key))?;
    let v = line.split_once(" = ")?.1.trim_end_matches(';').trim().replace('_', "");
    if let Some((a, b)) = v.split_once(" << ") {
        return Some(a.trim().parse::<u32>().ok()? << b.trim().parse::<u32>().ok()?);
    }
    match v.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => v.parse().ok(),
    }
}

/// The serial for `claim` in this host's bench registry — the same lookup bench-claim.sh
/// does, so the lock and the probe selector cannot name different boards.
fn local_registry_serial(claim: &str) -> Option<String> {
    let reg = std::env::var("BENCH_REGISTRY").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/pulseengine/bench-devices.yaml")
    });
    let text = fs::read_to_string(reg).ok()?;
    let mut inside = false;
    for l in text.lines() {
        if l == format!("  {claim}:") {
            inside = true;
            continue;
        }
        if inside {
            if l.starts_with("  ") && !l.starts_with("    ") && l.trim_end().ends_with(':') {
                return None;
            }
            if let Some(v) = l.trim().strip_prefix("serial:") {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Restores os/gust/memory.x however the build ends.
struct MemoryGuard {
    path: PathBuf,
    original: Vec<u8>,
}
impl Drop for MemoryGuard {
    fn drop(&mut self) {
        let _ = fs::write(&self.path, &self.original);
    }
}

/// The cargo target directory the gust build writes to. Passed to cargo explicitly as
/// `--target-dir`, so the ELF path below and the directory cargo builds into cannot name
/// two different places: an inherited `CARGO_TARGET_DIR` used to send the build
/// elsewhere while this side looked for the ELF under os/gust/target. A relative value
/// is relative to `gust`, which is where cargo runs and resolves it.
fn gust_target_dir(gust: &Path, cargo_target_dir: Option<std::ffi::OsString>) -> PathBuf {
    match cargo_target_dir.filter(|v| !v.is_empty()) {
        Some(d) => gust.join(d),
        None => gust.join("target"),
    }
}

/// The cargo invocation for one board image. Separate so a test can see that
/// `--target-dir` reaches cargo: without it cargo builds wherever its environment says
/// while build() looks for the ELF under `target_dir`.
fn cargo_build(gust: &Path, b: &Board, bin: &str, feats: &str, target_dir: &Path) -> Command {
    let mut c = Command::new("cargo");
    c.current_dir(gust)
        .args([
            "build",
            "--release",
            "--bin",
            bin,
            "--no-default-features",
            "--target",
            b.triple,
        ])
        .arg("--target-dir")
        .arg(target_dir)
        .args(["--features", feats]);
    c
}

/// Build `bin` for `b` into `target_dir` (see [`gust_target_dir`]) and stage its flash
/// image. The caller supplies the target dir; nothing here reads the environment.
fn build(
    root: &Path,
    b: &Board,
    bin: &str,
    extra: &[&str],
    target_dir: &Path,
) -> Result<PathBuf, String> {
    let gust = root.join("os/gust");
    let mem = gust.join("memory.x");
    // The generated maps live at the repo root (targets/*.aadl -> targets/generated/),
    // NOT under os/gust: joining them onto `gust` made every run FAIL on this read.
    let gen = root.join(format!("targets/generated/memory-{}.x", b.stem));
    let original = fs::read(&mem).map_err(|e| format!("read {}: {e}", mem.display()))?;
    let generated = fs::read(&gen).map_err(|e| format!("read {}: {e}", gen.display()))?;
    let _guard = MemoryGuard { path: mem.clone(), original };
    fs::write(&mem, generated).map_err(|e| format!("write memory.x: {e}"))?;

    // Force a relink: memory.x is not a cargo input, so a stale ELF linked against the
    // PREVIOUS board's map would otherwise be reused silently.
    let elf = target_dir.join(format!("{}/release/{bin}", b.triple));
    let _ = fs::remove_file(&elf);

    let mut feats: Vec<&str> = b.features.to_vec();
    feats.extend_from_slice(extra);
    let out = cargo_build(&gust, b, bin, &feats.join(","), target_dir)
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().filter(|l| l.starts_with("error")).take(5).collect();
        return Err(format!("build failed for {bin} on {}: {}", b.name, tail.join(" | ")));
    }
    if !elf.is_file() {
        return Err(format!("cargo succeeded but {} does not exist", elf.display()));
    }
    let staged = std::env::temp_dir().join(format!("gale-silicon-{}-{bin}{}.hex", b.name,
        if extra.is_empty() { String::new() } else { format!("-{}", extra.join("-")) }));
    flash_image(&elf, &staged)?;
    Ok(staged)
}

/// What actually goes onto the part: an Intel HEX of the FLASH-addressed contents only.
///
/// An ELF can carry loadable RAM segments. gust_two_tenant does: synth's
/// `.synth.wasm_mem_1` is PROGBITS at 0x2002_0000, and lld folds it (with .bss/.uninit)
/// into one in-file RAM segment. openocd's `program … verify` then fails ("no flash bank
/// found for address 0x20000020", "Verify Failed"). Renode never sees this — it writes
/// ELF segments straight into memory. On a part, RAM holds nothing across reset: its
/// contents are the startup code's and the embedder's job, never the debugger's.
fn flash_image(elf: &Path, hex: &Path) -> Result<(), String> {
    let objcopy = ["arm-none-eabi-objcopy", "llvm-objcopy"]
        .into_iter()
        .find(|t| Command::new(t).arg("--version").output().is_ok())
        .ok_or("no arm-none-eabi-objcopy or llvm-objcopy on PATH")?;
    let objdump = ["arm-none-eabi-objdump", "llvm-objdump"]
        .into_iter()
        .find(|t| Command::new(t).arg("--version").output().is_ok())
        .ok_or("no arm-none-eabi-objdump or llvm-objdump on PATH")?;
    let hdr = Command::new(objdump).arg("-h").arg(elf).output().map_err(|e| format!("objdump: {e}"))?;
    let mut remove: Vec<String> = vec![".bss".into(), ".uninit".into()];
    for l in String::from_utf8_lossy(&hdr.stdout).lines() {
        if let Some(name) = l.split_whitespace().nth(1) {
            if name.starts_with(".synth.wasm_mem_") {
                remove.push(name.to_string());
            }
        }
    }
    let mut c = Command::new(objcopy);
    c.args(["-O", "ihex"]);
    for r in &remove {
        c.args(["-R", r]);
    }
    let st = c.arg(elf).arg(hex).status().map_err(|e| format!("objcopy: {e}"))?;
    if !st.success() {
        return Err(format!("objcopy -O ihex failed for {}", elf.display()));
    }
    Ok(())
}

/// Address range [lo, hi) covered by an Intel HEX's data records.
fn hex_range(hex: &str) -> Option<(u32, u32)> {
    let (mut base, mut lo, mut hi) = (0u32, u32::MAX, 0u32);
    for line in hex.lines() {
        let b: Vec<u8> = (1..line.trim().len()).step_by(2)
            .filter_map(|i| u8::from_str_radix(line.trim().get(i..i + 2)?, 16).ok()).collect();
        if b.len() < 4 { continue; }
        let (len, addr, typ) = (b[0] as u32, ((b[1] as u32) << 8) | b[2] as u32, b[3]);
        match typ {
            0 => { let a = base + addr; lo = lo.min(a); hi = hi.max(a + len); }
            2 if b.len() >= 6 => base = (((b[4] as u32) << 8) | b[5] as u32) << 4,
            4 if b.len() >= 6 => base = (((b[4] as u32) << 8) | b[5] as u32) << 16,
            _ => {}
        }
    }
    (lo != u32::MAX).then_some((lo, hi))
}

/// Run `cmd`, killing it at the deadline. Returns (exit code or None if killed, output).
fn run_with_deadline(mut cmd: Command, deadline: Duration) -> Result<(Option<i32>, String), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || { let mut s = String::new(); let _ = out.read_to_string(&mut s); s });
    let t_err = std::thread::spawn(move || { let mut s = String::new(); let _ = err.read_to_string(&mut s); s });
    let start = Instant::now();
    let code = loop {
        match child.try_wait().map_err(|e| format!("wait: {e}"))? {
            Some(st) => break st.code(),
            None if start.elapsed() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    let mut text = t_out.join().unwrap_or_default();
    text.push_str(&t_err.join().unwrap_or_default());
    Ok((code, text))
}

fn ssh(host: &str, remote: &str) -> Command {
    let mut c = Command::new("ssh");
    c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=30", host, remote]);
    c
}

pub fn run(root: &Path, board_name: &str, bin: &str, extra: &[&str], timeout_s: u64) -> Verdict {
    run_with_env(root, board_name, bin, extra, timeout_s, |k| {
        std::env::var(k).ok()
    })
}

/// run() with the environment supplied by `env` (GALE_SILICON_*, CARGO_TARGET_DIR), so
/// tests can drive it without touching the process environment.
fn run_with_env(
    root: &Path,
    board_name: &str,
    bin: &str,
    extra: &[&str],
    timeout_s: u64,
    env: impl Fn(&str) -> Option<String>,
) -> Verdict {
    let Some(b) = board(board_name) else {
        return Verdict::Refused(format!("no board '{board_name}' (see `xtask silicon list`)"));
    };
    if let Some(why) = b.not_yet {
        return Verdict::Refused(format!("{}: not yet — {why}", b.name));
    }
    // Before any build: a board with no configured site is REFUSED, not built for nothing.
    let site = match resolve(b, &env) {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(e),
    };
    let consts = match fs::read_to_string(root.join(format!("targets/generated/gust_target_{}.rs", b.stem))) {
        Ok(t) => t,
        Err(e) => return Verdict::Refused(format!("generated target module for {}: {e}", b.stem)),
    };
    let (Some(csr), Some(rmvf)) = (generated_u32(&consts, "RCC_CSR"), generated_u32(&consts, "RMVF")) else {
        return Verdict::Refused(format!("RCC_CSR/RMVF missing from gust_target_{}.rs", b.stem));
    };

    let target_dir = gust_target_dir(
        &root.join("os/gust"),
        env("CARGO_TARGET_DIR").map(Into::into),
    );
    // A build that does not produce an image is "could not run", not a firmware FAIL:
    // exit 1 is reserved for the firmware's own `<bin> FAIL` line.
    let elf = match build(root, b, bin, extra, &target_dir) {
        Ok(p) => p,
        Err(e) => return Verdict::Refused(format!("{}: no image to flash — {e}", b.name)),
    };
    // Refuse an image that would write outside the part's flash (from the measured model).
    let (Some(fbase), Some(flen)) = (generated_u32(&consts, "FLASH_BASE"), generated_u32(&consts, "FLASH_LEN")) else {
        return Verdict::Refused(format!("FLASH_BASE/FLASH_LEN missing from gust_target_{}.rs", b.stem));
    };
    match fs::read_to_string(&elf).ok().as_deref().and_then(hex_range) {
        Some((lo, hi)) if lo >= fbase && hi <= fbase.wrapping_add(flen) => {}
        Some((lo, hi)) => return Verdict::Refused(format!(
            "{}: image data spans {lo:#010x}..{hi:#010x}, outside flash {fbase:#010x}+{flen:#x} — not flashing it", b.name)),
        None => return Verdict::Refused(format!("{}: flash image has no data records", b.name)),
    }

    let remote_elf = format!("/tmp/{}", elf.file_name().unwrap().to_string_lossy());
    let fw = match &site.host {
        Host::Local => elf.to_string_lossy().to_string(),
        Host::Ssh(h) => {
            let st = Command::new("scp").args(["-q", &elf.to_string_lossy(), &format!("{h}:{remote_elf}")]).status();
            if !matches!(st, Ok(s) if s.success()) {
                return Verdict::Refused(format!("could not copy the ELF to {h}"));
            }
            remote_elf.clone()
        }
    };

    // openocd, in its event loop: semihosting is serviced only while openocd polls, and
    // a batch `sleep` did not carry output across the watchdog reset. Each semihosting
    // call costs ~100 ms of halt on these probes, so the deadline is generous.
    // The deadline runs ON THE PROBE HOST, around openocd itself, so a dropped ssh or a
    // killed xtask cannot leave openocd holding the probe. macOS has no `timeout`;
    // `perl -e alarm` is the same contract there (the alarm survives exec, and SIGALRM
    // terminates openocd). Found by the first local run: with-device reported
    // "No such file or directory" for `timeout`, and the matrix recorded REFUSED.
    let mut ocd: Vec<String> = match &site.host {
        Host::Ssh(_) => vec!["timeout".into(), timeout_s.to_string()],
        // Forks rather than exec: with-device reports a child killed by a signal as exit 1,
        // which is indistinguishable from openocd's own failure. The parent turns the
        // deadline into an explicit 124, the same code `timeout` gives on the remote side.
        Host::Local => vec!["perl".into(), "-e".into(),
            "my $t = shift @ARGV; my $pid = fork() // die \"fork: $!\"; if (!$pid) { exec @ARGV or die \"exec: $!\" } \
             $SIG{ALRM} = sub { kill 'TERM', $pid; sleep 2; kill 'KILL', $pid; exit 124 }; alarm $t; waitpid($pid, 0); \
             exit($? & 127 ? 128 + ($? & 127) : $? >> 8)".into(),
            timeout_s.to_string()],
    };
    ocd.push("openocd".into());
    ocd.extend(b.iface.iter().map(|s| s.to_string()));
    if let Some(loc) = &site.usb_location {
        ocd.extend(["-c".into(), format!("adapter usb location {loc}")]);
    }
    ocd.extend(b.target.iter().map(|s| s.to_string()));
    // Pin::RegistrySerial over ssh adds nothing here: the serial lives in the PROBE host's
    // registry, which this side does not read. That is the pre-existing behaviour.
    if let Host::Local = site.host {
        match local_registry_serial(b.claim) {
            Some(serial) => ocd.extend(["-c".into(), format!("adapter serial {serial}")]),
            None => return Verdict::Refused(format!("no serial for '{}' in this host's bench registry", b.claim)),
        }
    }
    ocd.extend(["-c".into(), "gdb_port disabled; tcl_port disabled; telnet_port disabled".into(), "-c".into(), "init".into()]);
    let halt_clean: Vec<String> = match b.reset {
        Reset::ResetHalt => vec!["-c".into(), "reset halt".into()],
        Reset::HaltSysreset => vec!["-c".into(), "halt".into()],
    };
    ocd.extend(halt_clean);
    ocd.extend(["-c".into(), format!("program {fw} verify")]);
    // After programming, always `reset halt`. A plain `halt` after `program` timed out on
    // the F100 for gust_exec_probe and gust_os_probe (while gust_wdg_silicon got through
    // on the same sequence), leaving the run REFUSED with the new image never started.
    ocd.extend(["-c".into(), "reset halt".into()]);
    // Clear the reset flags with CSR | RMVF — never RMVF alone; the low bits of some
    // families' CSR are oscillator configuration (stm32wl55.aadl).
    ocd.extend(["-c".into(), format!("mww {csr:#010x} [expr {{[lindex [read_memory {csr:#010x} 32 1] 0] | {rmvf:#010x}}}]")]);
    ocd.extend(["-c".into(), "arm semihosting enable".into(), "-c".into(), "resume".into()]);

    let purpose = format!("gale silicon: {bin}{} on {}", if extra.is_empty() { String::new() } else { format!(" +{}", extra.join("+")) }, b.name);
    let deadline = Duration::from_secs(timeout_s + 60);
    let result = match &site.host {
        Host::Local => {
            let wd = Command::new("varve").current_dir(root).args(["which", "with-device"]).output();
            let wd = match wd {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string(),
                _ => return Verdict::Refused("with-device is not resolvable through the varve pin on this host".into()),
            };
            let mut c = Command::new(wd);
            c.env("BENCH_WHO", "gale").args([b.claim, "--purpose", &purpose, "--"]).args(&ocd);
            run_with_deadline(c, deadline)
        }
        Host::Ssh(h) => {
            // Stage THIS checkout's pin on the probe host and resolve with-device through
            // it, so the claim is asserted by the pinned tool (bench-claim.sh claim_remote).
            let stage = ssh(h, "mkdir -p gale-bench").status();
            let cp = Command::new("scp").args(["-q"]).arg(root.join("varve.toml")).arg(root.join("varve-realms.toml"))
                .arg(format!("{h}:gale-bench/")).status();
            if !matches!(stage, Ok(s) if s.success()) || !matches!(cp, Ok(s) if s.success()) {
                return Verdict::Refused(format!("could not stage the varve pin on {h}"));
            }
            let remote = format!(
                "cd gale-bench && wd=$($HOME/.varve/bin/varve which with-device 2>/dev/null | head -1) && [ -x \"$wd\" ] \
                 || {{ echo 'xtask-silicon: no pinned with-device on this host' >&2; exit 97; }}; \
                 BENCH_WHO=gale \"$wd\" {} --purpose {} -- {}",
                sq(b.claim), sq(&purpose), ocd.iter().map(|a| sq(a)).collect::<Vec<_>>().join(" ")
            );
            run_with_deadline(ssh(h, &remote), deadline)
        }
    };
    let (code, text) = match result {
        Ok(r) => r,
        Err(e) => return Verdict::Refused(e),
    };

    let log = root.join(format!("os/gust/target/silicon-logs/{}-{bin}{}.log", b.name,
        if extra.is_empty() { String::new() } else { format!("-{}", extra.join("-")) }));
    let _ = fs::create_dir_all(log.parent().unwrap());
    let _ = fs::write(&log, &text);

    firmware_verdict(b, bin, code, &text, &log)
}

/// The verdict from what came back through the probe: the firmware's own lines first,
/// then the exit status — which on its own is never a PASS.
///
/// PASS needs the firmware's `<bin> OK` line (`<bin>` with `_` as `-`, the form every
/// matrix firmware prints), FAIL its `<bin> FAIL` line; anything else is REFUSED.
///
/// Exit 0 WITHOUT the OK line is REFUSED, not PASS. The status arrives through
/// with-device, the deadline wrapper (perl locally, `timeout` remotely), ssh, and
/// openocd, and every one of them can produce a 0 of its own. openocd's "application
/// exited normally" line is, by its source, printed only while a gdb client is attached
/// (gdb is disabled here); otherwise SYS_EXIT becomes a bare process exit. So this runner
/// has no observation that tells the firmware's SYS_EXIT 0 apart from any other 0 in that
/// chain, and until it does, a 0 is "could not tell", which is what REFUSED means.
fn firmware_verdict(b: &Board, bin: &str, code: Option<i32>, text: &str, log: &Path) -> Verdict {
    let prefix = bin.replace('_', "-");
    let find = |tag: &str| verdict_line(text, &prefix, tag).map(str::to_string);
    let mut detail = vec![format!(
        "{} ({}): exit {:?}, log {}",
        b.name,
        b.what,
        code,
        log.display()
    )];
    if let Some(l) = find("FAIL") {
        detail.push(l);
        return Verdict::Fail(detail);
    }
    if let Some(l) = find("OK") {
        detail.push(l);
        return Verdict::Pass(detail);
    }
    // Exit 1 is NOT treated as a firmware FAIL, even when the firmware printed: the first
    // matrix run labelled gust_stack on the G474 "FAIL" when it had HUNG — with-device
    // reports a deadline-killed child as 1. A firmware failure must say so in a line.
    match code {
        Some(0) => Verdict::Refused(format!(
            "{}: exit 0 but no '{prefix} OK' line — the exit status alone does not show the firmware's \
             semihosting SYS_EXIT 0 (with-device, the deadline wrapper and openocd can each exit 0); not a pass — see {}",
            b.name, log.display())),
        Some(3) => Verdict::Refused(format!("{}: probe '{}' is claimed by someone else", b.name, b.claim)),
        Some(124) => Verdict::Refused(format!("{}: deadline passed with no verdict — see {}", b.name, log.display())),
        Some(97) => Verdict::Refused(format!("{}: no pinned with-device on the probe host", b.name)),
        None => Verdict::Refused(format!("{}: no verdict line before the deadline — see {}", b.name, log.display())),
        _ => Verdict::Refused(format!("{}: firmware printed no verdict (exit {:?}) — see {}", b.name, code, log.display())),
    }
}

/// The first `<prefix> <tag>` in `text`, from the prefix to the end of its line, matched
/// as a whole token: the prefix starts a line or follows a character that cannot be part
/// of a bin name (`[A-Za-z0-9_-]`), and the tag is followed by the end of the line, `:`,
/// `(` or whitespace. Case-sensitive. So `gust-x OK(private)` and `gust-x OK: ...` match;
/// `gust-x OKAY`, `gust-x OK-ish`, `xgust-x OK`, `'gust-x OK'` and, for bin `x`,
/// `silicon-x OK` do not.
fn verdict_line<'a>(text: &'a str, prefix: &str, tag: &str) -> Option<&'a str> {
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let needle = format!("{prefix} {tag}");
    for line in text.lines() {
        for (i, _) in line.match_indices(&needle) {
            let before_ok = line[..i].chars().next_back().is_none_or(|c| !name_char(c));
            let after_ok = line[i + needle.len()..]
                .chars()
                .next()
                .is_none_or(|c| c == ':' || c == '(' || c.is_whitespace());
            if before_ok && after_ok {
                return Some(&line[i..]);
            }
        }
    }
    None
}

/// A matrix row whose input cargo does not build (an object from a pinned toolchain run)
/// is REFUSED when the variable carrying that input is unset; `None` means it may run.
fn needs_env_refusal(e: &Entry, bd: &str, env: impl Fn(&str) -> Option<String>) -> Option<Verdict> {
    let (var, how) = e.needs_env?;
    match env(var) {
        Some(_) => None,
        None => Some(Verdict::Refused(format!(
            "{bd}: {var} is not set — produce it with {how}"
        ))),
    }
}

/// `matrix`: at least one requested row is NOT-YET (and none FAIL or REFUSED).
pub const EXIT_NOT_YET: i32 = 4;
/// `matrix`: the filters selected no row at all.
pub const EXIT_EMPTY: i32 = 5;

/// One matrix row as reported.
struct Row {
    board: String,
    bin: String,
    tag: &'static str,
    why: String,
}

fn entry_label(e: &Entry) -> String {
    if e.extra.is_empty() {
        e.bin.to_string()
    } else {
        format!("{} +{}", e.bin, e.extra.join("+"))
    }
}

fn verdict_tag(v: &Verdict) -> &'static str {
    match v {
        Verdict::Pass(_) => "PASS",
        Verdict::Fail(_) => "FAIL",
        Verdict::Refused(_) => "REFUSED",
    }
}

/// Filter names that match no board / no matrix bin, as messages.
fn unknown_names(boards: Option<&[String]>, bins: Option<&[String]>) -> Vec<String> {
    let mut out = Vec::new();
    for bd in boards.unwrap_or_default() {
        if board(bd).is_none() {
            out.push(format!("unknown board '{bd}'"));
        }
    }
    for bin in bins.unwrap_or_default() {
        if !MATRIX.iter().any(|e| e.bin == bin) {
            out.push(format!("unknown bin '{bin}' (not in the silicon matrix)"));
        }
    }
    out
}

/// The requested rows: runnable pairs through `runner`, plus every NOT-YET pair the
/// filters select (listed with its reason rather than left out).
fn matrix_rows(
    boards_filter: Option<&[String]>,
    bins_filter: Option<&[String]>,
    mut runner: impl FnMut(&Entry, &str) -> Verdict,
) -> Vec<Row> {
    let board_ok = |bd: &str| boards_filter.is_none_or(|f| f.iter().any(|x| x == bd));
    let mut rows = Vec::new();
    for e in MATRIX {
        if bins_filter.is_some_and(|f| !f.iter().any(|x| x == e.bin)) {
            continue;
        }
        let label = entry_label(e);
        for bd in e.boards.iter().filter(|bd| board_ok(bd)) {
            let v = runner(e, bd);
            let why = match &v {
                Verdict::Pass(d) | Verdict::Fail(d) => d.last().cloned().unwrap_or_default(),
                Verdict::Refused(r) => r.clone(),
            };
            rows.push(Row {
                board: bd.to_string(),
                bin: label.clone(),
                tag: verdict_tag(&v),
                why,
            });
        }
        for (bd, why) in e.not_on.iter().filter(|(bd, _)| board_ok(bd)) {
            rows.push(Row {
                board: bd.to_string(),
                bin: label.clone(),
                tag: "NOT-YET",
                why: why.to_string(),
            });
        }
    }
    for b in BOARDS.iter().filter(|b| board_ok(b.name)) {
        if let Some(why) = b.not_yet {
            rows.push(Row {
                board: b.name.to_string(),
                bin: "(all)".into(),
                tag: "NOT-YET",
                why: why.to_string(),
            });
        }
    }
    rows
}

/// The matrix exit code: 0 only when there is at least one row and every row PASSED.
/// Precedence FAIL (1) > REFUSED (3) > NOT-YET (4); no rows at all is EMPTY (5).
fn matrix_exit<'a>(tags: impl IntoIterator<Item = &'a str>) -> i32 {
    let tags: Vec<&str> = tags.into_iter().collect();
    if tags.is_empty() {
        EXIT_EMPTY
    } else if tags.contains(&"FAIL") {
        EXIT_FAIL
    } else if tags.contains(&"REFUSED") {
        EXIT_REFUSED
    } else if tags.contains(&"NOT-YET") {
        EXIT_NOT_YET
    } else if tags.iter().all(|t| *t == "PASS") {
        EXIT_PASS
    } else {
        EXIT_REFUSED
    }
}

/// `xtask silicon matrix`: every requested row, printed, and the exit code that
/// summarises them. `runner` answers each runnable (bin, board) pair; main passes the
/// real flash-and-run, the tests a stub.
fn matrix(
    boards_filter: Option<&[String]>,
    bins_filter: Option<&[String]>,
    json: bool,
    runner: impl FnMut(&Entry, &str) -> Verdict,
) -> i32 {
    // Unknown names first, before anything is built: a misspelt board or bin used to
    // select zero rows and exit 0, which reads exactly like "all green".
    let unknown = unknown_names(boards_filter, bins_filter);
    if !unknown.is_empty() {
        for u in &unknown {
            eprintln!("xtask silicon matrix: {u}");
        }
        eprintln!(
            "(see `xtask silicon list` for boards; bins are the firmware names in the matrix)"
        );
        return EXIT_USAGE;
    }
    let rows = matrix_rows(boards_filter, bins_filter, runner);
    if json {
        let items: Vec<String> = rows
            .iter()
            .map(|r| {
                format!(
                    "{{\"board\":\"{}\",\"bin\":\"{}\",\"result\":\"{}\",\"detail\":\"{}\"}}",
                    r.board,
                    r.bin,
                    r.tag,
                    r.why.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect();
        println!("[{}]", items.join(","));
    } else {
        println!("{:<8} {:<34} {:<8} detail", "board", "firmware", "result");
        for r in &rows {
            println!(
                "{:<8} {:<34} {:<8} {}",
                r.board,
                r.bin,
                r.tag,
                r.why.chars().take(140).collect::<String>()
            );
        }
    }
    let count = |t: &str| rows.iter().filter(|r| r.tag == t).count();
    let code = matrix_exit(rows.iter().map(|r| r.tag));
    eprintln!(
        "matrix: {} PASS, {} FAIL, {} REFUSED, {} NOT-YET of {} row(s) — exit {code}{}",
        count("PASS"),
        count("FAIL"),
        count("REFUSED"),
        count("NOT-YET"),
        rows.len(),
        match code {
            EXIT_PASS => "",
            EXIT_NOT_YET => " (a NOT-YET row is a gap, not a pass)",
            EXIT_EMPTY => " (the filters selected nothing; that is not a pass)",
            _ => " (not a pass)",
        }
    );
    code
}

pub fn usage() -> &'static str {
    "xtask silicon — gust firmware on physical boards, verdict from the firmware\n\n\
     USAGE:\n  \
       cargo xtask silicon list\n  \
       cargo xtask silicon run <board> <bin> [--features a,b] [--timeout SECS] [--format json]\n  \
       cargo xtask silicon matrix [--boards a,b] [--bins x,y] [--format json]\n\n\
     CONFIGURATION (per board; nothing about the bench is stored in the repo):\n  \
       GALE_SILICON_<BOARD>_HOST          `local`, or the ssh destination of the probe host\n  \
       GALE_SILICON_<BOARD>_USB_LOCATION  openocd `adapter usb location` (wl55jc, wb55rg, g031k8)\n  \
       BENCH_REGISTRY                     local probe's serial registry (default ~/.config/pulseengine/bench-devices.yaml)\n  \
       An unset variable REFUSES the board and names the variable. See os/gust/silicon/README.md.\n\n\
     VERDICT (run): PASS only on the firmware's `<bin> OK` line (`_` printed as `-`); FAIL on its\n  \
       `<bin> FAIL` line; anything else, INCLUDING exit 0 with no OK line, is REFUSED.\n\n\
     EXIT CODES:\n  \
       0  pass — run: the OK line; matrix: at least one row, and every row PASS\n  \
       1  fail — a `<bin> FAIL` line (matrix: any row FAIL)\n  \
       2  usage — bad flags, or (matrix) an unknown --boards / --bins name\n  \
       3  could not run — busy probe, no verdict, unconfigured board (matrix: any row REFUSED)\n  \
       4  matrix: a requested row is NOT-YET (a known gap: unsupported pair or not-yet board)\n  \
       5  matrix: the filters selected no row at all\n  \
       Only 0 is a pass. Precedence when rows disagree: 1 > 3 > 4.\n"
}

pub fn main(root: &Path, args: &[String]) -> i32 {
    let mut json = false;
    let mut features: Vec<String> = Vec::new();
    let mut boards_filter: Option<Vec<String>> = None;
    let mut bins_filter: Option<Vec<String>> = None;
    let mut timeout_s = DEFAULT_TIMEOUT_S;
    let mut pos: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--help" | "-h" => {
                print!("{}", usage());
                return EXIT_PASS;
            }
            "--format" => match it.next().map(String::as_str) {
                Some("json") => json = true,
                _ => { eprintln!("xtask silicon: --format takes 'json'"); return EXIT_USAGE; }
            },
            "--features" => match it.next() {
                Some(v) => features = v.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
                None => { eprintln!("xtask silicon: --features needs a value"); return EXIT_USAGE; }
            },
            "--boards" => match it.next() {
                Some(v) => boards_filter = Some(v.split(',').map(String::from).collect()),
                None => { eprintln!("xtask silicon: --boards needs a value"); return EXIT_USAGE; }
            },
            "--bins" => match it.next() {
                Some(v) => bins_filter = Some(v.split(',').map(String::from).collect()),
                None => { eprintln!("xtask silicon: --bins needs a value"); return EXIT_USAGE; }
            },
            "--timeout" => match it.next().and_then(|v| v.parse().ok()) {
                Some(v) => timeout_s = v,
                None => { eprintln!("xtask silicon: --timeout needs seconds"); return EXIT_USAGE; }
            },
            s if s.starts_with('-') => { eprintln!("xtask silicon: unknown flag '{s}'\n\n{}", usage()); return EXIT_USAGE; }
            s => pos.push(s.to_string()),
        }
    }
    match pos.first().map(String::as_str) {
        Some("list") => {
            for b in BOARDS {
                let host = match resolve(b, |k| std::env::var(k).ok()) {
                    Ok(Site { host: Host::Local, .. }) => "local".to_string(),
                    Ok(Site { host: Host::Ssh(h), .. }) => h,
                    Err(_) => format!("(unset: {})", env_key(b.name, "HOST")),
                };
                println!("{:<8} {:<30} claim={:<14} {}{}", b.name, host, b.claim, b.what,
                    b.not_yet.map(|w| format!("  [NOT YET: {w}]")).unwrap_or_default());
            }
            EXIT_PASS
        }
        Some("run") => {
            let (Some(bd), Some(bin)) = (pos.get(1), pos.get(2)) else {
                eprintln!("{}", usage());
                return EXIT_USAGE;
            };
            let extra: Vec<&str> = features.iter().map(String::as_str).collect();
            let v = run(root, bd, bin, &extra, timeout_s);
            if json { println!("{}", v.to_json(&format!("silicon:{bd}:{bin}"))); } else { println!("{v}"); }
            v.code()
        }
        Some("matrix") => matrix(
            boards_filter.as_deref(),
            bins_filter.as_deref(),
            json,
            |e, bd| {
                let env = |k: &str| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned());
                let v = needs_env_refusal(e, bd, env)
                    .unwrap_or_else(|| run(root, bd, e.bin, e.extra, timeout_s));
                eprintln!("{:<8} {bd:<8} {}", verdict_tag(&v), entry_label(e));
                v
            },
        ),
        _ => { eprintln!("{}", usage()); EXIT_USAGE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_generated_constants() {
        let t = "pub const RCC_CSR: u32 = 0x5800_0094;\npub const RMVF: u32 = 1 << 23;\n";
        assert_eq!(generated_u32(t, "RCC_CSR"), Some(0x5800_0094));
        assert_eq!(generated_u32(t, "RMVF"), Some(1 << 23));
        assert_eq!(generated_u32(t, "IWDGRSTF"), None);
    }

    #[test]
    fn every_matrix_board_exists_and_every_bin_board_is_runnable() {
        for e in MATRIX {
            for bd in e.boards.iter().chain(e.not_on.iter().map(|(b, _)| b)) {
                assert!(board(bd).is_some(), "matrix names unknown board {bd}");
            }
            for bd in e.boards {
                assert!(board(bd).unwrap().not_yet.is_none(), "{} is scheduled on not-yet board {bd}", e.bin);
            }
        }
    }

    fn fixture(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn site_comes_from_the_environment() {
        let b = board("wl55jc").unwrap();
        let got = resolve(b, fixture(&[
            ("GALE_SILICON_WL55JC_HOST", "board.example.invalid"),
            ("GALE_SILICON_WL55JC_USB_LOCATION", "9-9.9"),
        ]));
        assert_eq!(got, Ok(Site { host: Host::Ssh("board.example.invalid".into()), usb_location: Some("9-9.9".into()) }));

        let f100 = board("f100").unwrap();
        assert_eq!(resolve(f100, fixture(&[("GALE_SILICON_F100_HOST", "local")])),
                   Ok(Site { host: Host::Local, usb_location: None }));
    }

    #[test]
    fn missing_site_config_names_the_variable() {
        let b = board("wl55jc").unwrap();
        let e = resolve(b, fixture(&[])).unwrap_err();
        assert!(e.contains("GALE_SILICON_WL55JC_HOST"), "{e}");
        // Empty is unset, not "a host called ''".
        let e = resolve(b, fixture(&[("GALE_SILICON_WL55JC_HOST", "  ")])).unwrap_err();
        assert!(e.contains("GALE_SILICON_WL55JC_HOST"), "{e}");
        // A host without the port pin is refused too: an unpinned ST-LINK attaches to
        // whichever board enumerates first (gale#397).
        let e = resolve(b, fixture(&[("GALE_SILICON_WL55JC_HOST", "board.example.invalid")])).unwrap_err();
        assert!(e.contains("GALE_SILICON_WL55JC_USB_LOCATION"), "{e}");
    }

    #[test]
    fn shared_vid_pid_probes_are_pinned() {
        // Several ST-LINK/V2-1 and V3 probes share a VID:PID; `interface/stlink.cfg` alone
        // attaches to the first one. Every such board must name how it is pinned.
        for b in BOARDS.iter().filter(|b| b.not_yet.is_none()) {
            match b.pin {
                Pin::VidPid => assert!(b.iface.iter().any(|a| a.starts_with("hla_vid_pid")),
                    "{} claims a VID:PID pin but its interface args select none", b.name),
                Pin::UsbLocation | Pin::RegistrySerial => {}
            }
            assert!(!b.iface.iter().chain(b.target).any(|a| a.starts_with("adapter usb location") || a.starts_with("adapter serial")),
                "{}: probe-host topology belongs in the environment, not the table", b.name);
        }
    }

    #[test]
    fn hex_range_follows_extended_linear_address_records() {
        // :02000004 0800 F2 sets base 0x0800_0000; one 4-byte record at offset 0x0010.
        let hex = ":020000040800F2\n:0400100001020304E2\n:00000001FF\n";
        assert_eq!(hex_range(hex), Some((0x0800_0010, 0x0800_0014)));
        assert_eq!(hex_range(":00000001FF\n"), None);
    }

    #[test]
    fn shell_quoting_survives_single_quotes() {
        assert_eq!(sq("a'b"), "'a'\\''b'");
    }

    /// A fresh scratch root shaped like the repo, unique per test, removed on drop —
    /// including when the test panics.
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn scratch_root(tag: &str) -> Scratch {
        let r = std::env::temp_dir().join(format!("xtask-silicon-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&r);
        fs::create_dir_all(r.join("os/gust")).unwrap();
        // A manifest cargo refuses to parse: any build stops here, deterministically and fast.
        fs::write(r.join("os/gust/Cargo.toml"), b"this is not toml [").unwrap();
        fs::write(r.join("os/gust/memory.x"), b"ORIGINAL").unwrap();
        Scratch(r)
    }

    /// Goes through build()'s OWN path join. With the generated map absent the error must
    /// name the repo-root path; with it present, build() must get past the read (to cargo,
    /// which this fixture's broken manifest stops) and restore memory.x afterwards. On the
    /// old join (os/gust/targets/generated/...) both halves fail. The target dir is inside
    /// the scratch root, so build()'s stale-ELF removal cannot touch a real build.
    #[test]
    fn build_reads_the_generated_map_from_the_repo_root() {
        let s = scratch_root("build-path");
        let root = &s.0;
        let td = root.join("target");
        let b = board("g031k8").unwrap();
        let want = root.join("targets/generated/memory-stm32g031.x");

        let e = build(root, b, "gust_hm_probe", &[], &td).unwrap_err();
        assert!(
            e.starts_with(&format!("read {}:", want.display())),
            "missing map must name {}: {e}",
            want.display()
        );

        fs::create_dir_all(want.parent().unwrap()).unwrap();
        fs::write(&want, b"MEMORY {}").unwrap();
        let e = build(root, b, "gust_hm_probe", &[], &td).unwrap_err();
        assert!(
            e.starts_with("build failed for gust_hm_probe on g031k8"),
            "build() did not get past the map read: {e}"
        );
        assert_eq!(
            fs::read(root.join("os/gust/memory.x")).unwrap(),
            b"ORIGINAL",
            "memory.x not restored"
        );
    }

    #[test]
    fn target_dir_reaches_cargo() {
        let b = board("g031k8").unwrap();
        let c = cargo_build(
            Path::new("/r/os/gust"),
            b,
            "gust_hm_probe",
            "target-g031k8",
            Path::new("/t/gust"),
        );
        let args: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args
            .iter()
            .position(|a| a == "--target-dir")
            .expect("no --target-dir in the cargo args");
        assert_eq!(
            args.get(i + 1).map(String::as_str),
            Some("/t/gust"),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--target", "thumbv6m-none-eabi"]),
            "{args:?}"
        );
        assert_eq!(c.get_current_dir(), Some(Path::new("/r/os/gust")));
    }

    /// A build that produces no image is REFUSED (could not run), not FAIL: exit 1 means
    /// the firmware's own FAIL line. Through run() itself, with a configured local site
    /// and a manifest cargo cannot parse.
    #[test]
    fn build_error_is_refused_not_fail() {
        let s = scratch_root("build-refused");
        let root = &s.0;
        let gen = root.join("targets/generated");
        fs::create_dir_all(&gen).unwrap();
        fs::write(gen.join("memory-stm32g031.x"), b"MEMORY {}").unwrap();
        fs::write(
            gen.join("gust_target_stm32g031.rs"),
            b"pub const RCC_CSR: u32 = 0x4002_1060;\npub const RMVF: u32 = 1 << 23;\n",
        )
        .unwrap();
        let td = root.join("target").to_string_lossy().into_owned();
        let v = run_with_env(
            root,
            "g031k8",
            "gust_hm_probe",
            &[],
            1,
            fixture(&[
                ("GALE_SILICON_G031K8_HOST", "local"),
                ("GALE_SILICON_G031K8_USB_LOCATION", "0-0.0"),
                ("CARGO_TARGET_DIR", td.as_str()),
            ]),
        );
        assert!(
            matches!(&v, Verdict::Refused(r) if r.contains("build failed for gust_hm_probe")),
            "{v:?}"
        );
        assert_eq!(v.code(), EXIT_REFUSED);
    }

    #[test]
    fn gust_target_dir_follows_cargo_target_dir() {
        let g = Path::new("/r/os/gust");
        assert_eq!(gust_target_dir(g, None), PathBuf::from("/r/os/gust/target"));
        assert_eq!(
            gust_target_dir(g, Some("".into())),
            PathBuf::from("/r/os/gust/target")
        );
        assert_eq!(
            gust_target_dir(g, Some("/t/x".into())),
            PathBuf::from("/t/x")
        );
        assert_eq!(
            gust_target_dir(g, Some("rel".into())),
            PathBuf::from("/r/os/gust/rel")
        );
    }

    fn verdict_of(code: Option<i32>, text: &str) -> Verdict {
        firmware_verdict(
            board("g474re").unwrap(),
            "gust_hm_probe",
            code,
            text,
            Path::new("/dev/null"),
        )
    }

    #[test]
    fn exit_zero_without_the_ok_line_is_refused_not_pass() {
        // The firmware (or anything else in the chain) exited 0 and printed no OK line.
        let v = verdict_of(Some(0), "Info : clock speed 2000 kHz\nhm: ran\n");
        assert!(
            matches!(&v, Verdict::Refused(r) if r.contains("gust-hm-probe OK")),
            "{v:?}"
        );
        assert_eq!(v.code(), EXIT_REFUSED);
        // ...and nothing at all.
        assert!(matches!(verdict_of(Some(0), ""), Verdict::Refused(_)));
    }

    #[test]
    fn verdict_comes_from_the_firmware_lines() {
        assert!(matches!(
            verdict_of(Some(0), "gust-hm-probe OK: 6/6"),
            Verdict::Pass(_)
        ));
        // The line, not the status, decides: an OK line under a non-zero status still passes,
        // and a FAIL line wins over everything.
        assert!(matches!(
            verdict_of(Some(1), "gust-hm-probe OK: 6/6"),
            Verdict::Pass(_)
        ));
        assert!(matches!(
            verdict_of(Some(0), "gust-hm-probe FAIL: x\ngust-hm-probe OK"),
            Verdict::Fail(_)
        ));
        // Another bin's OK line is not this bin's.
        assert!(matches!(
            verdict_of(Some(0), "gust-timer-probe OK"),
            Verdict::Refused(_)
        ));
        assert!(matches!(verdict_of(None, ""), Verdict::Refused(_)));
        assert!(matches!(
            verdict_of(Some(1), "hm: ran"),
            Verdict::Refused(_)
        ));
    }

    fn filt(xs: &[&str]) -> Option<Vec<String>> {
        Some(xs.iter().map(|s| s.to_string()).collect())
    }

    /// The exit of the matrix COMMAND for a filter, every runnable pair answered by `v`.
    fn matrix_code(
        boards: Option<Vec<String>>,
        bins: Option<Vec<String>>,
        v: fn() -> Verdict,
    ) -> i32 {
        matrix(boards.as_deref(), bins.as_deref(), false, |_, _| v())
    }

    fn pass() -> Verdict {
        Verdict::Pass(vec![])
    }
    fn refused() -> Verdict {
        Verdict::Refused("x".into())
    }
    fn fail() -> Verdict {
        Verdict::Fail(vec![])
    }

    #[test]
    fn matrix_not_yet_board_is_not_a_pass() {
        assert_eq!(matrix_code(filt(&["esp32c3"]), None, pass), EXIT_NOT_YET);
    }

    #[test]
    fn matrix_not_yet_pair_is_not_a_pass() {
        assert_eq!(
            matrix_code(filt(&["g031k8"]), filt(&["gust_wdg_silicon"]), pass),
            EXIT_NOT_YET
        );
    }

    #[test]
    fn matrix_unknown_board_is_refused_as_usage() {
        assert_eq!(
            unknown_names(filt(&["rt1176"]).as_deref(), None),
            vec!["unknown board 'rt1176'".to_string()]
        );
        assert_eq!(matrix_code(filt(&["rt1176"]), None, pass), EXIT_USAGE);
    }

    #[test]
    fn matrix_unknown_bin_is_refused_as_usage() {
        assert_eq!(matrix_code(None, filt(&["no_such_bin"]), pass), EXIT_USAGE);
        assert_eq!(
            matrix_code(
                filt(&["f100"]),
                filt(&["gust_hm_probe", "no_such_bin"]),
                pass
            ),
            EXIT_USAGE
        );
    }

    #[test]
    fn matrix_with_zero_rows_is_empty_not_pass() {
        // Both names exist; the pair selects nothing (gust_adc_silicon runs only on f100).
        assert_eq!(
            matrix_code(filt(&["wl55jc"]), filt(&["gust_adc_silicon"]), pass),
            EXIT_EMPTY
        );
        assert_eq!(matrix_exit([]), EXIT_EMPTY);
    }

    #[test]
    fn matrix_refused_and_fail_rows_are_not_a_pass() {
        assert_eq!(
            matrix_code(filt(&["f100"]), filt(&["gust_hm_probe"]), refused),
            EXIT_REFUSED
        );
        assert_eq!(
            matrix_code(filt(&["f100"]), filt(&["gust_hm_probe"]), fail),
            EXIT_FAIL
        );
        // FAIL outranks REFUSED outranks NOT-YET.
        assert_eq!(
            matrix_exit(["NOT-YET", "REFUSED", "FAIL", "PASS"]),
            EXIT_FAIL
        );
        assert_eq!(matrix_exit(["NOT-YET", "REFUSED", "PASS"]), EXIT_REFUSED);
        assert_eq!(matrix_exit(["NOT-YET", "PASS"]), EXIT_NOT_YET);
    }

    #[test]
    fn matrix_all_pass_is_the_only_zero() {
        // Positive control: a filter whose every row is runnable, every row PASS -> 0.
        assert_eq!(
            matrix_code(filt(&["f100"]), filt(&["gust_hm_probe"]), pass),
            EXIT_PASS
        );
        let rows = matrix_rows(
            filt(&["f100"]).as_deref(),
            filt(&["gust_hm_probe"]).as_deref(),
            |_, _| pass(),
        );
        assert_eq!(rows.len(), 1);
    }

    /// No verdict line: whatever the exit status, the run is REFUSED. 142 is the local
    /// deadline (SIGALRM), 124 the remote one, 97 no pinned with-device, 3 a busy claim.
    #[test]
    fn no_verdict_line_is_refused_for_every_exit_code() {
        let log = "Info : clock speed 2000 kHz\nhm: ran, said nothing conclusive\n";
        for code in [
            Some(0),
            Some(1),
            Some(2),
            Some(3),
            Some(97),
            Some(124),
            Some(142),
            Some(255),
            None,
        ] {
            let v = verdict_of(code, log);
            assert!(
                matches!(v, Verdict::Refused(_)),
                "exit {code:?} with no verdict line gave {v:?}"
            );
        }
    }

    #[test]
    fn needs_env_unset_is_refused_and_set_may_run() {
        let tt = MATRIX
            .iter()
            .find(|e| e.needs_env.is_some())
            .expect("a needs_env row");
        let (var, _) = tt.needs_env.unwrap();
        let v = needs_env_refusal(tt, "wb55rg", fixture(&[]));
        assert!(
            matches!(&v, Some(Verdict::Refused(r)) if r.contains(var)),
            "{v:?}"
        );
        assert!(needs_env_refusal(tt, "wb55rg", fixture(&[(var, "/x/two_tenant.o")])).is_none());
        let plain = MATRIX.iter().find(|e| e.needs_env.is_none()).unwrap();
        assert!(needs_env_refusal(plain, "f100", fixture(&[])).is_none());
    }

    #[test]
    fn verdict_line_is_a_whole_token() {
        let pass = |t: &str| matches!(verdict_of(Some(0), t), Verdict::Pass(_));
        // Legal forms the firmware prints, including on a later line of the log.
        assert!(pass("gust-hm-probe OK"));
        assert!(pass("gust-hm-probe OK: 6/6"));
        assert!(pass("gust-hm-probe OK(private): faulted"));
        assert!(pass("gust-hm-probe OK\tdone"));
        assert!(pass(
            "Info : target halted\nsemihosting: [hm] gust-hm-probe OK: 6/6\n"
        ));
        assert!(pass(
            "Info : a\nInfo : b\nhm: step 3\ngust-hm-probe OK: 6/6\nInfo : c\n"
        ));
        // Not this bin's verdict.
        for t in [
            "gust-hm-probe OK-ish",
            "gust-hm-probe OKAY",
            "xgust-hm-probe OK",
            "expected 'gust-hm-probe OK' but the firmware hung",
            "gust-hm-probe ok",
            "GUST-HM-PROBE OK",
            "gust-hm-probe  OK",
        ] {
            assert!(
                matches!(verdict_of(Some(0), t), Verdict::Refused(_)),
                "{t:?} was taken as a verdict"
            );
        }
        assert!(matches!(
            verdict_of(Some(1), "gust-hm-probe FAILED_TO_INIT"),
            Verdict::Refused(_)
        ));
        assert!(matches!(
            verdict_of(Some(1), "gust-hm-probe fail: x"),
            Verdict::Refused(_)
        ));
        // A bin whose name is the tail of another's: `silicon-bench OK` is not `bench OK`.
        let bench = |t: &str| {
            firmware_verdict(
                board("f100").unwrap(),
                "bench",
                Some(0),
                t,
                Path::new("/dev/null"),
            )
        };
        assert!(matches!(bench("silicon-bench OK"), Verdict::Refused(_)));
        assert!(matches!(bench("silicon_bench OK"), Verdict::Refused(_)));
        assert!(matches!(bench("bench OK"), Verdict::Pass(_)));
    }
}
