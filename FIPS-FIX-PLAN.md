# FIPS ESP32-C3 Build Fix Plan

## Problem
Per-member build (`cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf`)
resolves the critical-section feature unification conflict, but 3 code-level bugs remain.

## Bug 1: Missing esp32c3 cfg variants in config.rs

**File:** `crates/microfips-esp-transport/src/config.rs`

Currently only `#[cfg(feature = "esp32")]` and `#[cfg(feature = "esp32s3")]` exist.
Need `#[cfg(feature = "esp32c3")]` variants for:

| Constant | ESP32-C3 Value | Notes |
|----------|---------------|-------|
| DEVICE_NSEC | unique key per device | generate or hardcode |
| DEVICE_NAME | "microfips-c3" | |
| RESET_REGISTER | 0x60007000 | RTC_CNTL reset reg |
| UART0_BASE | 0x60043000 | |
| GPIO_FUNC_IN_SEL_BASE | 0x60004000 | |
| UART_RX_GPIO_NUM | 5 (or configurable) | |

**Fix:** Add `#[cfg(feature = "esp32c3")]` blocks matching the esp32/esp32s3 pattern
with C3-specific register addresses.

**Risk:** None — only adds new cfg branch, doesn't touch existing targets.

## Bug 2: AtomicU32 not available on RISC-V

**File:** `crates/microfips-esp-common/src/stats.rs`

`core::sync::atomic::AtomicU32::fetch_add` requires hardware atomic CAS.
ESP32-C3 (RISC-V single-core) lacks this. Need `portable-atomic` crate.

**Fix:**
1. Add `portable-atomic = { version = "1", default-features = false }` to
   `crates/microfips-esp-common/Cargo.toml` `[dependencies]`
2. In `stats.rs`, replace:
   ```rust
   use core::sync::atomic::AtomicU32;
   ```
   with:
   ```rust
   #[cfg(target_arch = "riscv32")]
   use portable_atomic::AtomicU32;
   #[cfg(not(target_arch = "riscv32"))]
   use core::sync::atomic::AtomicU32;
   ```
3. All `AtomicU32::fetch_add`, `load`, `store` calls work identically —
   `portable_atomic::AtomicU32` has the same API.

**Risk:** Low — portable-atomic is a drop-in replacement. ESP32/S3 (Xtensa)
keep using `core::sync::atomic` which is natively supported there.

## Bug 3: log::set_logger missing in no_std

**File:** Wherever logger init happens (check `main.rs` or `lib.rs` of esp32c3 member)

`log::set_logger` and `log::set_max_level` require either:
- `std` feature on `log` crate (not available in no_std)
- A custom logger implementation registered via `log::set_logger`

**Fix options (pick one):**

A) Use `esp-println` logger (simplest):
   - Add `esp-println = { version = "0.10", features = ["log", "esp32c3"] }` to deps
   - Call `esp_println::logger::init_logger(log::LevelFilter::Info)` at startup
   - This registers a logger that outputs via UART

B) Implement minimal logger:
   ```rust
   struct NoStdLogger;
   impl log::Log for NoStdLogger {
       fn enabled(&self, _: log::Level) -> bool { true }
       fn log(&self, record: &log::Record) {
           // Output via esp-println or UART directly
       }
       fn flush(&self) {}
   }
   static LOGGER: NoStdLogger = NoStdLogger;
   // At startup:
   log::set_logger(&LOGGER).ok();
   log::set_max_level(log::LevelFilter::Info);
   ```

**Recommended:** Option A (esp-println) — less code, already in workspace deps.

**Risk:** None — only affects C3 target. Xtensa targets may already have
their own logger setup.

## Order of Changes

1. Fix Bug 2 first (portable-atomic) — unblocks compilation of stats module
2. Fix Bug 1 (config.rs) — unblocks compilation of transport module
3. Fix Bug 3 (logger) — unblocks main binary
4. Build: `cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf`
5. If clean, add to CI: `.cargo/config.toml` with riscv32imc target

## Verification

```bash
# After each fix:
cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf 2>&1 | grep error | head -5

# After all fixes:
cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf && echo "FIPS C3 BUILD OK"

# Also verify esp32 and esp32s3 still build:
cargo build -p microfips-esp32s3 --target xtensa-esp32s3-elf 2>&1 | tail -3
```

## Estimated time: 2 hours