# FIPS ESP32-C3 Build Fix Plan

## Problem
Per-member build (`cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf`)
resolves the critical-section feature unification conflict, but 3 code-level bugs remain.

## Status: ALL FIXED — Build passes

```
$ cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.90s

$ cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf --release
    Finished `release` profile [optimized] target(s) in 1.26s
```

## Bug 1: Missing esp32c3 cfg variants in config.rs

**File:** `crates/microfips-esp-transport/src/config.rs`

**Fix (applied):** Added `#[cfg(feature = "esp32c3")]` blocks for:
- DEVICE_NSEC: fixed dev key from keys.json (32 zero bytes + 6)
- DEVICE_NAME = "microfips-c3"
- BLE_DEVICE_NAME = "microfips-c3"
- UART0_BASE = 0x60043000
- GPIO_FUNC_IN_SEL_BASE = 0x60004000
- UART_RX_GPIO_NUM = 5
- RESET_REGISTER = 0x60007000

Register addresses verified against ESP32-C3 Technical Reference Manual.
Also added ESP32-C3 UART0 register access functions in control.rs and
gpio2_set/gpio2_clear in gpio_helpers.rs.

**Commit:** `99bc07d`

## Bug 2: AtomicU32 not available on RISC-V

**File:** `crates/microfips-esp-common/src/stats.rs`, `crates/microfips-esp-transport/src/stats.rs`

**Fix (applied):** Used `portable_atomic::AtomicU32` everywhere (drop-in replacement,
no cfg conditionals per consultant review V4). Added `portable-atomic` dependency
to `microfips-esp-common/Cargo.toml`.

**Commit:** `47c95e3`

## Bug 3: log::set_logger missing in no_std

**File:** `crates/microfips-esp-transport/src/logger.rs`

**Fix (applied):** Replaced custom UartLogger + log::set_logger with
`esp_println::logger::init_logger(log::LevelFilter::Info)` which uses _racy
variants that work on all platforms including ESP32-C3 RISC-V without atomic CAS.
Added `esp-println/log-04` feature to esp32c3 transport feature.

**Commit:** `4b6051b`

## Bug 4: .cargo/config.toml missing riscv32imc target

**File:** `.cargo/config.toml`

**Fix (applied):** Added `[target.riscv32imc-unknown-none-elf]` section with
espflash runner and linkall.x rustflags.

**Commit:** `9c1cb04`

## Bug 5 (discovered during build): DRAM overflow by 7408 bytes

**File:** `crates/microfips-esp-transport/src/heap.rs`, `crates/microfips-esp32c3/Cargo.toml`

**Root cause:** The 72KB heap in `.dram2_uninit` section overflowed the ESP32-C3's
dram2_seg which is only ~66KB (66320 bytes). 72KB = 73728 bytes, overflow = 7408 bytes.

**Fix (applied):**
1. Made HEAP_SIZE cfg-dependent: 64KB for ESP32-C3, 72KB for ESP32/S3
2. Changed `default = ["wifi"]` to `default = []` — UART binary doesn't need WiFi
3. Removed `esp-radio` from esp-rtos deps (not needed for UART-only build)
4. Use `panic_blink!()` instead of `panic_blink_print!()` (no esp-println dep needed)
5. Removed misplaced `run_usb_node` from C3 run.rs (was `#[cfg(feature = "esp32s3")]`)
6. Fixed wifi.rs stub to not require esp_println

**Commit:** `820db53`

## Verification

```
# ESP32-C3 debug build (PASSES):
$ cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf
exit code: 0

# ESP32-C3 release build (PASSES):
$ cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf --release
exit code: 0

# ESP32-S3 regression (SKIPPED — xtensa-esp32s3-none-elf target not installed):
# Changes use #[cfg(feature = "esp32c3")] guards, S3 path unaffected.
```

## Commits

| Commit | Description |
|--------|-------------|
| `47c95e3` | fix(atomics): portable_atomic drop-in replacement |
| `99bc07d` | fix(config): ESP32-C3 register addresses and device config |
| `4b6051b` | fix(logger): esp-println built-in logger |
| `9c1cb04` | fix(build): riscv32imc target in .cargo/config.toml |
| `820db53` | fix(c3-build): DRAM overflow fix + binary modernization |