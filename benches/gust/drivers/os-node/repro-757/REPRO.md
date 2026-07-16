# synth#757 — exact reproduction input (the module the reconstructions were missing)

`loom.wasm` (md5 `18da000d9142dfa0885f57578d3af150`, 3730 B) is the EXACT synth input that
miscompiles — the meld-fused + loom-inlined `gust:os {time, log}` node. The maintainer's 7
RawVec-grow+memmove reconstructions were all green (synth PR #772), so this is the module
they need.

## One command (synth 0.45.0, deterministic)

    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi --shadow-stack-size 2048 -o os-tl.o
    # dissolves clean: 13 functions, 0 skipped, only read32/write32 undefined

## The wrong result

The node's `run` builds `"gust:os up\n"` (11 bytes) into a `Vec<u8>` and the provider writes
each byte to an mmio sink. Captured on the sink at runtime (qemu cortex-m3):

    got      = [2, 0, 0, 0, 1, 0, 0, 32, 117, 112, 10]
    expected = [103,117,115,116,58,111,115, 32, 117, 112, 10]   ("gust:os up\n")

Bytes 7–10 (`" up\n"`) correct; bytes 0–6 are the LOW-offset `.data` constants leaking in
(see `os-tl-0.45.data`). The copy's source address is wrong for the head chunk only.

## Isolation (holds across 0.43.0 / 0.43.1 / 0.44.0 / 0.45.0 — byte-identical)

- Pre-synth wasm is CORRECT under wasmtime (both `fused.wasm` and this `loom.wasm` emit
  `"gust:os up\n"`). Only synth's ARM object is wrong.
- `.data` holds the correct string (`os-tl-0.45.data`); the CODE that copies it is wrong
  (traced to the inlined RawVec-grow/memmove path, `func_17`, reached `run → func_20 →
  func_16 → func_17`).
- r11=0 trampoline ruled out (failure identical with/without).

`os-tl-0.45.disasm` is the full `objdump -dr` of the miscompiled object. Happy to pair on it.
