#![cfg(any(feature = "ble", feature = "l2cap", feature = "wifi", feature = "esp-now"))]

/// Initialize the logger using esp-println's built-in log integration.
///
/// On targets without atomic CAS (e.g. ESP32-C3 RISC-V), `log::set_logger`
/// is unavailable. `esp_println::logger::init_logger` uses the `_racy`
/// variants which work on all platforms.
pub fn init() {
    esp_println::logger::init_logger(log::LevelFilter::Info);
}