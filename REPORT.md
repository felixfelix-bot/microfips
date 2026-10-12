# DQ05-BUILD-XTENSA — Full Report (kanban microfips:t_7d5c2b01)

## Verdict

**BOTH BUILDS SUCCEEDED.** microFIPS ESP32 (D0WD) and ESP32-S3 release
binaries built on DQ05 with the Xtensa Rust toolchain (`esp` rustup
toolchain, rustc 1.93.0-nightly). All binaries verified as Tensilica Xtensa
ELF executables.

## Binary paths (deliverable)

ESP32-D0WD (xtensa-esp32-none-elf, release):
    target/xtensa-esp32-none-elf/release/microfips-esp32
      ELF 32-bit LSB executable, Tensilica Xtensa, statically linked
      366,624 bytes (text=245,971 data=9,604 bss=170,616)

ESP32-S3 (xtensa-esp32s3-none-elf, release):
    target/xtensa-esp32s3-none-elf/release/microfips-esp32s3        1,135,992 B (wifi default)
    target/xtensa-esp32s3-none-elf/release/microfips-esp32s3-uart      622,672 B
    target/xtensa-esp32s3-none-elf/release/microfips-esp32s3-usb       591,016 B
      (all ELF 32-bit LSB, Tensilica Xtensa, statically linked, not stripped)

## How they were built (exact commands, all observed in tool output)

    cd /home/c03rad0r/repos/microfips
    rustup override set esp        # already set for this repo

    # ESP32-D0WD
    cargo build -p microfips-esp32 --release \
        -Zbuild-std=core,alloc --target xtensa-esp32-none-elf
    -> Finished `release` profile [optimized] target(s) in 27.35s (exit 0)

    # ESP32-S3
    export PATH="$HOME/.espressif/tools/xtensa-esp-elf/esp-15.2.0_20251204/xtensa-esp-elf/bin:$PATH"
    export LIBCLANG_PATH="$HOME/.rustup/toolchains/esp/xtensa-esp32-elf-clang/esp-20.1.1_20250829/esp-clang/lib"
    cargo build -p microfips-esp32s3 --release \
        -Zbuild-std=core,alloc --target xtensa-esp32s3-none-elf
    -> Finished `release` profile [optimized] target(s) in 23.02s (exit 0)

Both invocations used PATH with the Espressif Xtensa GCC bin prepended (the
D0WD build too; see "Why the literal commands could not work" below).

    # Reproducible wrapper (added): scripts/build-xtensa.sh
    ./scripts/build-xtensa.sh          # builds both, exit 0, lists binaries
    ./scripts/build-xtensa.sh esp32    # D0WD only
    ./scripts/build-xtensa.sh esp32s3  # S3 only

## Why the task body's literal commands could not work (diagnosis)

The card said to run `cargo build -p microfips-esp32 --release` (no
`--target`, no `-Zbuild-std`). On this host that invocation cannot produce
an ESP32 binary — three independent reasons, each observed:

 1. **No `--target`**: cargo builds for the x86_64 host. esp-sync's
    `xtensa_lx` dependency is gated behind `cfg(target_arch = "xtensa")`, so
    it is not linked, but esp-metadata still emits xtensa cfgs from the build
    script → `error[E0433]: failed to resolve ... xtensa_lx` in
    esp-sync-0.2.1/src/lib.rs:130.
 2. **`--target` without `-Zbuild-std`**: the `esp` rustup toolchain ships
    only x86_64-unknown-linux-gnu std libs — NO prebuilt Xtensa core/alloc
    → `error[E0463]: can't find crate for core` (verified:
    ~/.rustup/toolchains/esp/lib/rustlib/ contains only the host triple).
 3. **Missing Xtensa linker on PATH**: rustc compiles everything, then the
    link step fails → `error: linker xtensa-esp32-elf-gcc not found`.
    The binary exists at
    ~/.espressif/tools/xtensa-esp-elf/esp-15.2.0_20251204/xtensa-esp-elf/bin/
    but the host's ~/export-esp.sh pointed at a dead
    ~/.rustup/toolchains/esp/xtensa-esp-elf/... path (died sometime before
    2026-10-12; directory absent).

Repo history already contained the correct recipe (commit 48a7110, and
AGENTS.md): `. export-esp.sh && RUSTUP_TOOLCHAIN=esp cargo build ... --target
xtensa-esp32-none-elf -Zbuild-std=core,alloc`. The only missing piece on
this host was a working `export-esp.sh`, which I rewrote to the live
Espressif toolchain paths (host file, outside the repo).

## Verification (observed, not assumed)

  file(1)      : "ELF 32-bit LSB executable, Tensilica Xtensa" for every binary
  xtensa-esp32-elf-size : text 245,971 / data 9,604 / bss 170,616 (D0WD)
  cargo exit   : 0 for both packages; "Finished `release` profile" for both
  reproducibility: scripts/build-xtensa.sh re-run end-to-end → exit 0, both
                 chips up to date ("Finished ... in 0.23s" incremental), full
                 binary listing printed.

## Code changes (repo)

Only ONE new file: scripts/build-xtensa.sh (executable) — documents and
reproduces the working build in one command; no source changes were needed.
All crates compiled as-is; the "compilation errors" in the task were build
environment faults (target/std/linker), not code bugs.

## Artifacts committed & pushed

  commit 501193b "build(xtensa): add reproducible build-xtensa.sh; fix DQ05 Xtensa env docs"
    - scripts/build-xtensa.sh (new, executable)
    - PROGRESS.md, REPORT.md (work logs)
  branch: feat/fips-v0-compat (stacked on sibling commit e805c3f DQ05-BUILD-C3)
  remote: fork/feat/fips-v0-compat = felixfelix-bot/microfips
  push observed: "e805c3f..501193b  feat/fips-v0-compat -> feat/fips-v0-compat";
  verified in-sync via `git ls-remote` (501193b907f5...) == local HEAD, and
  `git status` shows no ahead/behind.

Binary checksums (sha256):
    ddcbc2a667a97c64769445eaf85bbb545daf6dcabbbf0f81c132805cdca1c905  microfips-esp32 (D0WD)
    ba7ca900140c70b031b86db3319829c2f9c78629a57029641343ab05ad104e91  microfips-esp32s3
    e20dde580ded375b609423b9d8f63a72a9df9d3b311e91bc917fb2f6c84edc47  microfips-esp32s3-uart
    05e6fd21ec87e079ea3603dfa5a37d181a422d607ea0444b67fbd2a639e00c83  microfips-esp32s3-usb

## Notes for next workers

  - `Cargo.lock` is gitignored in this repo, so dependency drift may vary
    between machines; builds were made with the lockfile present locally.
  - Warnings (pre-existing, not fixed, out of scope): 4× unexpected cfg
    `noise-xx` in microfips-protocol (no such feature), 1× unexpected cfg
    `log` in microfips-esp-common/src/dns.rs. These do not block the build.
  - Binaries were not flash-tested on hardware in this task (no board
    attached); binaries are ELF-verified only. Flash validation is a separate
    lane (see docs/mfe-002-flash-validate-status-2026-10-01.md).

## Status

DONE — ESP32-D0WD + ESP32-S3 release binaries built and verified on DQ05;
recipe captured in scripts/build-xtensa.sh; commit pushed to the fork.