ARCH      ?= riscv64gc-unknown-none-elf
LINKER_SCRIPT := src/arch/rv64/kernel.ld

TARGET_DIR := target/$(ARCH)/release
KERNEL_ELF := $(TARGET_DIR)/kernel
KERNEL_IMG := kernel.img

RUSTFLAGS := -C link-args=--script=$(LINKER_SCRIPT)

.PHONY: all setup build image run clean fmt clippy test check

all: build image

setup:
	@echo "==> Installing dependencies..."
	@scripts/setup.sh

build:
	RUSTFLAGS="$(RUSTFLAGS)" cargo rustc \
		--target=$(ARCH) \
		--release

image: build
	rust-objcopy --strip-all -O binary $(KERNEL_ELF) $(KERNEL_IMG)

run: image
	qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios none \
		-kernel $(KERNEL_IMG)

clean:
	rm -f $(KERNEL_IMG)
	cargo clean

fmt:
	cargo fmt

clippy:
	cargo clippy --target=$(ARCH) --release -- -D warnings

test:
	cargo test --lib

check: fmt clippy test
	cargo check --target=$(ARCH) --release