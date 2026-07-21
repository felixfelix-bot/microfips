//! Shared ESP32 transport implementations: UART, USB CDC, BLE GATT, BLE L2CAP, WiFi, ESP-NOW, and common hardware abstractions.

#![no_std]

extern crate alloc;

#[cfg(all(
    feature = "esp32",
    feature = "esp32s3",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
compile_error!("features \"esp32\" and \"esp32s3\" are mutually exclusive");

pub mod config;
pub mod gpio_helpers;
pub mod handler;
pub mod heap;
pub mod led;
pub mod macros;
pub mod node_info;
pub mod rng;
pub mod runner;
pub mod run_tasks;
pub mod stats;
pub mod uart_transport;

/// LR2021 FLRC framing layer (hardware-agnostic, unit-testable).
pub mod lr2021_framing;

/// LR2021 SPI driver (trait + mock, hardware-agnostic).
#[cfg(any(test, feature = "mock"))]
pub mod lr2021_spi;

/// LR2021 Transport trait adapter.
#[cfg(any(test, feature = "mock"))]
pub mod lr2021_transport;

/// LR2021 real hardware SPI driver for ESP32-C3 using esp-hal.
/// Only compiled for embedded targets (esp32c3 feature).
#[cfg(feature = "esp32c3")]
pub mod lr2021_esp_hal;

#[cfg(any(feature = "esp32s3", feature = "esp32c3"))]
pub mod usb_transport;

#[cfg(feature = "wifi")]
pub mod wifi_transport;

#[cfg(feature = "espnow")]
pub mod esp_now_transport;

#[cfg(any(feature = "ble", feature = "l2cap", feature = "wifi", feature = "espnow"))]
pub mod control;
#[cfg(any(feature = "ble", feature = "l2cap", feature = "wifi", feature = "espnow"))]
pub mod logger;

#[cfg(feature = "ble")]
pub mod ble_host;
#[cfg(feature = "ble")]
pub mod ble_transport;

#[cfg(feature = "l2cap")]
pub mod backoff;
#[cfg(feature = "l2cap")]
pub mod l2cap_host;
#[cfg(feature = "l2cap")]
pub mod l2cap_transport;
#[cfg(feature = "l2cap")]
pub mod peer_caps;
#[cfg(feature = "l2cap")]
pub mod rate_limit;
