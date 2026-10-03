#!/usr/bin/env bash
# Cross-build quacksat for the duck's board: Radxa Zero 3 (RK3566,
# aarch64) running Armbian with the Debian 13 (Trixie) userland.
#
# Usage: scripts/cross-build.sh [extra cargo arguments]
# Output: target/aarch64-unknown-linux-gnu/release/quacksat
#
# Needs rustup with the aarch64-unknown-linux-gnu target, zig and
# cargo-zigbuild (on a Mac: brew install rustup zig cargo-zigbuild, then
# rustup toolchain install stable --target aarch64-unknown-linux-gnu).
# No Docker: `zig cc` is the cross linker and brings the glibc stubs, and
# also compiles the one C dependency (ring, under rustls: the agent
# backend's wss:// and the direct backend's https://).
#
# The `.2.31` suffix pins the glibc floor (a cargo-zigbuild feature), as
# microduck's own `cargo board` and quack-nav's build do: unpinned, the
# binary would link against the build host's glibc and could refuse to
# load on the board.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu
GLIBC_FLOOR=2.31

cd "$(dirname "$0")/.."

has_target() {
    [ -d "$(rustc --print sysroot 2>/dev/null)/lib/rustlib/$TARGET" ]
}

# Homebrew's `rust` has no std for other targets; Homebrew's rustup is
# keg-only, so its cargo may not be first on PATH.
if ! has_target && [ -x /opt/homebrew/opt/rustup/bin/rustup ]; then
    PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi
if ! has_target; then
    echo "no Rust std for $TARGET on this PATH (rustc: $(command -v rustc || echo none))" >&2
    echo "  rustup target add $TARGET" >&2
    exit 1
fi
for tool in zig cargo-zigbuild; do
    command -v "$tool" >/dev/null || { echo "$tool is not installed (brew install zig cargo-zigbuild)" >&2; exit 1; }
done

echo "building quacksat for $TARGET, glibc >= $GLIBC_FLOOR ($(rustc --version))"
cargo zigbuild --release -p quacksat --bin quacksat --target "$TARGET.$GLIBC_FLOOR" "$@"

BIN="${CARGO_TARGET_DIR:-target}/$TARGET/release/quacksat"
command -v file >/dev/null && file "$BIN"
if command -v objdump >/dev/null; then
    echo "glibc required: $(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
fi
echo "built $BIN"
