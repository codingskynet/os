#!/usr/bin/env bash
set -euo pipefail

echo "==> Installing system dependencies (QEMU and Zig)..."
if command -v brew &>/dev/null; then
    brew install qemu zig
elif command -v apt &>/dev/null; then
    sudo apt update && sudo apt install -y qemu-system-riscv64 zig
else
    echo "WARN: No supported package manager found. Please install QEMU manually."
fi

if ! command -v zig &>/dev/null; then
    echo "ERROR: Zig is required to build the MicroPython userland port."
    echo "       Install it from https://ziglang.org/download/ and rerun setup."
    exit 1
fi

echo "==> Installing Rust toolchain components..."
rustup component add llvm-tools-preview
cargo install cargo-binutils

echo "==> Installing typos (spell checker)..."
if command -v typos &>/dev/null; then
    echo "  typos already installed ($(typos --version))"
elif command -v cargo &>/dev/null; then
    cargo install typos-cli
else
    echo "WARN: Cargo not found, skipping typos installation."
fi

echo "==> Done! Run 'make run' to build and boot the kernel."
