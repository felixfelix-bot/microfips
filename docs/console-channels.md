# Console channels vs. frame channels (ESP32 family)

Rule for this repo: **an image may install the log backend only if esp-println's output
channel is a different peripheral from the one that carries its FIPS frames.**
`microfips_esp_transport::logger::init()` installs a backend that formats every
`log::info!`/`log::error!` record through `esp_println::println!`; if that printer shares the
frame channel, log text lands *inside* the byte stream the peer is parsing.

Per-image answer (measured, see "Verifying an image" below):

| Image | Frames leave on | esp-println channel | `logger::init()`? |
|---|---|---|---|
| `microfips-esp32s3-uart` | UART0 (GPIO43 TX / GPIO44 RX) | USB-Serial-JTAG FIFO (`0x6003_8000`), via `esp-println/jtag-serial` | **yes** — first statement of `run_uart_node` |
| `microfips-esp32s3-usb` | the USB-Serial-JTAG peripheral itself | the same FIFO | **no, by design** |
| `microfips-esp32s3` (wifi) / `-espnow` / `-relay-ap` / `-mdns-spike` | Wi-Fi/UDP, ESP-NOW, or mDNS | USB-Serial-JTAG FIFO | yes (through `run_tasks`, `relay_ap`, the spike's `main`) |
| `microfips-esp32s3-espnow-gw` (USB-bridged gateway) | USB serial to the host bridge | the same FIFO | **no** — the AGENTS.md "Console" rule; the bridge's length-prefix resync skips ROM boot text and panics, not interleaved log lines |
| `microfips-esp32c3-uart` | UART0 | USB-Serial-JTAG FIFO | **yes** |
| `microfips-esp32c3-usb` | the USB-Serial-JTAG peripheral itself | the same FIFO | **no, by design** (that image also uses `panic_blink!`, not `panic_blink_print!`) |
| `microfips-esp32c3-wifi` / `-esp-now` | Wi-Fi/UDP, ESP-NOW | USB-Serial-JTAG FIFO | yes (through `run_tasks`) |
| `microfips-esp32` (uart) | UART0 (GPIO1 TX / GPIO3 RX) | **UART0** — ROM `uart_tx_one_char` (`0x4000_9200`) | **no, by design** (see below) |
| `microfips-esp32-wifi` / `-espnow` / `-espnow-wifi-gw` | Wi-Fi/UDP, ESP-NOW | UART0 | yes (through `run_tasks` / `espnow_wifi_gateway`) |

## Where the channel is selected

`crates/microfips-esp-transport/Cargo.toml`: the `esp32s3` and `esp32c3` features select
`esp-println/jtag-serial`; the `esp32` feature selects `esp-println/auto`. In esp-println
0.18.0 (the workspace pin) `auto` resolves to

* the USB-Serial-JTAG printer on chips that have that peripheral (`src/lib.rs:159-198`), and
* `uart_printer` on chips that do not — the `auto_printer` arm documented as *"models that
  only have UART"* (`src/lib.rs:227-229`), which for `esp32` pushes every byte through the
  ESP32 ROM routine `uart_tx_one_char` (`src/lib.rs:363-380`, address literal at `:366`).

(The S3's own printer is `serial_jtag_printer` and writes the FIFO at `0x6003_8000`,
`src/lib.rs:272`; the C3's at `0x6004_3000`, `:252`.)

That is why the classic **ESP32 `uart` image stays console-free**: there is no USB-Serial-JTAG
peripheral on ESP32-D0WD, so the only available printer is UART0 — the very peripheral whose
TX pin (GPIO1) carries the FIPS frames. Installing a backend there would interleave log text
into the frame stream. The chip's `wifi`/`esp-now` images are unaffected: their transport is
UDP over Wi-Fi (or ESP-NOW), so UART0 is a free console and they log as usual.

On the C3 the install is `log`'s racy variant (`set_logger_racy` / `set_max_level_racy`),
because `riscv32imc-unknown-none-elf` has no compare-and-swap; on the S3 and ESP32 targets
pointer-width atomics exist and the ordinary `log::set_logger` path is compiled. That split
lives in `crates/microfips-esp-transport/src/logger.rs`.

## Verifying an image (link/compile-time, no board needed)

```sh
source ~/export-esp.sh
cargo +esp build -p microfips-esp32s3 --release --target xtensa-esp32s3-none-elf \
  -Zbuild-std=core,alloc --no-default-features --bin microfips-esp32s3-uart
ELF=target/xtensa-esp32s3-none-elf/release/microfips-esp32s3-uart

# 1. which backend symbols are linked (expect logger::init + log::set_logger on S3,
#    and *no* _racy symbols: those are the riscv32imc path)
llvm-nm --demangle "$ELF" | grep -E 'esp_transport::logger|set_logger'

# 2. reachability: the first call inside the entry point must be logger::init,
#    heap::init second
llvm-objdump -d --demangle "$ELF" | grep -A12 '<microfips_esp32s3::run::run_uart_node'

# 3. which printer esp-println linked (S3: 0x6003_8000 USB-Serial-JTAG FIFO;
#    ESP32: 0x4000_9200 ROM uart_tx_one_char)
llvm-objdump -d --demangle "$ELF" | grep -E '6003_8000|40009200'
```

`llvm-nm`/`llvm-objdump` come from `rustup component add llvm-tools-preview`; the xtensa
binutils are not installed on the balloon worker host. Evidence for the S3/ESP32 decision
(2026-09-18, card `t_fb69067d`) lives in
`~/reports/balloon/t_fb69067d/evidence/`.
