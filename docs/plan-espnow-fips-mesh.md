# Plan: ESP-NOW Transport for microFIPS Mesh on ESP32-C3

**Status**: Draft — pending approval
**Date**: 2026-07-07
**Author**: Hermes (microFIPS-esp32 group discussion)

## 1. Summary

Replace WiFi AP/Station hierarchy with ESP-NOW peer-to-peer L2 transport for the microFIPS mesh on ESP32-C3. FIPS handles all mesh routing (spanning-tree + bloom filters). The pipeline/erasure-coding layer from `balloon-fresh` handles fragmentation (250-byte ESP-NOW MTU → FIPS's 2048-byte frames). ESP-NOW is just `send(mac, data)` / `recv(mac, data)` — the simplest possible L2 transport.

## 2. Architecture

### Before (Current)
```
FIPS Node A ←→ WiFi AP/Station ←→ FIPS Node B
              (hierarchy, IP, DHCP, ARP)
```

### After (Proposed)
```
FIPS Node A ←→ [ESP-NOW unicast] ←→ FIPS Node B
              (peer-to-peer, no IP, no hierarchy)
```

### Full Stack
```
L7  Application (Nostr, routing msgs)
L6  FIPS Noise XK (end-to-end encrypt)   ← from microfips-protocol
L5  FIPS STP + bloom filter routing      ← from FIPS (already in plan)
L4  FIPS FMP session protocol            ← from microfips-protocol
L3  Pipeline: frag + PRBS23-XOR erasure  ← REUSE from balloon-fresh
L2  ESP-NOW unicast transport            ← NEW (this plan)
L1  ESP32-C3 built-in WiFi radio (2.4GHz)
```

## 3. Reuse from balloon-fresh

| Component | Path | What It Does | Reuse |
|---|---|---|---|
| `components/erasure/` | `tracker/firmware/components/erasure/` | PRBS23-XOR erasure encode/decode, ~1KB RAM | Port to Rust or wrap via FFI |
| Fragment header format | `mesh-stack/protocol/SPEC.md` §4 | 6-byte header: block_id + frag_index + original_count + crc16 | Adopt as-is |
| Pipeline logic | `SPEC.md` §6 | Pad → split → encode → header → emit | Reimplement in Rust or port C |
| FIPS bridge | `mesh-stack/flrc-bench-espidf/` | FIPS-over-radio proven working | Reference design |

## 4. Key Constraints

| Constraint | Value | Impact |
|---|---|---|
| ESP-NOW MTU | 250 bytes | FIPS 2048-byte MAX_FRAME → 9 fragments |
| Peer limit | ~20 per node | Manageable for mesh neighborhood |
| ESP-NOW delivery | Fire-and-forget | Erasure coding compensates for loss |
| Channel | All nodes same WiFi channel | Must coordinate with any WiFi AP usage |
| ESP32-C3 RAM | 400KB total | ~100KB for ESP-NOW + FIPS + pipeline |
| Language | Rust + embedded Embassy | ESP-NOW C API needs esp-now-sys FFI bindings |

## 5. Phased Task Plan

### Phase 0: Foundation (2-3 days)

**P0.1 — esp-now-sys crate**
- [ ] Create FFI bindings for ESP-NOW C API (`esp_now.h`)
- [ ] Wrap: `esp_now_init()`, `esp_now_register_send_cb()`, `esp_now_register_recv_cb()`
- [ ] Wrap: `esp_now_add_peer()`, `esp_now_send()`, `esp_now_deinit()`
- [ ] Wrap: `esp_now_peer_info_t` struct + MAC address helpers
- [ ] Unit test at FFI boundary (init/deinit cycle on real HW)

**P0.2 — esp-now-transport crate (new)**
- [ ] Implement `microfips_protocol::transport::Transport` trait for ESP-NOW
- [ ] `send(mac, data)`: add MAC to peer list → `esp_now_send()` → return OK
- [ ] `recv()`: callback-driven, buffer incoming into circular queue
- [ ] Register/unregister peers dynamically via FIPS routing decisions
- [ ] Feature gate: `esp-now` feature on `microfips-esp32c3`
- [ ] Test: send/receive loopback on two ESP32-C3 boards

**P0.3 — ESP32-C3 binary variant "espnow"**
- [ ] New binary target `crates/microfips-esp32c3/src/bin/espnow.rs`
- [ ] Init ESP-NOW, register local peer
- [ ] Expose UART control CLI (like existing wifi/ble variants)
- [ ] Test: two C3 boards exchange FIPS handshake over ESP-NOW

### Phase 1: Pipeline (fragmentation + erasure) — 3-4 days

**P1.1 — Port erasure code to Rust**
- [ ] Port `balloon-fresh` PRBS23-XOR erasure from C to `#[no_std]` Rust
- [ ] `ErasureEncoder::new(k, block_size)`, `encode_redundant(idx) → [u8]`
- [ ] `ErasureDecoder::new(k, block_size)`, `process(frag_counter, data) → status`
- [ ] 5+ unit tests matching existing C test suite
- [ ] Feature gate: `erasure` on `microfips-link` or new `microfips-pipeline` crate

**P1.2 — Pipeline TX**
- [ ] `Pipeline::send_frame(data: &[u8]) → Result`
- [ ] Pad data to `k * block_size`
- [ ] Split into `k` fragments
- [ ] Generate `N` redundant fragments
- [ ] Prepend 6-byte header to each
- [ ] Emit via `esp_now_send()` callback
- [ ] Test: 2000-byte input → verify fragments on receiver

**P1.3 — Pipeline RX**
- [ ] `Pipeline::recv_frame() → Option<Vec<u8>>`
- [ ] Collect fragments by `block_id`
- [ ] Feed originals + redundant to erasure decoder
- [ ] Reassemble when decoder signals complete
- [ ] Trim padding, return full data
- [ ] Test: inject fragments out-of-order with 30% loss → verify recovery

**P1.4 — Integration: Pipeline + FIPS over ESP-NOW**
- [ ] Wire Pipeline between `Transport` trait and ESP-NOW transport
- [ ] FIPS Noise XK handshake over ESP-NOW pipeline (147 bytes MSG1 + MSG2, fits in 1-2 fragments)
- [ ] FMP encrypted data frames over ESP-NOW pipeline
- [ ] Test: FIPS PING/PONG between two ESP32-C3 boards
- [ ] Measure: latency per hop, throughput, packet loss

### Phase 2: Routing (STP + bloom filters) — 3-4 days

**P2.1 — MAC ↔ FIPS node address mapping**
- [ ] Maintain peer table: `[node_addr(16B) → mac_addr(6B)]`
- [ ] FIPS routing says "send to node_addr X" → look up MAC → ESP-NOW send
- [ ] Broadcast discovery: ESP-NOW broadcast with FIPS identity
- [ ] Add/remove peers dynamically as routing table changes

**P2.2 — Spanning tree protocol**
- [ ] Port or implement FIPS STP for root election (lowest hash)
- [ ] Root announcement broadcast via ESP-NOW broadcast
- [ ] Distance vector maintenance (root, distance, parent)
- [ ] Test: 3-node STP convergence

**P2.3 — Bloom filter path tracking**
- [ ] Maintain bloom filter per neighbor: "node X reachable via neighbor Y"
- [ ] Exchange bloom filters with neighbors via ESP-NOW unicast
- [ ] Query: "shortest path to node X" → which neighbor to send to
- [ ] Test: 4-node mesh, send from A→D via bloom filter routing

**P2.4 — FIPS mesh routing over ESP-NOW**
- [ ] FIPS routing layer uses ESP-NOW transport
- [ ] STP + bloom filter messages sent as Pipeline fragments
- [ ] Forwarding: receive → FIPS routing decision → ESP-NOW send to next hop
- [ ] Test: 3-node chain, A→B→C, FIPS tunnel through mesh

### Phase 3: Hardening — 2-3 days

**P3.1 — Peer table management**
- [ ] Max 20 peer limit → LRU eviction for stale peers
- [ ] Heartbeat mechanism to detect dead peers
- [ ] Auto-re-add peer if ESP-NOW send fails with ESP_ERR_ESPNOW_NOT_FOUND

**P3.2 — MTU adaptation**
- [ ] Auto-detect max payload per hop (250 bytes minus header = 244 payload)
- [ ] Pipeline block_size config per link
- [ ] Handle asymmetric MTU (different ESP32 variants)

**P3.3 — Reliability**
- [ ] Pipeline retransmit: if no fragments received within timeout, retransmit
- [ ] Erasure redundancy level adaptive (start at 30%, adjust based on loss)
- [ ] Test: sustained 30% packet loss, verify 0% data loss via erasure coding

**P3.4 — WiFi coexistence**
- [ ] ESP-NOW and WiFi AP can share radio on ESP32-C3
- [ ] Channel must match between ESP-NOW and any connected AP
- [ ] Document coexistence constraints
- [ ] Test: ESP-NOW mesh node also connected to WiFi AP for upstream

### Phase 4: Integration & Validation — 2-3 days

**P4.1 — Android exit node integration**
- [ ] ESP32-C3 ESP-NOW mesh → WiFi/AP link to Android exit node
- [ ] FIPS tunnel from mesh node through Android to internet
- [ ] Test: Nostr event from mesh node relayed to FIPS VPS

**P4.2 — Multi-hop throughput benchmark**
- [ ] 1 hop, 2 hop, 3 hop throughput measurement
- [ ] Compare: ESP-NOW vs current WiFi AP/Station
- [ ] Latency per hop (PING)
- [ ] Loss rate vs distance

**P4.3 — Long-duration stability test**
- [ ] 24-hour mesh stability test (3+ nodes)
- [ ] Monitor: peer drops, reconnections, routing convergence time
- [ ] Memory profiling: no leaks over time

## 6. Repository Layout

```
microfips/
  crates/
    microfips-esp32c3/
      src/bin/
        uart.rs        (existing)
        usb.rs         (existing)
        wifi.rs        (existing)
        espnow.rs      (NEW — Phase 0.3)
    microfips-esp-transport/
      src/
        esp_now_transport.rs   (NEW — Phase 0.2)
    microfips-pipeline/           (NEW — Phase 1)
      src/
        lib.rs
        fragment.rs             (6-byte header + reassembly)
        erasure.rs              (PRBS23-XOR port from balloon-fresh)
    microfips-now-sys/            (NEW — Phase 0.1)
      src/
        lib.rs                   (FFI bindings)
      build.rs
    microfips-routing-espnow/     (NEW — Phase 2)
      src/
        lib.rs
        peer_table.rs
        stp.rs
        bloom_routing.rs
```

## 7. Dependencies (NEW crates)

| Crate | Depends On | License |
|---|---|---|
| `microfips-now-sys` | esp-idf-sys (ESP-NOW headers) | MIT |
| `microfips-esp-transport` (esp-now feature) | microfips-now-sys, microfips-protocol | MIT |
| `microfips-pipeline` | microfips-protocol | MIT |
| `microfips-routing-espnow` | microfips-pipeline, microfips-esp-transport | MIT |

## 8. Acceptance Criteria

1. Two ESP32-C3 boards exchange encrypted FIPS messages over ESP-NOW
2. Three-node linear mesh forwards a FIPS frame through one intermediate hop
3. 250-byte FIPS frame (control message) delivered in a single ESP-NOW packet
4. 2048-byte FIPS frame (data payload) fragmented, erasure-coded, reassembled correctly
5. Routing recovers within 30s of a node going offline
6. 24-hour stability: no memory leaks, no peer table corruption

## 9. Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| ESP-NOW ESP32-C3 C API unstable | Low | High | Pin esp-idf-sys version, test early |
| RAM pressure (pipeline buffers) | Medium | Medium | Stack buffers on stack, not heap; tune k and block_size |
| ESP-NOW peer limit (20) | Low | Medium | LRU eviction; broadcast for >20 node discovery |
| Channel conflict with WiFi | Medium | Low | Document, test coexistence in P3.4 |
| Erasure port from C has bugs | Low | Medium | Port tests alongside code |

## 10. Schedule (gantt-style)

```
Phase 0: Foundation        ████████░░░░░░░░░░░░   3 days
Phase 1: Pipeline          ░░████████░░░░░░░░░░   4 days
Phase 2: Routing           ░░░░░░████████░░░░░░   4 days
Phase 3: Hardening         ░░░░░░░░░░██████░░░░   3 days
Phase 4: Integration       ░░░░░░░░░░░░░░██████   3 days
                           ────────────────────
                           17 days total (full-time)
```

## 11. Related Files

| File | Purpose |
|---|---|
| `docs/plan-espnow-fips-mesh.md` | This document |
| `crates/microfips-now-sys/src/lib.rs` | ESP-NOW C API FFI bindings |
| `crates/microfips-esp-transport/src/esp_now_transport.rs` | Transport trait impl |
| `crates/microfips-pipeline/src/` | Fragmentation + erasure coding |
| `crates/microfips-routing-espnow/src/` | STP + bloom filter routing |
| `crates/microfips-esp32c3/src/bin/espnow.rs` | Binary target for C3+ESP-NOW |
| `~/repos/balloon-fresh/tracker/firmware/components/erasure/` | Source for port |
| `~/repos/balloon-fresh/mesh-stack/protocol/SPEC.md` | Fragment format spec |
