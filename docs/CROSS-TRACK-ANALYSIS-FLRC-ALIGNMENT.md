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

---

## UPDATE: Range-Tests Track Has Fixed All 3 Bugs (2026-07-24)

Commit `9b740aa` from balloon-range-tests implements exactly the fixes predicted above:

### Fix 1: Dynamic Sync Header Search
- LR2021 prepends framing bytes before payload in FIFO
- Sync header 0xA5 0x5A 0x42 0x24 NOT at byte 0
- RX now scans for sync header dynamically, sets gpsOff = foundOffset + 4
- Walk test evidence confirmed: FLRC first bytes ≠ sync header

### Fix 2: App-Layer CRC-16 (CCITT 0x1021)
- TX computes CRC-16 over payload bytes 4-21, writes to bytes 29-30
- RX verifies CRC, logs APP_CRC_FAIL on mismatch
- PHASE_RESULT now reports garbage count alongside crc_err

### Fix 3: RX FIFO Clear Before Re-Arm
- `rfClearRxFifo()` existed but was never called
- Now called after every packet, CRC error, and other IRQ
- Prevents stale data from corrupting next packet read

### Also: GPS range sanity check
- Rejects impossible values (|lat|>90, |lon|>180, sats>50)

---

## Impact on FIPS LR2021 Transport Code (Code Audit)

Audited `crates/microfips-esp-transport/src/` against range-tests fixes.

### What FIPS Already Does Right
- `lr2021_esp_hal.rs`: Uses correct 2-byte opcodes (0x01xx, 0x02xx) — NOT SX1280 1-byte
- `start_rx()`: Clears RX FIFO before entering RX mode (`OP_CLR_RX_FIFO`)
- `send_packet()`: Clears TX FIFO before writing
- CALIBRATE mask = 0x5F (correct, not 0x6F)
- SET_RX_PATH_HF called (mandatory for 2.4 GHz)
- Init sequence follows proven RP2040 raw SPI baseline

### CRITICAL: 3 Gaps in FIPS Code

**Gap 1: `read_packet()` is fully stubbed (BLOCKER)**
`lr2021_esp_hal.rs` lines 316-360: ALL SPI read calls are COMMENTED OUT.
- `GET_RX_BUFFER_STATUS` read: commented
- `READ_RX_FIFO` read: commented
- `GET_PACKET_STATUS` read: commented
- Returns hardcoded `crc_ok: true`, `length: 0`
- Root cause: trait defines `read_packet(&self)` but SPI needs `&mut self`
- Transport layer has NEVER been tested with real radio data

**Gap 2: No app-layer CRC in framing layer**
`lr2021_framing.rs`: RxFramer pushes raw FIFO bytes with NO integrity check.
- Comment says "FrameWriter adds 2-byte LE length prefix" — that's framing, not CRC
- Noise protocol MAC (Poly1305) catches corruption at protocol layer
- BUT: corrupt radio packets waste airtime and cause handshake timeouts
- Range-tests proved hardware CRC passes garbage — must not trust it

**Gap 3: No FIFO clear on CRC error / other IRQ paths**
`lr2021_transport.rs` recv flow: calls `start_rx()` which clears FIFO.
BUT: on CRC_ERROR IRQ or other IRQ sources, transport may not clear FIFO
before re-arming. Range-tests showed this causes stale data corruption.

### Action Items for FIPS Track
1. **Implement `read_packet()`** — fix trait to `&mut self` or use RefCell wrapper
2. **Add CRC-16 (CCITT 0x1021) to framing layer** — TX computes over payload, RX verifies
3. **Clear RX FIFO on ALL IRQ paths** — not just successful RX, also CRC_ERROR and timeout
4. **Test with real hardware** — current code has never received a real packet
