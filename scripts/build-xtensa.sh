#!/usr/bin/env bash
# Build microFIPS Xtensa firmware (ESP32-D0WD + ESP32-S3) with the esp-rs
# Xtensa Rust toolchain.
#
# The `esp` rustup toolchain ships NO prebuilt Xtensa core/alloc crates and
# NO Xtensa linker, so plain `cargo build -p microfips-esp32 --release`
# cannot work on the host. Both must be supplied:
#   - core/alloc: built from source via -Zbuild-std=core,alloc
#   - linker:     xtensa-esp{,s3}-elf-gcc from the Espressif GCC toolchain
#
# Usage:
#   scripts/build-xtensa.sh            # both chips, release
#   scripts/build-xtensa.sh esp32      # D0WD only
#   scripts/build-xtensa.sh esp32s3    # S3 only
# Extra args are forwarded to cargo (e.g. --features ble).
set -euo pipefail

chip="${1:-all}"
shift || true
extra_args=("$@")

# Locate the newest Espressif Xtensa GCC toolchain (espup layout or IDF layout).
idf_tools="${HOME}/.espressif/tools/xtensa-esp-elf"
rustup_esp="${HOME}/.rustup/toolchains/esp"
gcc_bin=""
if [[ -d "$idf_tools" ]]; then
    gcc_bin="$(ls -d "$idf_tools"/esp-*/xtensa-esp-elf/bin 2>/dev/null | sort -V | tail -1)"
elif [[ -d "$rustup_esp/xtensa-esp-elf" ]]; then
    gcc_bin="$(ls -d "$rustup_esp"/xtensa-esp-elf/*/xtensa-esp-elf/bin 2>/dev/null | sort -V | tail -1)"
fi
if [[ -z "$gcc_bin" || ! -x "$gcc_bin/xtensa-esp32-elf-gcc" ]]; then
    echo "error: xtensa-esp32-elf-gcc not found (looked in $idf_tools and $rustup_esp)" >&2
    exit 1
fi
export PATH="$gcc_bin:$PATH"

# libclang for bindgen-based crates (esp-bootloader-esp-idf et al.).
libclang="$(ls -d "$rustup_esp"/xtensa-esp32-elf-clang/*/esp-clang/lib 2>/dev/null | sort -V | tail -1)"
[[ -n "$libclang" ]] && export LIBCLANG_PATH="$libclang"

toolchain="$(rustup show active-toolchain | awk '{print $1}')"
if [[ "$toolchain" != esp* ]]; then
    echo "error: active toolchain is '$toolchain', expected 'esp' (rustup override set esp)" >&2
    exit 1
fi

build() {
    local pkg="$1" target="$2"
    echo "==> cargo build -p $pkg --release -Zbuild-std=core,alloc --target $target ${extra_args[*]:-}"
    cargo build -p "$pkg" --release -Zbuild-std=core,alloc --target "$target" "${extra_args[@]}"
    echo "--> binaries in target/$target/release/:"
    find "target/$target/release" -maxdepth 1 -type f -perm -u+x ! -name '*.d' ! -name '*.rlib' \
        -exec file {} \; | sed 's/^/    /'
}

case "$chip" in
    esp32)   build microfips-esp32   xtensa-esp32-none-elf ;;
    esp32s3) build microfips-esp32s3 xtensa-esp32s3-none-elf ;;
    all)     build microfips-esp32   xtensa-esp32-none-elf
             build microfips-esp32s3 xtensa-esp32s3-none-elf ;;
    *) echo "usage: $0 [esp32|esp32s3|all]" >&2; exit 1 ;;
esac