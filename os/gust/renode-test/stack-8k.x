/* Renode 8 KB-SRAM targets (gust-renode, gust-f100-renode): both map 8 KB of SRAM at
 * 0x20000000, but os/gust/memory.x is the qemu lm3s map with 64 KB of RAM, so the
 * default stack top 0x20010000 lies outside their SRAM and the image faults within 8
 * instructions. `cargo xtask renode-elfs` links gust_wasm.elf with this script.
 * Checked: a stack top below the statics fails the link with the message below. */
_stack_start = 0x20002000;
ASSERT(_stack_start >= __euninit + 1024, "gust_wasm: .data+.bss plus a 1 KiB stack do not fit the 8 KB SRAM");
