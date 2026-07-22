# FIPS Mesh Transport Design — 2W LR2021 Module Assumption

**Status**: Approved by operator (2026-07-22)
**Date**: 2026-07-22
**Module**: NiceRF LoRa2021F33-2G4 (2W PA, +33 dBm output)
**Applies to**: Balloon node + ground station (both ends)

## 1. Module Change Summary

The project switches from the baseline LR2021 module (+12 dBm) to the NiceRF
LoRa2021F33-2G4, which integrates a 2W power amplifier delivering +33 dBm
output. This affects only the RF front-end — the SPI interface, register set,
FLRC/LoRa modulation, and framing logic are identical to the baseline chip.

**What changes:**
- TX output power: +12 dBm → +33 dBm (+21 dB per end)
- Link budget: +21 dB × 2 ends = +42 dB total system gain improvement
- Expected range: multiplies by the Friis margin

**What does NOT change:**
- EspHalLr2021Radio SPI driver design (same SPI interface, same register map)
- LR2021 FLRC framing module (TxFramer/RxFramer — modulation-agnostic)
- Lr2021Transport adapter (Transport trait implementation)
- FIPS protocol layer (Noise IK, FMP, FSP — completely radio-agnostic)
- Pin mapping (MOSI=7, MISO=2, SCLK=6, CS=10, RESET=3, BUSY=4, DIO9=5)
- SPI clock, timing, errata — all chip-level, not PA-level

## 2. Link Budget Recalculation

### 2.1 System Parameters

| Parameter | Baseline (+12 dBm) | 2W Module (+33 dBm) |
|-----------|--------------------|--------------------|
| TX power | +12 dBm | +33 dBm |
| Frequency | 2440 MHz (FLRC) | 2440 MHz (FLRC) |
| Modulation | FLRC 2600 kbps | FLRC 2600 kbps |
| TX antenna | PCB/on-board | PCB/on-board (module) |
| RX sensitivity (FLRC 2600 kbps) | ~-108 dBm (from RadioLib LR2021 datasheet) | ~-108 dBm (same chip) |
| RX sensitivity (LoRa, SF12, 125kHz) | ~-137 dBm (from LR2021 datasheet) | ~-137 dBm (same chip) |
| Implementation margin | 10 dB | 10 dB |
| Fade margin | 10 dB | 10 dB |

### 2.2 FLRC Mode Link Budget

**Baseline (existing data from balloon-range-tests):**
- TX: +12 dBm, RX sensitivity: -108 dBm
- Available margin: 12 - (-108) = 120 dB
- Subtracted margins (impl + fade + path): 20 dB
- Path loss budget: 120 - 20 = 100 dB
- Free-space path loss at 2440 MHz: FSPL(dB) = 40 + 20·log₁₀(d_km)
- Max distance: 100 = 40 + 20·log₁₀(d) → d = 10^(60/20) = 1000 m = 1 km
- Measured result: <1 km at 0 dBm in NLOS, <30m at low power with obstructions
- Proven sustained: 1377 kbps at 0% loss, bench/short-range

**2W Module (+33 dBm):**
- TX: +33 dBm, RX sensitivity: -108 dBm
- Available margin: 33 - (-108) = 141 dB
- Subtracted margins (impl + fade + path): 20 dB
- Path loss budget: 141 - 20 = 121 dB
- Max distance: 121 = 40 + 20·log₁₀(d) → d = 10^(81/20) = 10^4.05 ≈ 11,200 m ≈ 11 km
- **Expected FLRC range (free-space, line-of-sight): 10-20 km**
- Conservative estimate with real-world fade (15 dB extra): 10^(66/20) ≈ 2 km minimum guaranteed, 10-20 km typical LOS

### 2.3 LoRa Mode Link Budget (Fallback)

For maximum range, the module can switch to LoRa modulation (lower bitrate, higher sensitivity):

- TX: +33 dBm, RX sensitivity: -137 dBm (SF12, 125 kHz, ~100 bps)
- Available margin: 33 - (-137) = 170 dB
- Subtracted margins: 20 dB
- Path loss budget: 170 - 20 = 150 dB
- Max distance: 150 = 40 + 20·log₁₀(d) → d = 10^(110/20) = 10^5.5 ≈ 316 km
- **Expected LoRa range (free-space, line-of-sight): 100+ km** (conservative with fade)
- At balloon altitude (30+ km), near-vertical links have minimal terrain obstruction

### 2.4 Summary Table

| Mode | Bitrate | Baseline Range | 2W Range | Use Case |
|------|---------|----------------|-----------|----------|
| FLRC 2600 kbps | 2.6 Mbps | ~1 km LOS | 10-20 km LOS | High-speed telemetry, FIPS handshake |
| FLRC 1300 kbps | 1.3 Mbps | ~1.5 km | 15-30 km | Balanced speed/range |
| FLRC 650 kbps | 650 kbps | ~2 km | 20-40 km | Robust telemetry |
| LoRa SF12 | ~100 bps | ~10 km | 100+ km | Emergency beacon, minimal data |

## 3. FIPS Protocol Fit at 2W

### 3.1 Noise IK Handshake

The Noise IK handshake MSG1 is 114 bytes (prefix + sender_idx + Noise payload).
This fits comfortably in a single FLRC packet at any bitrate:

| FLRC Bitrate | Max Payload | MSG1 (114B) | MSG2 (69B) | Heartbeat (32B) |
|-------------|-------------|-------------|------------|-----------------|
| 2600 kbps | 255 bytes | 1 packet | 1 packet | 1 packet |
| 1300 kbps | 255 bytes | 1 packet | 1 packet | 1 packet |
| 650 kbps | 255 bytes | 1 packet | 1 packet | 1 packet |

**No fragmentation needed for any FIPS control message.** The full handshake +
heartbeats complete in single-packet exchanges at any FLRC bitrate.

### 3.2 Data Frames

Full FIPS session datagrams (up to 2048 bytes) still require fragmentation +
erasure coding. The erasure pipeline from balloon-fresh (PRBS23-XOR) remains
necessary for large payloads. The 2W power does not change the FLRC packet size
— it only improves the link margin, meaning fewer fragments are lost to
packet errors at range.

**Practical impact:** At 2W with 10-20 km LOS, the packet error rate drops
significantly compared to baseline. Erasure coding redundancy can be reduced
from 30% to 10-15% for typical links, improving effective throughput.

## 4. Transport Design (Updated)

### 4.1 Layer Stack

```
L7  Application (Nostr, routing msgs)
L6  FIPS Noise IK (end-to-end encrypt)
L5  FIPS STP + bloom filter routing (future — not needed for V1)
L4  FIPS FMP session protocol
L3  Pipeline: frag + PRBS23-XOR erasure (only for >255B payloads)
L2  LR2021 FLRC framing (TxFramer/RxFramer) — UNCHANGED
L1  NiceRF LoRa2021F33-2G4 (2W PA, +33 dBm) — NEW MODULE
L0  ESP32-C3 SPI bus (MOSI=7, MISO=2, SCLK=6, CS=10) — UNCHANGED
```

### 4.2 What This Means for Current Work

The EspHalLr2021Radio SPI driver being developed is **unaffected** by the
module change. The F33 uses the same Semtech SX1280 die, same SPI command
set, same register map. The PA is transparent to the digital interface.

**No code changes required for the 2W module:**
- Lr2021Radio trait — no changes (abstracts SPI + register access)
- EspHalLr2021Radio impl — no changes (SPI init sequence identical)
- Lr2021Transport adapter — no changes (uses Lr2021Radio trait)
- TxFramer/RxFramer — no changes (FLRC framing is modulation-level)
- TX power register — set to max (0x1F = +33 dBm on F33, was max ~+12 dBm
  on baseline). This is a one-register change in the init sequence.

**TX power configuration:**
The SX1280 TX power register (`SetTxParams`) controls output power in
steps. On the F33 module, the internal PA amplifies the chip's output.
The SPI command is the same — only the power value changes:

```rust
// Baseline module: max TX power ~+12 dBm (0x0C)
// F33 2W module: max TX power +33 dBm (0x1F or per F33 datasheet)
radio.set_tx_power(0x1F); // Set to max for F33
```

This is the ONLY code change: the TX power value in the radio init.

### 4.3 V1 Architecture: Point-to-Point 2W

For the first flight (V1), point-to-point is sufficient. No mesh routing
(STP + bloom filters) needed. Two nodes communicate directly:

```
+-------------------+          FLRC 2.4 GHz, 2W          +-------------------+
|  Balloon Node     |  ==============================  |  Ground Station   |
|  ESP32-C3 + F33   |     10-20 km LOS (FLRC)          |  ESP32-C3 + F33   |
|  FIPS Node (init) |     100+ km LOS (LoRa fallback)  |  FIPS Node (resp) |
+-------------------+                                  +-------------------+
```

**V1 scope (point-to-point):**
- Noise IK handshake: 2 packets (MSG1 114B → MSG2 69B), <1s at any bitrate
- Heartbeat: 32B every 10s, 1 packet each
- Telemetry uplink: fragmented + erasure-coded, ~1 Mbps effective at FLRC 2600
- Emergency fallback: switch to LoRa SF12 for 100+ km beacon mode

**V1 does NOT include:**
- Mesh routing (STP, bloom filters)
- Multi-hop forwarding
- More than 2 nodes

### 4.4 V2 Architecture: Multi-Node Mesh (Future)

With 2W power, mesh relay becomes viable at much longer inter-node distances:

```
  Balloon A (2W) ---- 10-20 km ---- Balloon B (2W)
      |                                   |
      +---- 10-20 km ---- Ground Station ---+
                         (2W)
```

Each hop can span 10-20 km FLRC. A 3-node chain covers 20-40 km total path.
STP + bloom filter routing (deferred to V2) enables this.

## 5. Range Testing Plan

### 5.1 Ground Validation (Before Flight)

1. **Bench test:** Verify F33 module initializes over SPI, TX power register
   reads back 0x1F, FLRC mode active. Serial log confirmation.
2. **Short-range LOS (100m):** FLRC 2600 kbps, 0% loss expected. Compare
   with baseline module at same distance — should see identical packet
   structure, different RSSI (higher TX power).
3. **Medium-range LOS (1-5 km):** FLRC 2600 kbps, measure packet loss.
   Baseline module would lose signal here. 2W should sustain link.
4. **Long-range LOS (10-20 km):** FLRC at reduced bitrate (650 kbps if
   needed). Measure RSSI, SNR, packet loss at edge of range.
5. **LoRa fallback test:** Switch to LoRa SF12, verify 100+ km margin
   (test at 10+ km, confirm RSSI is well above sensitivity floor).

### 5.2 Flight Test

1. Balloon ascends with 2W FLRC node (initiator)
2. Ground station with 2W FLRC node (responder)
3. Continuous FIPS heartbeat exchange throughout ascent
4. Telemetry: altitude, GPS, link RSSI, SNR, packet loss
5. At max altitude (>30 km): verify LoRa fallback if FLRC degrades
6. Record: max distance for FLRC contact, max distance for LoRa contact

## 6. Power Considerations

### 6.1 TX Current Draw

| Mode | Baseline (+12 dBm) | 2W Module (+33 dBm) | Notes |
|------|--------------------|--------------------|-------|
| TX current (FLRC) | ~30 mA | ~1.5-2.0 A | 2W PA draws significant current |
| RX current | ~12 mA | ~12 mA | PA inactive in RX |
| Sleep | ~1 µA | ~1 µA | Same chip sleep mode |

**Critical:** The 2W PA draws 1.5-2A during TX. The ESP32-C3 cannot power
this from its 3.3V rail. A separate power supply path is needed:

- Balloon node: dedicated 3.3V/2A regulator from battery pack
- Ground station: bench supply or high-capacity battery

**Duty cycling:** At 10s heartbeat interval (32B packet, ~0.3ms TX time),
average current is negligible. The PA only draws during TX bursts. For
telemetry uplink at higher duty cycle, thermal management may be needed
on the F33 module heatsink.

### 6.2 Power Sequencing

The F33 module PA may require specific power-up sequencing:
1. ESP32-C3 boots, holds RESET low on LR2021
2. PA power rail stabilized (separate regulator)
3. Release RESET, SPI init, set FLRC mode
4. Set TX power to max (0x1F)
5. Ready for TX/RX

**Verify against F33 datasheet** — PA enable pin may need GPIO control
separate from the SX1280 chip RESET. If the F33 has a PA enable pin, add
it to the GPIO mapping.

## 7. No-Change Confirmation

The following work items are **completely unaffected** by the 2W module
change. No redesign, no rework, no re-estimation needed:

| Work Item | Status | Impact of 2W |
|-----------|--------|--------------|
| Fix LR2021 test crate compile errors | In progress | NONE — import paths, SPI-agnostic |
| EspHalLr2021Radio SPI driver | Not started | NONE — same SPI interface |
| LR2021 FLRC framing (TxFramer/RxFramer) | Written, tests broken | NONE — modulation-level |
| Lr2021Transport adapter | Written, untested | NONE — trait-level abstraction |
| Erasure coding port (Rust) | Not started | NONE — payload-level, radio-agnostic |
| FIPS Noise IK handshake | Proven (sim + UART) | NONE — protocol-level |
| FIPS FMP session protocol | Proven | NONE — protocol-level |
| Pin mapping (GPIO7/2/6/10/3/4/5) | Documented | NONE — same module footprint |

**The 2W change is purely an RF front-end upgrade.** The entire software
stack from SPI driver upward is identical. The only code change is the
TX power register value in the radio init sequence.

## 8. Revised Integration Checklist

Updated from INTEGRATION-ASSESSMENT.md checklist, with 2W annotations:

1. [ ] LR2021 test crate compiles and all 13 framing tests pass on host
2. [ ] EspHalLr2021Radio implemented for esp-hal SPI (real hardware driver)
3. [ ] **F33 module: verify PA power rail (separate 2A regulator)**
4. [ ] **F33 module: verify PA enable pin (if any) — add to GPIO mapping**
5. [ ] **F33 module: set TX power to 0x1F (+33 dBm) in radio init**
6. [ ] Lr2021Transport roundtrip test passes with MockLr2021Radio (host)
7. [ ] Lr2021Transport compiles for riscv32imc-unknown-none-elf target
8. [ ] Erasure coding ported to Rust no_std (from balloon-fresh C source)
9. [ ] Erasure coding unit tests pass on host
10. [ ] Erasure coding compiles for riscv32imc target
11. [ ] LR2021 binary flashes to ESP32-C3 with F33 module, radio init succeeds
    (serial log: 2440 MHz, +33 dBm, FLRC 2600 kbps)
12. [ ] Two-node 2W LR2021 demo: MSG1 sent via FLRC, received + logged
13. [ ] FIPS Noise IK handshake completes over 2W LR2021 radio (MSG1 → MSG2)
14. [ ] FIPS encrypted message sent + received over 2W LR2021 (end-to-end crypto)
15. [ ] **Range test: 1-5 km LOS, FLRC 2600 kbps, measure RSSI + loss**
16. [ ] **Range test: 10-20 km LOS, FLRC (reduce bitrate if needed)**
17. [ ] **LoRa fallback test: switch to SF12, verify 100+ km margin**
18. [ ] Noise IK responder implemented (bidirectional handshake)
19. [ ] RAM usage measured on ESP32-C3 (verify < 300KB used, leaving margin)
20. [ ] 24-hour stability test: two 2W nodes, heartbeat alive, no panics

## 9. Risks (Updated)

| # | Risk | Likelihood | Impact | Mitigation |
|---|------|-----------|--------|------------|
| 1 | PA current exceeds ESP32-C3 3.3V rail capacity | HIGH | HIGH | Dedicated 2A regulator for PA power. Verify before first TX. |
| 2 | F33 PA enable pin not in current GPIO mapping | MEDIUM | MEDIUM | Check F33 datasheet. If PA enable exists, assign spare GPIO. |
| 3 | PA thermal shutdown during high-duty TX | LOW | MEDIUM | Monitor temperature, duty-cycle limit if needed. Heatsink on F33. |
| 4 | SPI driver debugging (same as baseline) | MEDIUM | MEDIUM | RP2040 logic analyzer for SPI signal verification. |
| 5 | RAM budget on ESP32-C3 | MEDIUM | MEDIUM | Measure early. LR2021-only path (no WiFi) uses less RAM. |
| 6 | Regulatory: 2W at 2.4 GHz may exceed EIRP limits | LOW | LOW | Balloon mission is experimental/test. Document compliance notes. |

**New risk (#1) is the most critical:** The 2W PA current draw (1.5-2A)
must not go through the ESP32-C3's onboard regulator. Hardware design must
include a dedicated high-current path for the PA. This is a hardware
(boards/wiring) concern, not a firmware concern — but it must be verified
before the first TX test to avoid brown-out reset.

## 10. Relationship to Other Plans

| Document | Relationship |
|----------|-------------|
| `docs/architecture.md` | Overall FIPS architecture — protocol layer, unchanged by 2W |
| `docs/plan-espnow-fips-mesh.md` | ESP-NOW mesh plan — parallel radio path, LR2021 is now primary |
| `docs/INTEGRATION-ASSESSMENT.md` | Integration assessment — baseline was +12 dBm, this plan supersedes RF assumptions |
| `~/repos/balloon-fresh/` | Erasure coding source for port — unaffected by module change |
| RadioLib LR2021 FLRC PRs | Upstream RadioLib changes — affect driver API, not RF module |

## 11. Summary

The 2W module upgrade is a **pure RF front-end change** with zero software
stack impact. The +42 dB total system gain improvement transforms the link
from sub-kilometer bench testing to 10-20 km FLRC / 100+ km LoRa — enabling
real balloon-to-ground communication without any protocol or driver redesign.

**The critical path remains:**
1. Fix LR2021 test crate compile errors (2-4h)
2. Write EspHalLr2021Radio SPI driver (2-3 days)
3. Flash to ESP32-C3 with F33 module, verify radio init (1 day)
4. Two-node 2W FIPS demo (2-3 days)
5. Range validation 1-5 km, then 10-20 km (1-2 days)

**New critical-path item:** Verify PA power rail before first TX (hardware,
not firmware — but blocks all radio testing).