#![cfg(all(
    feature = "log",
    any(feature = "esp32", feature = "esp32s3", feature = "esp32c3")
))]

//! Global log backend for the ESP firmware images.
//!
//! The module is compiled whenever the crate has both the `log` crate (`log`
//! feature) and the esp-println printer (each chip feature enables
//! `esp-println/<chip>`). It used to be gated on `any(ble, l2cap, wifi, esp-now)`,
//! which excluded the radio-less `uart`/`usb` binaries; every one of those features
//! implies `log`, so the new gate is a strict superset of the old one.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct UartLogger;

static LOGGER: UartLogger = UartLogger;

impl Log for UartLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            esp_println::println!(
                "[{} {}] {}",
                record.level(),
                record.module_path().unwrap_or("?"),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Installs the global log backend for this image.
///
/// `log::set_logger` / `log::set_max_level` are gated in `log` 0.4 behind
/// `target_has_atomic = "ptr"`, which is **false** for
/// `riscv32imc-unknown-none-elf` (RV32IMC has no compare-and-swap). Gating the whole
/// function on that cfg made it an empty no-op on the ESP32-C3, so every
/// `log::info!` / `log::error!` in the C3 firmware (`microfips-protocol` alone has
/// 47 sites) was discarded at runtime.
///
/// The CAS-less branch installs through `log`'s racy API (`set_logger_racy` /
/// `set_max_level_racy`): ungated in `log` 0.4, available on every target, and
/// backed by a `Cell`-wrapped `AtomicUsize` where pointer-width atomics do not
/// exist. That is the *same* mechanism `esp-println`'s own `log-04` backend uses
/// (`esp-println-0.18.0/src/logger.rs::init_logger` calls exactly these two
/// functions), so nothing here is hand-rolled and the `log` contract is unchanged.
///
/// # Why the racy install is sound here (CAS-less targets)
///
/// `set_logger_racy` must not race another installer. Each caller is a single
/// firmware entry point (`run_uart_node`, `run_wifi_node`, `run_esp_now_node`, …)
/// reached once from `main` before any other task runs, and no entry point calls
/// another, so two installers can never overlap. An interrupt that logs in the
/// window between the `LOGGER` store and the `STATE` store sees
/// `STATE == UNINITIALIZED` (and a max level of `Off`) and is dropped, so a
/// partially-initialised logger is never observed.
#[cfg(target_has_atomic = "ptr")]
pub fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(LevelFilter::Info);
}

#[cfg(not(target_has_atomic = "ptr"))]
pub fn init() {
    // SAFETY: single installer per image, called once from the entry point before
    // any other task exists; see the doc comment above.
    unsafe {
        log::set_logger_racy(&LOGGER).unwrap();
        log::set_max_level_racy(LevelFilter::Info);
    }
}

/// Compile-time witness that the *installing* path is the one compiled for this
/// target: `set_logger_racy` is the ungated alternative to the
/// `target_has_atomic = "ptr"`-gated `set_logger`, so this static only resolves if
/// the racy installer really exists on `riscv32imc-unknown-none-elf` (a regression to
/// a no-op or to `set_logger` fails the C3 build). `#[used]` keeps the pointer in the
/// object file so the C3 image can be inspected as evidence
/// (`llvm-nm`/`llvm-objdump`) that a real installer is linked in.
#[cfg(not(target_has_atomic = "ptr"))]
#[used]
static RACY_INSTALL_WITNESS: unsafe fn(&'static dyn Log) -> Result<(), log::SetLoggerError> =
    log::set_logger_racy;
