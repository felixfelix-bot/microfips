# MFE-002 Status: Flash and validate ESP-NOW binary — BLOCKED (dependency + hardware)

**Task:** MFE-002 (kanban board `microfips`) — Flash ESP-NOW binary to physical
ESP32-C3 on `/dev/ttyACM2`; verify boot, "ESP-NOW initialized" + MAC on serial,
3-fast-blink LED pattern, no panics.
**Date:** 2026-10-01
**Outcome:** NOT executable on this host today. Two independent blockers,
both with reproduced evidence below. Deliverable of this pass: the reusable
flash+validate harness `labgrid/scripts/flash_espnow_validate.sh` that encodes
the card's success criteria as automated serial-log assertions, ready to run
the moment both blockers clear. No serial output was captured and none is
claimed — there is no board to produce it.

## Blocker 1 — dependency MFE-001 is not complete: the binary does not link

MFE-002's own plan entry lists "Task 1.1 (Linking fix) completed" as a
dependency (`docs/microfips-esp32-implementation-plan-2026-07-09.md:113`).
Reproduced on `feat/fips-v0-compat` @ `a481095` (esp toolchain
`1.93.0-nightly`):

```
cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf \
    -Zbuild-std=core,alloc --features espnow \
    --bin microfips-esp32c3-espnow --release

rust-lld: error: undefined symbol: esp_netif_init
rust-lld: error: undefined symbol: esp_event_loop_create_default
rust-lld: error: undefined symbol: esp_wifi_init
rust-lld: error: undefined symbol: esp_read_mac
  >>> referenced by microfips_esp_transport
  >>>   (<microfips_esp_transport::esp_now_transport::EspNowTransport>::init)
error: could not compile `microfips-esp32c3` (bin "microfips-esp32c3-espnow")
```

All four are ESP-IDF C APIs declared in
`crates/microfips-esp-transport/src/esp_now_transport.rs` (ffi block, ~L57) and
called from `EspNowTransport::init` (L207–L251). The esp-radio blob set linked
by this build (`-lespnow -lnet80211 -lphy -lpp -lcoexist -lcore …`) provides
the *other* FFI calls used there (`esp_wifi_set_mode`, `esp_wifi_start`,
`esp_wifi_set_channel`, `esp_now_*`) but no provider exists for these four.
`FIPS-FIX-PLAN.md` ("espnow path — fixed compile bugs, remaining link gap")
reached the same conclusion and scoped the fix as rewriting
`EspNowTransport` against esp-radio's Rust APIs — that is MFE-001 feature
work (a separate Critical card, still pending), not flash-validate work.

Note: the last seven commits on the branch (`a6f2220` NVS FFI drop, `d7b1044`
logger wiring, `26e439f` register fixes, `a481095` log-backend hardening)
closed the *earlier* undefined symbols (`nvs_flash_init` et al.) and the
compile bugs, but the four above remain. MFE-001's card status (`pending`,
unassigned) matches reality: do not mark it done from the commit log alone.

### Side finding — the plan doc's build command is stale

`docs/microfips-esp32-implementation-plan-2026-07-09.md:87` builds without
`-Zbuild-std`. On the installed `esp` toolchain (which ships host std +
rust-src only, no prebuilt `riscv32imc-unknown-none-elf` std) that exact
command fails before any crate compiles:

```
error[E0463]: can't find crate for `core`
  = note: the `riscv32imc-unknown-none-elf` target may not be installed
```

The working invocation (also in `FIPS-FIX-PLAN.md` and used by
`scripts/test_dual_mcu.sh`) is the one with `-Zbuild-std=core,alloc`. The
harness uses the working form.

## Blocker 2 — no ESP32-C3 hardware is present on this host

The card and plan assume "2x ESP32-C3 boards (on /dev/ttyACM1/2)"
(plan L18). Measured now:

```
$ ls /dev/ttyACM* /dev/ttyUSB*
ls: cannot access '/dev/ttyACM*': No such file or directory
ls: cannot access '/dev/ttyUSB*': No such file or directory

$ ls /dev/serial/by-id/
ls: cannot access '/dev/serial/by-id/': No such file or directory

$ lsusb
Bus 002 Device 002: ID 0bda:8153 Realtek RTL8153 Gigabit Ethernet
Bus 003 Device 002: ID 0bda:b850 Realtek Bluetooth Radio
Bus 003 Device 003: ID 21c4:0004 Lexar ES4
Bus 003 Device 004: ID 0573:1573 Zoran USB Audio and HID
(no 303a Espressif, no 10c4 CP210x, no 1a86 CH340 — no serial device at all)

$ journalctl -k -g 'ttyACM|cdc_acm' --since -30d
-- No entries --
```

No USB-serial enumerator has appeared in the kernel log for 30 days, so the
boards were not recently detached either. `labgrid/config/inventory.yaml`
defines an `esp32-d0wd` target but has **no esp32c3 entry**, and
`labgrid/config/envs/` contains no esp32c3 env — there is no remote/exporter
path to a C3 either; all labgrid targets are local-attach designs.

## What this pass delivered

- `labgrid/scripts/flash_espnow_validate.sh` — build (MFE-001 gate) → flash
  (`espflash flash --chip esp32c3`) → timed monitor capture → automated
  assertions, derived from the firmware's real serial contract in
  `crates/microfips-esp32c3/src/run.rs`:
  - `ESP-NOW initialized. MAC: ([0-9a-f]{2}:){5}[0-9a-f]{2}` (run.rs:75;
    `MacAddress` Display is lowercase colon-hex, esp_now_transport.rs:133)
  - `ESP-NOW mesh node ready on channel 1` (run.rs:89; `ESPNOW_CHANNEL = 1`)
  - negative check: no `panic` / `ESP-NOW init failed` /
    `Failed to add broadcast peer` markers (the `.expect()` strings, run.rs:73,87)
  - LED 3-fast-blink is a physical check — the script prints a manual-check
    reminder; it cannot be asserted from serial.
  Smoke-tested to the extent possible without a board: it fails closed at
  preflight (verified: `[FAIL] espflash not installed` → exit 1 while the
  install was in flight; same gate covers the missing-port case next).
- espflash installed (card dependency "espflash tool installed"):
  `espflash 4.6.0` requires rustc ≥ 1.95 (host stable is 1.94), so pinned
  `cargo +stable install espflash --version 4.5.0 --locked` — installed to
  `~/.cargo/bin/espflash` (verified `espflash 4.5.0`). Note: `~/.cargo/bin`
  is NOT on this host's default PATH (why an older 4.3.0 was invisible to
  `which espflash`); the harness resolves PATH first, then falls back to
  `~/.cargo/bin/espflash`.
- This document.

## To unblock (owner: manager / next dispatch)

1. Dispatch MFE-001 (esp-radio Rust API rewrite of `EspNowTransport::init`
   init path — at minimum replace the four unlinked IDF calls) and confirm
   `labgrid/scripts/flash_espnow_validate.sh` passes its build step.
2. Attach the ESP32-C3 board (any CP210x/C3 SuperMini with native USB) and
   confirm it enumerates (expect `/dev/ttyACM*`; the card names ACM2).
3. Run the harness: `PORT=/dev/ttyACM2 labgrid/scripts/flash_espnow_validate.sh`.
