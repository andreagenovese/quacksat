#!/bin/sh
# Cross-build quacksat for the duck (aarch64 RK3566, Armbian).
# macOS has no aarch64-linux sysroot, so the build runs in a Linux container.
set -eu

cd "$(dirname "$0")/.."

docker run --rm \
    -v "$PWD":/src \
    -v quacksat-cargo-registry:/usr/local/cargo/registry \
    -w /src \
    rust:1 \
    sh -c 'rustup target add aarch64-unknown-linux-gnu >/dev/null && \
           apt-get update -qq && apt-get install -y -qq gcc-aarch64-linux-gnu >/dev/null && \
           CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
           cargo build --release --target aarch64-unknown-linux-gnu'

echo "Built: target/aarch64-unknown-linux-gnu/release/quacksat"
