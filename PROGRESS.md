# DQ05-BUILD-C3 progress

- Finding: the task's literal command `cargo +nightly build -p microfips-esp32c3 --release`
  CANNOT work on this host on two counts:
  (1) toolchain `nightly-x86_64-unknown-linux-gnu` is NOT installed (only stable,
      1.94.1, 1.98.0, and the espup `esp` toolchain);
  (2) no `--target` is given, so it would build for the host anyway.
  `rustup show`: active = toolchain `esp` (directory override for this repo),
  esp rustc 1.93.0-nightly (2b43689c5 2026-01-27).
  -> status: RESOLVED (canonical build command used instead, see below).
- Canonical command (FIPS-FIX-PLAN.md line 12): the `esp` toolchain ships NO
  prebuilt riscv std, so `-Zbuild-std=core,alloc` is MANDATORY:
  `cargo build -p microfips-esp32c3 --release --target riscv32imc-unknown-none-elf -Zbuild-std=core,alloc`
  -> status: DONE, EXIT=0, "Finished release profile in 3m 40s". No compilation
  errors; only pre-existing unexpected_cfgs warnings (noise-xx, log, ble).
  files touched: PROGRESS.md, REPORT.md
- Artifacts (target/ is gitignored, so copied out to a durable dir):
  /home/c03rad0r/dq05-build-c3-artifacts/{microfips-esp32c3-uart,usb,wifi}
  + sha256sums.txt. All 3 = genuine ELF32 RISC-V RVC soft-float statics,
  entry 0x42010072 (uart/usb) / 0x42020072 (wifi). Hashes re-verified MATCH
  against the target dir after a settled rebuild.
  -> status: VERIFIED
- Out-of-scope regression check: default build unchanged after this card's work
  (rebuild 0.27s, cache hit, same hashes).
  `--features espnow` still FAILS TO LINK with undefined `esp_netif_init`,
  `esp_event_loop_create_default`, `esp_wifi_init`, `esp_read_mac` — exactly the
  pre-existing KNOWN REMAINING gap documented in FIPS-FIX-PLAN.md lines 73-79
  (espnow bin not covered by the default `--release` build). NOT a regression.
  -> status: DOCUMENTED
- Host check: `hostname` = c03rad0r-DQ05proplus, 192.168.2.32 -> WE ARE ALREADY ON
  DQ05. Spec'd `ssh ... c03rad0r@192.168.2.12` has no route + missing id_dq05 key
  -> not needed; build ran locally.
- Toolchain note: `cargo +nightly` should be `cargo +esp`, but the repo's rustup
  directory override already selects `esp`, so plain `cargo` is correct.

---
## Archived: previous card's PROGRESS.md content (DQ05-GITHUB-AUTH)

- Finding: GitHub auth was NOT broken. `gh auth status` = OK, account felixfelix-bot,
  token valid. Real problem = IDENTITY MISMATCH: git remote `origin` pointed at
  c03rad0r/microfips (upstream parent), which felixfelix-bot has PUSH=no on (403).
  felixfelix-bot/microfips is the bot's own fork with admin+push.
- Action: added remote `fork` = https://github.com/felixfelix-bot/microfips.git,
  pushed feat/fips-v0-compat, set upstream tracking to fork.
- ngit: already installed at /home/c03rad0r/.opencode/bin/ngit.
