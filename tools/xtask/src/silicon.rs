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
//! that says nothing is not a run that passed.
//!
//! The board table holds only PUBLIC board metadata: the part, its target triple, the
//! openocd interface/target scripts, the claim name, and HOW its probe is pinned. Where a
//! board is plugged in — the probe host, and the USB port path that pins it there — is
//! private bench topology and is read at run time from the environment (see
//! [`resolve`] and benches/gust/silicon/README.md). A board with no configured host is
//! REFUSED with the variable to set, never run against a default. Memory maps and
//! register addresses are NOT here either: they come from the measured target models
//! (benches/gust/targets/*.aadl -> generated/).

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
    /// Stem of benches/gust/targets/generated/{memory-,gust_target_}<stem>.{x,rs}.
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
             this board's probe is plugged into (benches/gust/silicon/README.md)", b.name)),
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
    // --- module COMBINATIONS: composed/fused images, verdict by semihosting exit ---
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
        needs_env: Some(("GUST_TWO_TENANT_O", "benches/gust/drivers/run-two-tenant.sh (pinned synth) → /tmp/gale-two-tenant/two_tenant.o")),
        boards: &["wb55rg"],
        not_on: &[("g474re", "96 KiB SRAM cannot hold two 64 KiB wasm pages plus a stack (needs synth's used-extent symbol, v0.69)"),
                  ("wl55jc", "64 KiB SRAM cannot hold two 64 KiB wasm pages"),
                  ("g031k8", "8 KiB SRAM, and ARMv7-M object (synth#1301)"),
                  ("f100", "no MPU")],
    },
    Entry {
        bin: "gust_two_tenant",
        extra: &["no-regions"],
        needs_env: Some(("GUST_TWO_TENANT_O", "benches/gust/drivers/run-two-tenant.sh (pinned synth) → /tmp/gale-two-tenant/two_tenant.o")),
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

/// Restores benches/gust/memory.x however the build ends.
struct MemoryGuard {
    path: PathBuf,
    original: Vec<u8>,
}
impl Drop for MemoryGuard {
    fn drop(&mut self) {
        let _ = fs::write(&self.path, &self.original);
    }
}

fn build(root: &Path, b: &Board, bin: &str, extra: &[&str]) -> Result<PathBuf, String> {
    let gust = root.join("benches/gust");
    let mem = gust.join("memory.x");
    let gen = gust.join(format!("targets/generated/memory-{}.x", b.stem));
    let original = fs::read(&mem).map_err(|e| format!("read {}: {e}", mem.display()))?;
    let generated = fs::read(&gen).map_err(|e| format!("read {}: {e}", gen.display()))?;
    let _guard = MemoryGuard { path: mem.clone(), original };
    fs::write(&mem, generated).map_err(|e| format!("write memory.x: {e}"))?;

    // Force a relink: memory.x is not a cargo input, so a stale ELF linked against the
    // PREVIOUS board's map would otherwise be reused silently.
    let elf = gust.join(format!("target/{}/release/{bin}", b.triple));
    let _ = fs::remove_file(&elf);

    let mut feats: Vec<&str> = b.features.to_vec();
    feats.extend_from_slice(extra);
    let out = Command::new("cargo")
        .current_dir(&gust)
        .args(["build", "--release", "--bin", bin, "--no-default-features", "--target", b.triple])
        .args(["--features", &feats.join(",")])
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
    let Some(b) = board(board_name) else {
        return Verdict::Refused(format!("no board '{board_name}' (see `xtask silicon list`)"));
    };
    if let Some(why) = b.not_yet {
        return Verdict::Refused(format!("{}: not yet — {why}", b.name));
    }
    // Before any build: a board with no configured site is REFUSED, not built for nothing.
    let site = match resolve(b, |k| std::env::var(k).ok()) {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(e),
    };
    let consts = match fs::read_to_string(root.join(format!("benches/gust/targets/generated/gust_target_{}.rs", b.stem))) {
        Ok(t) => t,
        Err(e) => return Verdict::Refused(format!("generated target module for {}: {e}", b.stem)),
    };
    let (Some(csr), Some(rmvf)) = (generated_u32(&consts, "RCC_CSR"), generated_u32(&consts, "RMVF")) else {
        return Verdict::Refused(format!("RCC_CSR/RMVF missing from gust_target_{}.rs", b.stem));
    };

    let elf = match build(root, b, bin, extra) {
        Ok(p) => p,
        Err(e) => return Verdict::Fail(vec![e]),
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

    let log = root.join(format!("benches/gust/target/silicon-logs/{}-{bin}{}.log", b.name,
        if extra.is_empty() { String::new() } else { format!("-{}", extra.join("-")) }));
    let _ = fs::create_dir_all(log.parent().unwrap());
    let _ = fs::write(&log, &text);

    let prefix = bin.replace('_', "-");
    let find = |tag: &str| text.lines().find_map(|l| l.find(&format!("{prefix} {tag}")).map(|i| l[i..].to_string()));
    let mut detail = vec![format!("{} ({}): exit {:?}, log {}", b.name, b.what, code, log.display())];
    if let Some(l) = find("FAIL") {
        detail.push(l);
        return Verdict::Fail(detail);
    }
    if let Some(l) = find("OK") {
        detail.push(l);
        return Verdict::Pass(detail);
    }
    // No marker line: fall back to the firmware's own semihosting EXIT status. openocd
    // is given no `shutdown`, so it exits only through the firmware's SYS_EXIT (with that
    // status), a failed command (1), or the deadline (124 remote / 142 local). So 0 can
    // only be EXIT_SUCCESS. 1 is ambiguous — firmware EXIT_FAILURE or an openocd error —
    // and is a FAIL only when the firmware demonstrably ran, i.e. printed something.
    let firmware_spoke = text.lines().any(|l| {
        let l = l.trim();
        !l.is_empty()
            && !["Info", "Warn", "Error", "Debug", "Open On-Chip", "Licensed", "For bug", "http",
                 "**", "[", "xPSR", "semihosting is", "adapter", "none separate", "srst_only",
                 "hla_vid_pid", "DEPRECATED", "with-device", "gdb_port", "Consider", "shutdown"]
                .iter().any(|p| l.starts_with(p))
    });
    if code == Some(0) {
        detail.push("firmware exited EXIT_SUCCESS (semihosting SYS_EXIT 0)".into());
        return Verdict::Pass(detail);
    }
    // Exit 1 is NOT treated as a firmware FAIL, even when the firmware printed: the first
    // matrix run labelled gust_stack on the G474 "FAIL" when it had HUNG — with-device
    // reports a deadline-killed child as 1. A firmware failure must say so in a line.
    let _ = firmware_spoke;
    match code {
        Some(3) => Verdict::Refused(format!("{}: probe '{}' is claimed by someone else", b.name, b.claim)),
        Some(124) => Verdict::Refused(format!("{}: deadline passed with no verdict — see {}", b.name, log.display())),
        Some(97) => Verdict::Refused(format!("{}: no pinned with-device on the probe host", b.name)),
        None => Verdict::Refused(format!("{}: no verdict line before the deadline — see {}", b.name, log.display())),
        _ => Verdict::Refused(format!("{}: firmware printed no verdict (exit {:?}) — see {}", b.name, code, log.display())),
    }
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
       An unset variable REFUSES the board and names the variable. See benches/gust/silicon/README.md.\n\n\
     EXIT CODES: 0 pass, 1 fail, 2 usage, 3 could not run (busy probe, no verdict, unconfigured board) — NOT a pass\n"
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
        Some("matrix") => {
            let mut rows: Vec<(String, String, &'static str, String)> = Vec::new();
            let (mut fails, mut refused) = (0, 0);
            for e in MATRIX {
                if bins_filter.as_ref().is_some_and(|f| !f.iter().any(|x| x == e.bin)) { continue; }
                let label = if e.extra.is_empty() { e.bin.to_string() } else { format!("{} +{}", e.bin, e.extra.join("+")) };
                for bd in e.boards {
                    if boards_filter.as_ref().is_some_and(|f| !f.iter().any(|x| x == bd)) { continue; }
                    let v = match e.needs_env {
                        Some((var, how)) if std::env::var_os(var).is_none() =>
                            Verdict::Refused(format!("{bd}: {var} is not set — produce it with {how}")),
                        _ => run(root, bd, e.bin, e.extra, timeout_s),
                    };
                    let (tag, why) = match &v {
                        Verdict::Pass(d) => ("PASS", d.last().cloned().unwrap_or_default()),
                        Verdict::Fail(d) => { fails += 1; ("FAIL", d.last().cloned().unwrap_or_default()) }
                        Verdict::Refused(r) => { refused += 1; ("REFUSED", r.clone()) }
                    };
                    eprintln!("{tag:<8} {bd:<8} {label}");
                    rows.push((bd.to_string(), label.clone(), tag, why));
                }
                for (bd, why) in e.not_on {
                    if boards_filter.as_ref().is_some_and(|f| !f.iter().any(|x| x == bd)) { continue; }
                    rows.push((bd.to_string(), label.clone(), "NOT-YET", why.to_string()));
                }
            }
            for b in BOARDS.iter().filter(|b| b.not_yet.is_some()) {
                if boards_filter.as_ref().is_some_and(|f| !f.iter().any(|x| x == b.name)) { continue; }
                rows.push((b.name.to_string(), "(all)".into(), "NOT-YET", b.not_yet.unwrap().to_string()));
            }
            if json {
                let items: Vec<String> = rows.iter().map(|(b, bin, t, w)| format!(
                    "{{\"board\":\"{}\",\"bin\":\"{}\",\"result\":\"{}\",\"detail\":\"{}\"}}",
                    b, bin, t, w.replace('\\', "\\\\").replace('"', "\\\""))).collect();
                println!("[{}]", items.join(","));
            } else {
                println!("{:<8} {:<34} {:<8} detail", "board", "firmware", "result");
                for (b, bin, t, w) in &rows {
                    println!("{b:<8} {bin:<34} {t:<8} {}", w.chars().take(140).collect::<String>());
                }
            }
            if fails > 0 { EXIT_FAIL } else if refused > 0 { EXIT_REFUSED } else { EXIT_PASS }
        }
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
}
