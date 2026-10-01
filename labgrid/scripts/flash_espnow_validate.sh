#!/usr/bin/env bash
# MFE-002: build, flash, and validate the microFIPS ESP-NOW binary on ESP32-C3.
#
# Encodes the card's success criteria as automated serial-log assertions:
#   1. firmware flashes to the board
#   2. serial shows "ESP-NOW initialized. MAC: xx:xx:xx:xx:xx:xx"
#   3. serial shows "ESP-NOW mesh node ready on channel 1"
#   4. no panic / expect-failure markers in the capture
#   5. LED: 3 fast blinks (~100 ms on/off) — PHYSICAL CHECK, manual (see footer)
#
# STATUS (2026-10-01): NOT YET RUN AGAINST HARDWARE — blocked by
#   (a) MFE-001: the espnow bin does not link yet (esp_netif_init,
#       esp_event_loop_create_default, esp_wifi_init, esp_read_mac undefined;
#       see docs/mfe-002-flash-validate-status-2026-10-01.md)
#   (b) no ESP32-C3 enumerated on this host (no /dev/ttyACM*).
# The build step here intentionally FAILS until (a) is fixed, so this script
# cannot silently validate a stale binary.
#
# Usage:
#   PORT=/dev/ttyACM2 MONITOR_SECS=45 labgrid/scripts/flash_espnow_validate.sh
#
# NOTE: the build uses -Zbuild-std=core,alloc. The invocation in
# docs/microfips-esp32-implementation-plan-2026-07-09.md omits it and fails
# with E0463 ("can't find crate for `core`") because the esp toolchain ships
# no prebuilt riscv32imc std. FIPS-FIX-PLAN.md documents the same.

set -euo pipefail

PORT="${PORT:-/dev/ttyACM2}"
MONITOR_SECS="${MONITOR_SECS:-45}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$ROOT/target/riscv32imc-unknown-none-elf/release/microfips-esp32c3-espnow"
LOG="${LOG:-$ROOT/target/espnow-validate-serial.log}"

fail() { echo "[FAIL] $*" >&2; exit 1; }

echo "== MFE-002 flash+validate: port=$PORT binary=$BIN =="

# ── preflight ────────────────────────────────────────────────────────────────
# espflash resolution: PATH first, then ~/.cargo/bin (this host's default shell
# PATH does not include ~/.cargo/bin even though cargo install puts it there).
ESPFLASH="${ESPFLASH:-$(command -v espflash || true)}"
if [ -z "$ESPFLASH" ] && [ -x "$HOME/.cargo/bin/espflash" ]; then
    ESPFLASH="$HOME/.cargo/bin/espflash"
fi
[ -n "$ESPFLASH" ] || fail "espflash not installed (cargo install espflash) and not in ~/.cargo/bin"
echo "using espflash: $ESPFLASH ($("$ESPFLASH" --version 2>/dev/null || echo version-unknown))"
[ -e "$PORT" ] || fail "serial port $PORT not present — connect the ESP32-C3 board"

# ── build (MFE-001 gate: this is where linking must succeed) ─────────────────
cd "$ROOT"
echo "== building (release, espnow feature) =="
cargo build -p microfips-esp32c3 \
    --target riscv32imc-unknown-none-elf \
    -Zbuild-std=core,alloc \
    --features espnow \
    --bin microfips-esp32c3-espnow \
    --release
[ -f "$BIN" ] || fail "build did not produce $BIN"

# ── flash ────────────────────────────────────────────────────────────────────
echo "== flashing $BIN to $PORT =="
"$ESPFLASH" flash --chip esp32c3 -p "$PORT" "$BIN"

# ── capture boot + init serial output ────────────────────────────────────────
echo "== monitoring $PORT for ${MONITOR_SECS}s =="
mkdir -p "$(dirname "$LOG")"
timeout "$MONITOR_SECS" "$ESPFLASH" monitor -p "$PORT" >"$LOG" 2>&1 || true
echo "== captured $(wc -l <"$LOG") lines to $LOG =="

# ── assertions (card success criteria) ───────────────────────────────────────
checks=0; failed=0
check() { # check <name> <regex>
    checks=$((checks + 1))
    if grep -Eq "$2" "$LOG"; then
        echo "[PASS] $1"
    else
        echo "[FAIL] $1 (pattern: $2)"
        failed=$((failed + 1))
    fi
}

check "serial capture non-empty (boot output observed)" '\S'
check "ESP-NOW initialized with MAC address" \
      'ESP-NOW initialized\. MAC: ([0-9a-f]{2}:){5}[0-9a-f]{2}'
check "mesh node ready on channel 1" \
      'ESP-NOW mesh node ready on channel 1'

neg=0
grep -Eq 'panic|ESP-NOW init failed|Failed to add broadcast peer' "$LOG" && neg=1
checks=$((checks + 1))
if [ "$neg" -eq 0 ]; then
    echo "[PASS] no panic / expect-failure markers"
else
    echo "[FAIL] panic or expect-failure marker found in serial log:"
    grep -En 'panic|ESP-NOW init failed|Failed to add broadcast peer' "$LOG" | head -5
    failed=$((failed + 1))
fi

echo "== $((checks - failed))/$checks automated checks passed =="
if [ "$failed" -ne 0 ]; then
    exit 1
fi

cat <<'EOF'

MANUAL CHECK (cannot be asserted from serial): the on-board LED (GPIO2) must
show 3 fast blinks (~100 ms on / 100 ms off) immediately after the
"ESP-NOW initialized. MAC:" line — see crates/microfips-esp32c3/src/run.rs:78.

Serial log: <LOG>
EOF
exit 0
