#!/usr/bin/env bash
# Installer/developer build step; never called by a runtime helper.
set -euo pipefail
build_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
destination="${1:-$build_root/scripts/bin/inir-helper}"
helper_binary="${INIR_HELPER_BIN:-}"
if command -v cargo >/dev/null && [[ -f "$build_root/rust/Cargo.toml" ]]; then
    cargo build --manifest-path "$build_root/rust/Cargo.toml" -p inir-helper --release --locked --jobs 2
    helper_binary="${CARGO_TARGET_DIR:-$build_root/rust/target}/release/inir-helper"
fi
if [[ ! -x "$helper_binary" ]] || [[ "$("$helper_binary" --protocol-version)" != 1 ]]; then
    printf '%s\n' 'Cannot deliver inir-helper: install the Rust toolchain plus libudev/xkbcommon development files, or set INIR_HELPER_BIN to a matching prebuilt binary.' >&2
    exit 1
fi
mkdir -p -- "$(dirname -- "$destination")"
install -m755 -- "$helper_binary" "$destination.new-$$"
mv -f -- "$destination.new-$$" "$destination"
