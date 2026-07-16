/* iso_contain.x — segment-placement fragment for the gust_iso_contain_* probes
 * (linked IN ADDITION to cortex-m-rt's link.x via a per-bin -T from build.rs).
 *
 * The dissolved synth object carries __synth_wasm_seg_0/1/2 in ONE .data
 * section (offsets +0x00/+0x0c/+0x18, 0x34 bytes total, __synth_globals at
 * +0x30), so seg_0 cannot be split out and placed alone. The honest
 * alternative documented in the probe: build.rs objcopy-renames that .data to
 * .iso_stale_data and THIS script pins it at 0x2000_BFF0, deliberately
 * straddling the probe's MPU guard boundary at 0x2000_C000:
 *
 *     denied guard   [0x2000_8000, 0x2000_C000)   granted stack window
 *   ...------------------------------+--------------------------------...
 *        seg_0 = 0x2000_BFF0         |   seg_2 = 0x2000_C008
 *        stale read target seg_0+8   |   correct string  seg_2+8
 *          = [0x2000_BFF8,0x2000_C000)  = 0x2000_C010.."gust:os up\n"
 *                DENIED              |         GRANTED
 *
 * so synth#757's miscompiled head-chunk read (the R_ARM_ABS32 at .text+0x694
 * bound to seg_0 instead of seg_2, addend +8) lands in MPU-denied SRAM, while
 * every address the CORRECT program needs (code, seg_2's payload,
 * __synth_globals at +0x30 = 0x2000_C020, arena .bss, stack) is granted.
 *
 * The section lives in RAM but loads from FLASH; cortex-m-rt only initialises
 * its own .data, so the probe copies [lma..lma+size) to 0x2000_BFF0 itself,
 * BEFORE the MPU is switched on. */
SECTIONS
{
  .iso_stale_data 0x2000BFF0 :
  {
    __iso_stale_data_start = .;
    KEEP(*(.iso_stale_data));
    __iso_stale_data_end = .;
  } > RAM AT > FLASH
  __iso_stale_data_lma = LOADADDR(.iso_stale_data);
} INSERT AFTER .uninit;
