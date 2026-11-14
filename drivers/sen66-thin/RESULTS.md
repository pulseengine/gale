# sen66-thin: the SEN66 air-quality sensor over gust:bus/i2c-master

Device driver for wohl's air node (gale#511, seam `gale-wohl/air-sensor-read`).
This stage holds only the pure core:
- the CRC-8;
- word checking and framing;
- the command table;
- which raw values mean "unknown".

The exported interface waits for the #511 decision on how a reading is represented.

Facts come from the SEN6x datasheet, "D1 Version 1.0 – October 2026"
(sha256 34b8e5b69c3686c76955abe7781b52841aaff7bd67beed012c4655b4566bbeeb). Every
constant in `src/lib.rs` names the section or table it comes from.

## Measured (local, 2026-11-14)

- **Unit tests: 3 of 3 pass.** They cover the datasheet CRC example, the frame/check round trip, and unknown values by signedness.
- **Kani 0.67.0: 5 of 5 harnesses verified.**

| Harness | What it checks |
|---|---|
| c1 | CRC(0xBEEF) = 0x92, the datasheet example |
| c2 | every single-bit error in the 24-bit received word is rejected |
| c3 | every double-bit error is rejected |
| c4 | `check_word` accepts exactly the framed words |
| c5 | field decoding is total over 0..9, and each field has exactly one unknown value matching Table 34 |

- **Negative controls, each caught (exit 1):**

| Planted fault | Caught by |
|---|---|
| polynomial 0x07 | c1 |
| init 0x00 | c1 |
| CRC over the low byte only | c2 |
| RH treated as uint16 | c5 |

## What is not shown

- Nothing talks to the bus yet. There is no read sequence and no data-ready poll.
- Nothing has run against a real SEN66, Renode or silicon.
- c2 and c3 are about the code over all 2^16 words. They say nothing about error patterns of three or more bits.
- The device-status bit definitions are in a separate Sensirion document, which has not been extracted.
