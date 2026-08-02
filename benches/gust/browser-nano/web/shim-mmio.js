// gust:hal/mmio — the browser's "tires".
//
// The component asks the world for exactly two things: read a 32-bit register, write one.
// On silicon these are real MMIO accesses. Here they are an array. Nothing else about the
// component changes — that substitution IS the thesis.
const REGS = new Uint32Array(64);
const TIM2_CNT = 0x40000024;          // the one clock register the OS reads

let clock = 4000;
export const reads = [];              // every read, so the page can prove there is one clock

export function setClock(v) { clock = v >>> 0; }

export function read32(addr) {
  const a = addr >>> 0;
  const val = a === TIM2_CNT ? clock : REGS[(a >>> 2) & 63];
  reads.push({ addr: a, val });
  return val;
}

export function write32(addr, val) {
  REGS[((addr >>> 0) >>> 2) & 63] = val >>> 0;
}
