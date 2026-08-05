//! ESP32-C3 UART binary entry point.

#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();
microfips_esp_transport::panic_blink!();

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let sw_ints =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_ints.software_interrupt0);

    // UART on GPIO20 (TX) and GPIO21 (RX) for ESP32-C3
    let gpio2 = peripherals.GPIO2;   // LED
    let gpio20 = peripherals.GPIO20;
    let gpio21 = peripherals.GPIO21;
    let uart0 = peripherals.UART0;
    let rng_periph = peripherals.RNG;
    let adc1 = peripherals.ADC1;

    microfips_esp32c3::run::run_uart_node(
        gpio2, uart0, gpio20, gpio21, rng_periph, adc1,
    ).await;
}