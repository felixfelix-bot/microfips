# FIPS-FIX-PLAN — ESP32-C3 Rust build fixes (executed 2026-09-12)

Task: "P3: Fix FIPS ESP32-C3 Rust build — 3 code bugs", dispatched against
`~/repos/microfips-upstream/`. That path does not exist on this host; the actual
repo is `~/repos/microfips` (branch `feat/fips-v0-compat`, origin =
github.com/c03rad0r/microfips). This file records what was actually found and
changed there, and where execution deviated from the task spec and why.

## Build command (authoritative)

```
cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf -Zbuild-std=core,alloc
```

- The `esp` toolchain (espup, selected by rustup directory override for this repo)
  ships NO prebuilt riscv/xtensa std — only host std. `-Zbuild-std=core,alloc` is
  therefore mandatory for every cross build in this workspace (all AGENTS.md
  build commands already use it). `rustup target add` cannot help: `esp` is a
  custom toolchain and rustup refuses component/target operations on it.
- No environment sourcing is needed for the C3 (riscv) build — no xtensa GCC,
  no LIBCLANG_PATH required. Verified in a clean `env -i` shell.
- Xtensa builds (esp32/esp32s3) additionally need `xtensa-esp32s3-elf-gcc`, which
  is NOT installed on this host (only the esp rustc + esp-clang libs exist).
  Canonical xtensa builds run on the `/home/ubuntu` host per AGENTS.md.

## Task-vs-reality matrix

| Task bug | Task instruction | Actual repo state | Action taken |
|---|---|---|---|
| 1 ATOMICS | add portable-atomic to esp-common, use it in stats.rs | Already done in-tree (commit history): `portable-atomic = "1"` in `microfips-esp-common/Cargo.toml`; `stats.rs` imports `portable_atomic::{AtomicU32, Ordering}` with no cfg conditionals | None needed — verified present |
| 2 CONFIG | add `#[cfg(feature="esp32c3")]` variants with given values | Variants already existed but 3 of the values were wrong vs TRM | Fixed values (see below) |
| 3 LOGGER | add esp-println dep + `esp_println::logger::init_logger(log::LevelFilter::Info)` | esp-println already wired via transport features and a project logger (`logger.rs`, esp-println backend, `set_logger_racy` — required on riscv32imc, which lacks native CAS atomics). Real bugs: espnow runner's init was compiled out by a nonexistent `feature="log"` cfg AND called `logger::init(LevelFilter)` which doesn't match the zero-arg signature; esp32c3's `espnow` feature didn't enable `dep:log`; `ESPNOW_CHANNEL` was private | Fixed the project's own logger path instead of switching to `esp_println::logger::init_logger` (which would regress: plain `log::set_logger` needs atomics riscv32imc doesn't have) |
| 4 .cargo/config.toml | add `[target.riscv32imc-unknown-none-elf]` runner+rustflags | Already present | None (an experiment adding global `[unstable] build-std` was reverted — it breaks host builds of std crates: duplicate lang-item `core`) |

## Register addresses — verified against ESP-IDF v5.4.1 headers (TRM-derived)

Source: `~/esp/esp-idf/components/soc/esp32c3/include/soc/{reg_base.h,soc.h,gpio_reg.h,rtc_cntl_reg.h}`

| Constant | Old (wrong) | New (correct) | ESP-IDF source |
|---|---|---|---|
| UART0_BASE | 0x6004_0000 | 0x6000_0000 | `DR_REG_UART_BASE 0x60000000`, UART0 = base + 0*0x10000 |
| GPIO_FUNC_IN_SEL_BASE | 0x6000_9000 (= IO_MUX, copy-paste from S3) | 0x6000_4154 | `GPIO_FUNC0_IN_SEL_CFG_REG = DR_REG_GPIO_BASE(0x60004000) + 0x154` |
| UART_RX_GPIO_NUM | 19 (USB D− on C3!) | 5 (free ADC pin) | task spec agreed; GPIO18/19 are USB D+/D− |
| RESET_REGISTER | 0x6000_8000 | 0x6000_8000 (unchanged) | `RTC_CNTL_OPTIONS0_REG = DR_REG_RTCCNTL_BASE(0x60008000) + 0x0` |

Task-spec values NOT applied (both contradict the TRM/IDF headers):
- `RESET_REGISTER=0x60007000` → that is the C3 EFUSE base, not RTC_CNTL.
- `UART0_BASE=0x60043000` → that is USB_SERIAL_JTAG (already used correctly as
  such in `control.rs`), not UART0.
- `DEVICE_NAME="microfips-c3"` not applied: existing `"microfips-esp32c3"`
  follows the sibling convention (`microfips-esp32`, `microfips-esp32s3`).
- `DEVICE_NSEC` fixed dev key: already satisfied — `microfips-build` injects the
  fixed all-zeros dev key from `keys.json` (`devices.esp32c3.nsec_hex`) at build
  time via `env!`.

Also: `RESET_REGISTER` cfg gate extended from `any(ble,l2cap,wifi)` to include
`espnow` for ALL chips — `control.rs` (compiled under espnow) imports it; the
previously un-gated C3 variant had silently masked this for espnow builds.

## espnow path — fixed compile bugs, remaining link gap

Fixed (all previously dead code behind a never-true `cfg(feature="log")`):
- `run_espnow_node` now calls `logger::init()` unconditionally (was
  `logger::init(log::LevelFilter::Info)` behind a dead cfg — a latent compile
  error and, worse, silently disabled logging).
- `espnow` feature in `microfips-esp32c3/Cargo.toml` now enables `dep:log`.
- `ESPNOW_CHANNEL` made `pub`.
- Removed the `nvs_flash_init`/`nvs_flash_erase` FFI calls from
  `EspNowTransport::init` — no blob in the esp-radio stack provides them
  (nm-verified) and the working wifi transport initializes the same blobs
  without NVS.

KNOWN REMAINING (pre-existing, out of scope): the `microfips-esp32c3-espnow`
binary still fails to LINK: its raw FFI targets the full ESP-IDF C API
(`esp_netif_init`, `esp_event_loop_create_default`, `esp_wifi_init`,
`esp_read_mac`), which the esp-radio blob stack does not provide. Fixing it
means rewriting `EspNowTransport` against esp-radio's Rust APIs — feature work,
not a bugfix. Baseline (pre-change) verified to fail identically, so this is
not a regression. Default-feature builds (wifi/uart/usb bins) link fine.

## Verification (all actually executed)

| Check | Command | Result |
|---|---|---|
| C3 default build (bins: wifi, uart, usb) | `cargo build -p microfips-esp32c3 --target riscv32imc-unknown-none-elf -Zbuild-std=core,alloc` (also in clean `env -i` shell) | exit 0, 3 binaries produced |
| S3 regression | `cargo check -p microfips-esp32s3 --target xtensa-esp32s3-none-elf` | exit 0 (full binary link needs xtensa GCC — absent on this host; env gap, not code) |
| ESP32 classic regression | `cargo check -p microfips-esp32 --target xtensa-esp32-none-elf` (+ `--features ble`) | exit 0 |
| Host crates unaffected | `cargo check -p microfips-core` | exit 0 |
| espnow feature | build `--features espnow` | lib + shared code compile; espnow bin link fails on pre-existing missing C API (documented above) |
