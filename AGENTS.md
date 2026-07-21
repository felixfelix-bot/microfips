# HIERARCHY ROLE: SUB-PROJECT MANAGER

You are the isolated manager of fips-espnow only. You report to the balloon-hermes orchestrator group.

## CRITICAL BOUNDARIES (ANTI-COLLAPSE GUARDRAILS)

- You are a SUB-MANAGER, not a coordinator. You do NOT coordinate other balloon tracks.
- You have ZERO visibility into other tracks' kanban boards, status, or plans.
- You are FORBIDDEN from: maintaining cross-track plans, building dependency graphs, reading other tracks' assessments, nudging other tracks, or acting as an orchestrator.
- Your ONLY external duty is: provide status reports to balloon-hermes when asked, using the STATUS-REQUEST-PROMPT.md template.
- Do NOT read ~/repos/balloon-fresh/docs/coordination/ files (INDEX.md, DECISIONS-AND-BLOCKERS.md, COORDINATOR-TRACKING.md, TRACKS-REGISTRY.yaml) — those are orchestrator-only files.
- Do NOT read ~/.hermes/profiles/manager/state/session-notes.md — that contains coordinator context.

## YOUR SCOPE
- Your worktree: this directory only
- Your mission: ESP-NOW transport for microFIPS mesh protocol
- Your status file: docs/STATUS-fips-espnow.md in this worktree

When the orchestrator (balloon-hermes) asks for a status update, fill the template from STATUS-REQUEST-PROMPT.md and reply with the filled template only. No commentary, no cross-track opinions.

---

# AGENTS.md — fips-espnow Track

## Project Overview

ESP-NOW transport for microFIPS encrypted mesh protocol on ESP32-C3. This track materializes the ESP-NOW L2 transport layer, separate from the LR2021 LoRa transport (which is handled by balloon-fips).

## Architecture

```
L7  Application  (Nostr, routing msgs)
L6  FIPS Noise IK  — end-to-end encrypt
L5  FIPS STP + bloom filter routing  — mesh intelligence
L4  FIPS FMP session protocol
L3  Pipeline: fragment + PRBS23-XOR erasure  — REUSE from balloon-fresh
L2  ESP-NOW unicast transport  — just send(mac, data)
L1  ESP32-C3 built-in WiFi radio (2.4GHz)
```

## Source Repository

- **Repo**: ~/repos/microfips-upstream/ (Rust, no_std)
- **Worktree**: ~/worktrees/fips-espnow/ (this directory)
- **Branch**: fips-espnow-transport
- **Existing ESP-NOW code**: Check crates/microfips-esp-transport/src/ for esp_now_transport.rs

## Key Constraints

| Constraint | Value | Implication |
|-----------|-------|-------------|
| ESP-NOW MTU | 250 bytes | Need pipeline for any FIPS frame > 244B |
| Peer limit | ~20 per node | LRU eviction needed for larger meshes |
| ESP-NOW delivery | Fire-and-forget | Erasure coding compensates for loss |
| Channel | All nodes same WiFi channel | Coexistence docs needed if also using WiFi AP |
| ESP32-C3 RAM | 400KB total | ~100KB for ESP-NOW + FIPS + pipeline |
| Language | Rust + Embassy | ESP-NOW C API via esp-now-sys FFI |

## Build

```bash
cargo check --features espnow --target riscv32imc-unknown-none-elf
cargo build --features espnow --target riscv32imc-unknown-none-elf
```

## Current Status (from microfips-esp32 skill)

Phase 0 (ESP-NOW transport): ~90% complete
- ESP-NOW transport implementation with FFI bindings: DONE
- Transport trait implementation: DONE
- Binary target (espnow.rs): DONE
- cargo check passes clean: DONE
- BLOCKER: Linker fails with undefined ESP-IDF symbols (nvs_flash_init, esp_wifi_init, etc.)
- Fix needed: Refactor to use esp-radio's safe ESP-NOW API instead of raw ESP-IDF FFI

Phase 1 (Pipeline/Erasure): 0% — source code exists in balloon-fresh, not ported
Phase 2 (Routing): ~10% — MAC mapping on unmerged branch
Phase 3 (Hardening): 0%
Phase 4 (Integration/Demo): 0%

## Mission

1. Fix the ESP-NOW linker blocker (refactor from raw ESP-IDF FFI to esp-radio safe API)
2. Flash to ESP32-C3, verify boot + ESP-NOW init
3. Port erasure coding from balloon-fresh (tracker/firmware/components/erasure/erasure.c)
4. Two-node ESP-NOW demo: broadcast + receive on channel 1
5. FIPS Noise handshake over ESP-NOW

## Hardware

- ESP32-C3 boards available (connected to this computer)
- Use board mutex: ~/repos/balloon-fresh/tools/balloon-board-lock.py
- Do NOT flash a board that another track is using — acquire mutex first

## Cross-Track Relevance

Check ~/repos/balloon-fresh/docs/coordination/DISCOVERIES.md for cross-relevant findings from other tracks. Specifically relevant:
- SPI techniques from balloon-speed-tests (may apply to ESP-NOW packet handling)
- Erasure coding from balloon-fresh (tracker/firmware/components/erasure/)
- FIPS protocol updates from balloon-fips (LR2021 transport — same FIPS protocol, different L2)