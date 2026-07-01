#!/usr/bin/env bash
set -euo pipefail

echo "==> Installing system dependencies (QEMU)..."
if command -v brew &>/dev/null; then
    brew install qemu
elif command -v apt &>/dev/null; then
    sudo apt update && sudo apt install -y qemu-system-riscv64
elif command -v dnf &>/dev/null; then
    sudo dnf install -y qemu-system-riscv
elif command -v pacman &>/dev/null; then
    sudo pacman -S --noconfirm qemu-system-riscv
else
    echo "WARN: No supported package manager found. Please install QEMU manually."
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