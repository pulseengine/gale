/* NUCLEO-G474RE — STM32G474RET6: Cortex-M4F, 512 KB flash @ 0x08000000,
 * 128 KB SRAM (we map a safe 96 KB of contiguous SRAM1/2 @ 0x20000000;
 * the bench needs <1 KB). The dissolved gust_mix is cortex-m3 and runs here
 * unmodified (thumbv7m ⊂ thumbv7em). */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 512K
  RAM   : ORIGIN = 0x20000000, LENGTH = 96K
}
