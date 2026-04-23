#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
#
# Minimal Zephyr CTF 1.8 parser for the engine-control bench.
#
# The Zephyr-upstream scripts/tracing/parse_ctf.py uses babeltrace2's
# Python bindings (`bt2`). Those aren't installable without root in the
# Gale dev container, and babeltrace2 itself is not packaged with the
# Zephyr SDK. This file is a narrow substitute: it parses the binary CTF
# stream that CONFIG_TRACING_BACKEND_UART emits on qemu_cortex_m3 UART1,
# decoding the kernel events the bench actually exercises (ISR, sem_give,
# sem_take, timer, thread switch). It is NOT a full CTF 1.8 implementation
# — unknown event IDs are skipped after the header (6 bytes) is consumed;
# since Zephyr events are variable-length, a single unknown ID that
# appears before any known ID will desync the stream. The event table
# below covers every kernel-object event Gale shims + Zephyr core emit
# in the engine_control ISR path as of zephyr/subsys/tracing/ctf/tsdl/
# metadata, commit [check `git -C zephyr log -1 --format=%H -- subsys/tracing/ctf/tsdl/metadata`].
#
# Usage:
#   ./parse_ctf_minimal.py <channel0_0 file>
#   ./parse_ctf_minimal.py --histogram <channel0_0 file>
#   ./parse_ctf_minimal.py --per-isr <channel0_0 file>
#
# Output formats:
#   default:     one line per event, "<ts_ns>\t<name>\t<payload>"
#   --histogram: count by event name + total bytes
#   --per-isr:   count events between isr_enter/isr_exit pairs
#
# Byte order is little-endian (per metadata `byte_order = le`). Event
# header is 4-byte u32 timestamp (ns, from k_cyc_to_ns_floor64) + 2-byte
# u16 id. All integer fields are 8-byte aligned but actually packed
# tight by Zephyr's CTF_INTERNAL_FIELD_APPEND — so struct.unpack with
# "<" alignment works directly. ctf_bounded_string_t is fixed 20-byte ASCII.

import argparse
import collections
import struct
import sys

# Event schemas. None => no payload. "skip_strN" = skip N strings of 20 bytes.
# Field types: I=uint32, H=uint16, B=uint8, b=int8, i=int32, Q=uint64,
# S20=20-byte ctf_bounded_string.
EVENTS = {
    0x10: ("thread_switched_out",    ["I:thread_id", "S20:name"]),
    0x11: ("thread_switched_in",     ["I:thread_id", "S20:name"]),
    0x12: ("thread_priority_set",    ["I:thread_id", "S20:name", "b:prio"]),
    0x13: ("thread_create",          ["I:thread_id", "S20:name"]),
    0x14: ("thread_abort",           ["I:thread_id", "S20:name"]),
    0x15: ("thread_suspend",         ["I:thread_id", "S20:name"]),
    0x16: ("thread_resume",          ["I:thread_id", "S20:name"]),
    0x17: ("thread_ready",           ["I:thread_id", "S20:name"]),
    0x18: ("thread_pending",         ["I:thread_id", "S20:name"]),
    0x19: ("thread_info",            ["I:thread_id", "S20:name",
                                       "I:stack_base", "I:stack_size"]),
    0x1A: ("thread_name_set",        ["I:thread_id", "S20:name"]),
    0x1B: ("isr_enter",              []),
    0x1C: ("isr_exit",               []),
    0x1D: ("isr_exit_to_scheduler",  []),
    0x1E: ("idle",                   []),
    0x21: ("semaphore_init",         ["I:id", "i:ret"]),
    0x22: ("semaphore_give_enter",   ["I:id"]),
    0x23: ("semaphore_give_exit",    ["I:id"]),
    0x24: ("semaphore_take_enter",   ["I:id", "I:timeout"]),
    0x25: ("semaphore_take_blocking",["I:id", "I:timeout"]),
    0x26: ("semaphore_take_exit",    ["I:id", "I:timeout", "i:ret"]),
    0x27: ("semaphore_reset",        ["I:id"]),
    0x28: ("mutex_init",             ["I:id", "i:ret"]),
    0x29: ("mutex_lock_enter",       ["I:id", "I:timeout"]),
    0x2A: ("mutex_lock_blocking",    ["I:id", "I:timeout"]),
    0x2B: ("mutex_lock_exit",        ["I:id", "I:timeout", "i:ret"]),
    0x2C: ("mutex_unlock_enter",     ["I:id"]),
    0x2D: ("mutex_unlock_exit",      ["I:id", "i:ret"]),
    0x2E: ("timer_init",             ["I:id"]),
    0x2F: ("timer_start",            ["I:id", "I:duration", "I:period"]),
    0x30: ("timer_stop",             ["I:id"]),
    0x34: ("thread_user_mode_enter", ["I:thread_id", "S20:name"]),
    0x35: ("thread_wakeup",          ["I:thread_id", "S20:name"]),
    0x7F: ("k_sleep_enter",          ["I:timeout"]),
    0x80: ("k_sleep_exit",           ["I:timeout", "i:ret"]),
}

HDR = struct.Struct("<IH")  # u32 timestamp_ns, u16 id

def decode_field(buf, off, spec):
    kind, _, name = spec.partition(":")
    if kind == "I":
        v = struct.unpack_from("<I", buf, off)[0]; return (name, v, off + 4)
    if kind == "i":
        v = struct.unpack_from("<i", buf, off)[0]; return (name, v, off + 4)
    if kind == "H":
        v = struct.unpack_from("<H", buf, off)[0]; return (name, v, off + 2)
    if kind == "B":
        v = struct.unpack_from("<B", buf, off)[0]; return (name, v, off + 1)
    if kind == "b":
        v = struct.unpack_from("<b", buf, off)[0]; return (name, v, off + 1)
    if kind == "Q":
        v = struct.unpack_from("<Q", buf, off)[0]; return (name, v, off + 8)
    if kind.startswith("S"):
        n = int(kind[1:])
        s = buf[off:off + n]
        nul = s.find(b"\0")
        s = s[:nul] if nul >= 0 else s
        try:
            return (name, s.decode("ascii", errors="replace"), off + n)
        except UnicodeDecodeError:
            return (name, s.hex(), off + n)
    raise ValueError(f"unknown field kind {kind}")

def parse(buf):
    off = 0
    n = len(buf)
    while off + HDR.size <= n:
        ts, eid = HDR.unpack_from(buf, off)
        off += HDR.size
        ev = EVENTS.get(eid)
        if ev is None:
            # Unknown ID; we cannot know the payload length. Bail with a
            # marker so the operator knows the stream desynced.
            yield (ts, eid, "UNKNOWN", {"_desync_at_byte": off - HDR.size})
            # Skip one byte and try to resync. Not ideal but keeps us going.
            continue
        name, fields = ev
        payload = {}
        for spec in fields:
            try:
                k, v, off = decode_field(buf, off, spec)
            except struct.error:
                yield (ts, eid, name, {"_truncated": True})
                return
            payload[k] = v
        yield (ts, eid, name, payload)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("stream", help="CTF raw binary (channel0_0)")
    ap.add_argument("--histogram", action="store_true",
                    help="event-name counts only")
    ap.add_argument("--per-isr", action="store_true",
                    help="events grouped per isr_enter..isr_exit pair")
    ap.add_argument("--limit", type=int, default=0,
                    help="stop after N events (0 = all)")
    args = ap.parse_args()

    with open(args.stream, "rb") as f:
        buf = f.read()
    print(f"# parsed {len(buf)} bytes from {args.stream}", file=sys.stderr)

    if args.histogram:
        counts = collections.Counter()
        for ts, eid, name, p in parse(buf):
            counts[name] += 1
        width = max(len(n) for n in counts) if counts else 0
        total = sum(counts.values())
        print(f"# total events: {total}")
        for name, c in counts.most_common():
            print(f"{name:<{width}}  {c}")
        return

    if args.per_isr:
        # Count events between isr_enter and matching isr_exit.
        groups = []
        current = None
        for ts, eid, name, p in parse(buf):
            if name == "isr_enter":
                current = {"ts_in": ts, "events": collections.Counter(),
                           "total": 0}
            elif name == "isr_exit" and current is not None:
                current["ts_out"] = ts
                current["duration_ns"] = ts - current["ts_in"]
                groups.append(current)
                current = None
            elif current is not None:
                current["events"][name] += 1
                current["total"] += 1
        print(f"# {len(groups)} ISR invocations recorded")
        if not groups:
            return
        # Summary: events per ISR + total
        totals = [g["total"] for g in groups]
        durations = [g["duration_ns"] for g in groups if "duration_ns" in g]
        print(f"events_per_isr min/mean/max = "
              f"{min(totals)}/{sum(totals)//len(totals)}/{max(totals)}")
        if durations:
            print(f"isr_duration_ns min/mean/max = "
                  f"{min(durations)}/{sum(durations)//len(durations)}/{max(durations)}")
        # Aggregate event types across all ISRs.
        agg = collections.Counter()
        for g in groups:
            agg.update(g["events"])
        print("# per-event totals within ISRs:")
        for k, v in agg.most_common():
            print(f"  {k}: {v}  (avg {v/len(groups):.2f}/isr)")
        return

    n = 0
    for ts, eid, name, p in parse(buf):
        pstr = ", ".join(f"{k}={v}" for k, v in p.items())
        print(f"{ts}\t{name}\t{pstr}")
        n += 1
        if args.limit and n >= args.limit:
            break

if __name__ == "__main__":
    main()
