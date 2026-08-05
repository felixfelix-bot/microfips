//! Stub binary — wifi feature not used for balloon.
//! Created to satisfy Cargo.toml [[bin]] declaration.
#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();
microfips_esp_transport::panic_blink!();

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) {
    // WiFi stub — not used for balloon flight
    loop {}
}