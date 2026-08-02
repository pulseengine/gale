// gust:os/taskdisp — the APP side of the seam.
//
// The OS does not contain the tasks; it dispatches into them. The composite imports this,
// which is why `taskdisp` shows up as a residual import next to the hardware seam.
export const dispatched = [];         // the ids the OS actually dispatched, in order

const polls = new Map();

export function pollTask(id) {
  dispatched.push(id);
  const n = (polls.get(id) ?? 0) + 1;
  polls.set(id, n);
  return n >= 3 ? 1 : 0;              // done on the 3rd poll — same as the wasmtime host
}
