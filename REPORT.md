# DQ05-BUILD-C3 — REPORT

Task: Build microFIPS ESP32-C3 binary on DQ05.
Repo: /home/c03rad0r/repos/microfips, branch feat/fips-v0-compat.
Result: **BUILD SUCCEEDED** — 3 release binaries produced, exit 0, no compilation
errors, no code changes required.

## 1. Where the build ran

The host this session runs on IS DQ05: `hostname` = `c03rad0r-DQ05proplus`,
`hostname -I` includes 192.168.2.32. The task's target `c03rad0r@192.168.2.12` is a
different machine: `ssh` fails with "No route to host" and `~/.ssh/id_dq05` does
not exist. No SSH hop was needed or attempted — the build ran locally on DQ05.

## 2. The literal command does not work — two independent reasons

    cargo +nightly build -p microfips-esp32c3 --release

1. `+nightly` → `error: toolchain 'nightly-x86_64-unknown-linux-gnu' is not installed`.
   Installed toolchains (`rustup toolchain list`): `stable` (default), `1.94.1`,
   `1.98.0`, `esp`. There is no `nightly` toolchain.
2. Even if a host nightly existed, the command passes no `--target`, so it would
   build `microfips-esp32c3` (a bare-metal no_std ESP32-C3 crate) for the x86-64 host
   and fail immediately at the `esp-hal`/`esp-rtos` layer.

## 3. The command that actually builds it (canonical, verified)

    cd /home/c03rad0r/repos/microfips
    cargo build -p microfips-esp32c3 --release \
      --target riscv32imc-unknown-none-elf -Zbuild-std=core,alloc

Why this shape (all confirmed on this host, matches FIPS-FIX-PLAN.md lines 9-24):
- `cargo` resolves to the `esp` toolchain via the repo's rustup **directory override**
  (`/home/c03rad0r/repos/microfips  ->  esp`). `cargo --version` = 1.93.0-nightly,
  `rustc --version` = 1.93.0-nightly (2b43689c5 2026-01-27). So `+esp` would be the
  correct explicit form; plain `cargo` is already equivalent here and `+nightly` is wrong.
- The `esp` (espup) toolchain ships **no prebuilt riscv32imc std** — only host std
  (`~/.rustup/toolchains/esp/lib/rustlib/` contains only `x86_64-unknown-linux-gnu`).
  Therefore `-Zbuild-std=core,alloc` is **mandatory**, and `rustup target add` cannot
  substitute (rustup refuses component/target ops on the custom `esp` toolchain).
  The task's claim that "target riscv32imc-unknown-none-elf already installed" is
  misleading: the *target* may be known, but its *std* is absent and must be built.
- No env sourcing / xtensa GCC / LIBCLANG_PATH needed for the C3 (riscv) build.

## 4. Build result (real observed output)

    warning: unexpected `cfg` condition value: `noise-xx`   (microfips-protocol, x4)
    warning: unexpected `cfg` condition value: `log`        (microfips-esp-common, x1)
    warning: unexpected `cfg` condition value: `ble`        (microfips-esp32c3 lib, x2)
    warning: `microfips-esp32c3` (lib) generated 2 warnings
        Finished `release` profile [optimized] target(s) in 3m 40s
    EXIT=0

No `error:` lines. The 7 warnings are all pre-existing `unexpected_cfgs` lint
warnings (features `noise-xx`, `log`, `ble` referenced in cfg but not declared),
present on the untouched tree; none block the build. **No compilation errors
needed fixing.**

## 5. Binary paths (the deliverable)

    /home/c03rad0r/repos/microfips/target/riscv32imc-unknown-none-elf/release/microfips-esp32c3-uart
    /home/c03rad0r/repos/microfips/target/riscv32imc-unknown-none-elf/release/microfips-esp32c3-usb
    /home/c03rad0r/repos/microfips/target/riscv32imc-unknown-none-elf/release/microfips-esp32c3-wifi

`target/` is gitignored (`.gitignore:1 /target`), so durable copies were written to:

    /home/c03rad0r/dq05-build-c3-artifacts/microfips-esp32c3-uart   819944 B
    /home/c03rad0r/dq05-build-c3-artifacts/microfips-esp32c3-usb    795888 B
    /home/c03rad0r/dq05-build-c3-artifacts/microfips-esp32c3-wifi  1931720 B
    /home/c03rad0r/dq05-build-c3-artifacts/sha256sums.txt

sha256 (re-verified identical between target dir and the copies):

    2646270939965429e7b29f9678d743f73fc411c273565c77194b0433c4ea14f6  microfips-esp32c3-uart
    1cbe5dec18f94cd339c9acd5c1e8fdfbb99110c20a955a5e23cb615225c76142  microfips-esp32c3-usb
    8bdcaf0de11c3feaac4ac1ca17b192c9480766f8496891c1db630ee518311641  microfips-esp32c3-wifi

Artifact sanity (real `file`/`readelf` output):

    ELF 32-bit LSB executable, UCB RISC-V, RVC, soft-float ABI, statically linked
    Machine: RISC-V
    Entry point: 0x42010072 (uart, usb)  /  0x42020072 (wifi)

These are genuine riscv32imc bare-metal images (entry in the ESP32-C3 IROM/DROM
mapped region, RVC compressed ISA, soft-float ABI = riscv32imc). The three binaries
are the three `[[bin]]` targets of the crate that are enabled by the default
feature set: `uart.rs`, `usb.rs` (no `required-features`), and `wifi.rs`
(`required-features = ["wifi"]`, and `wifi` is the default feature).

## 6. Regression / scope check

- Default build is unchanged by this card: the immediate re-run finished in 0.27s
  (cache hit) with the same hashes → no code was touched.
- `--features espnow` was also exercised:

      cargo build -p microfips-esp32c3 --release --features espnow \
        --target riscv32imc-unknown-none-elf -Zbuild-std=core,alloc
      -> EXIT=101, rust-lld: undefined symbol: esp_netif_init,
         esp_event_loop_create_default, esp_wifi_init, esp_read_mac

  This is the **pre-existing, already-documented** limitation, not a regression:
  FIPS-FIX-PLAN.md lines 73-79 record that `microfips-esp32c3-espnow` still fails to
  link because its raw FFI targets the full ESP-IDF C API, which the esp-radio blob
  stack does not provide; the baseline was explicitly verified to fail identically.
  The `espnow` bin is not part of the default `--release` build, so it does not
  affect this task's deliverable. Fixing it is feature work (rewriting
  `EspNowTransport` against esp-radio's Rust APIs), correctly out of scope here.

## 7. Changes made

None to source code. Only `PROGRESS.md` and `REPORT.md` (worktree-root tracking files)
were written. No compilation errors were encountered, so nothing needed fixing —
the task's "fix compilation errors if any" clause resolved to "none".

## 8. Honest status

- Built: YES, observed EXIT=0 and the three files on disk.
- Verified: YES, `file` + `readelf` + sha256 on both target dir and copies.
- Committed / pushed: see the commit + remote-verify evidence appended below by the
  worker at the time of this report; only a push whose remote SHA was observed is
  claimed (cross-check with `git log --oneline -1` and `git ls-remote fork`).
