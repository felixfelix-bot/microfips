# DQ05-BUILD-XTENSA progress (kanban microfips:t_7d5c2b01)

- Finding: task id is board-scoped (`HERMES_KANBAN_TASK=microfips:t_7d5c2b01`); on
  board `microfips`, not default. Card status = triage. Prior 3 attempts all
  just blocked with "fleet-offload:dq05" — NO real build attempt ever made.
  -> status: ORIENTED
- Finding: rustup override for /home/c03rad0r/repos/microfips already = `esp`.
  esp toolchain rustc 1.93.0-nightly supports xtensa-esp32-none-elf and
  xtensa-esp32s3-none-elf. ldproxy present at ~/.cargo/bin/ldproxy.
  -> status: TOOLCHAIN OK
- Finding: task body's literal commands (`cargo build -p microfips-esp32
  --release`) CANNOT work on this host: (a) no --target => builds for x86_64
  host, where esp-sync's xtensa-lx dep is cfg-gated out => E0433 unresolved
  xtensa_lx; (b) with --target but without -Zbuild-std => E0463 can't find
  crate `core` (esp toolchain ships NO prebuilt xtensa std libs). Commit
  48a7110 in repo history confirms the working invocation uses BOTH
  --target and -Zbuild-std=core,alloc.
  -> status: DIAGNOSED (matches repo history 48a7110)
- Finding: rustc compiles all crate code fine once targeted properly; failure
  is at LINK step: `xtensa-esp32-elf-gcc` not found on PATH. Found GCC at
  ~/.espressif/tools/xtensa-elf/esp-15.2.0_20251204/xtensa-esp-elf/bin/
  (also 13.2.0/14.2.0). ~/export-esp.sh was STALE (dead rustup path).
  -> status: LINKER LOCATED
- Action: fixed env (PATH += espressif xtensa gcc bin, LIBCLANG_PATH),
  rebuilt esp32: SUCCESS (exit 0, Finished in 27.35s). Binary verified:
  target/xtensa-esp32-none-elf/release/microfips-esp32 — ELF 32-bit LSB,
  Tensilica Xtensa, 366,624 bytes; xtensa-esp32-elf-size: text=245971
  data=9604 bss=170616.
  -> status: ESP32-D0WD BUILT
- Action: built esp32s3: SUCCESS (exit 0, Finished in 23.02s). Binaries
  verified: target/xtensa-esp32s3-none-elf/release/{microfips-esp32s3 (1,135,992
  B, wifi default), microfips-esp32s3-uart (622,672 B), microfips-esp32s3-usb
  (591,016 B)} — all ELF 32-bit Tensilica Xtensa.
  -> status: ESP32-S3 BUILT
- Action: added scripts/build-xtensa.sh (reproducible both-chips build wrapper:
  auto-locates Espressif GCC, exports LIBCLANG_PATH, runs the correct cargo
  invocation, prints verified binary paths). Ran it end-to-end: exit 0,
  "Finished" both chips, correct binary listing.
  -> status: SCRIPT VERIFIED
- Action: rewrote stale ~/export-esp.sh (host env file, outside repo) to point
  at ~/.espressif GCC path + rustup clang libs.
  -> status: HOST ENV FIXED
- Action: committed build fix docs+script (commit 0d3f5ba) and pushed to fork.
  -> status: SHIPPED
- Next: REPORT.md, then kanban_complete with artifacts.
  -> status: WRAPPING UP