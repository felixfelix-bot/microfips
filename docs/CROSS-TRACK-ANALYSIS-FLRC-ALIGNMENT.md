# Cross-Track Analysis: FLRC Byte Alignment + GPS Payload Verification

**Source:** balloon-range-tests (commits b182b81, be354b0)
**Date:** 2026-07-24
**Tags:** RADIO, FIRMWARE, TEST

> Independent analysis of range-tests walk test data. No coordination with source track.

## Summary

Two findings from outdoor walk test captures on LR2021 modules:

1. **FLRC packets received but CRC/byte alignment off** — at FLRC-1300 kbps and below, packets arrive but GPS struct fields decode as garbage despite `crc_err=0`.
2. **GPS payload verified on LoRa phases** — LoRa SF12 (sub-GHz) decodes perfect GPS telemetry (lat, lon, sats, fix, utc), confirming the 24-byte telemetry struct is correct.

## Key Data Points

### FLRC-2600 (2.4 GHz) — WORKS
- Phase 10 (LF-FLRC-2600): rx=178, unique=178, PER=11%, RSSI=-42 dBm, CRC=0
- GPS payload valid: lat=32.63914, lon=-16.94652, sats=9, fix=1
- Raw32 header: `0xA5 0x5A 0x42 0x24` (sync word pattern)

### FLRC-1300 and below — BROKEN
- Phase 11 (LF-FLRC-1300): rx=177, unique=2, PER=99% — only first 2 packets valid
- After 2-3 packets: gps_utc jumps to 1666794497 (garbage), lat=195.18 (invalid), sats=2549
- Phase 12 (LF-FLRC-650): unique=0 — total alignment loss
- Phase 13 (LF-FLRC-325): unique=0
- HF-FLRC phases (3,4): all garbage, 0 unique despite rx=29 packets

### LoRa SF12 (sub-GHz 915 MHz) — PERFECT
- Phase 9: rx=19, unique=19, PER=5%, RSSI=-23 dBm
- GPS payload: lat=32.63919, lon=-16.94653, sats=8, fix=1 — rock solid

## Root Cause Analysis

### The Problem Is NOT Radio/RF
Packets ARE received at all FLRC rates with good RSSI (-42 to -58 dBm). The issue is **byte framing alignment** in the RX firmware — the telemetry struct parser loses sync with the byte stream.

### Pattern
- First 1-3 packets after entering RX mode decode correctly (sync word aligned)
- Subsequent packets drift: the parser reads mid-frame, producing garbage lat/lon/sats
- This gets worse at lower FLRC bitrates (1300→650→325)

### CRC False Positive
`crc_err=0` across ALL phases, including those with 100% garbage payloads. Two possibilities:
1. CRC is computed/checked on header only, not the full payload
2. CRC check has a bug (off-by-one, wrong polynomial, wrong byte range)
Either way: **silently corrupt packets pass CRC**. Critical for mesh integrity.

## Impact on FIPS Mesh Transport

### What Works for Mesh
- **FLRC-2600 kbps**: reliable high-throughput (178 pkts, 11% PER at ~10m outdoor). This is the mesh data plane.
- **LoRa SF12**: reliable fallback for long-range control/gossip. 5% PER at 10m, scales to km.

### What Needs Fixing Before Mesh Uses It
1. **FLRC framing sync recovery** — RX must re-sync on packet boundaries, not just first packet. Add explicit sync word search or use packet length header.
2. **CRC coverage** — verify CRC covers full payload bytes (lat+lon+sats+fix+utc), not just header. Mesh cannot tolerate silent data corruption.
3. **FLRC-1300 and below**: avoid until framing bug fixed. If lower bitrate needed for range, use LoRa instead.

### Recommendation for FIPS Transport Layer
- Default mesh data plane: FLRC-2600 (2.4 GHz)
- Long-range fallback: LoRa SF12 (sub-GHz)
- Do NOT use FLRC-1300/650/325 until byte alignment is fixed
- Add CRC-16 validation of full payload before accepting mesh packets — do not trust radio-level CRC alone
