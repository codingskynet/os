ARCH      ?= riscv64gc-unknown-none-elf
LINKER_SCRIPT := src/arch/rv64/kernel.ld

TARGET_DIR := target/$(ARCH)/release
KERNEL_ELF := $(TARGET_DIR)/kernel
KERNEL_IMG := kernel.img

MEMORY     := 64M

RUSTFLAGS := \
	-C code-model=medium \
	-C relocation-model=static \
	-C link-arg=--script=$(LINKER_SCRIPT) \
	-C link-arg=--no-relax

.PHONY: all setup build image run clean fmt clippy typos test check

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
		-m $(MEMORY) \
		-nographic \
		-bios none \
		-kernel $(KERNEL_IMG)

clean:
	rm -f $(KERNEL_IMG)
	cargo clean

fmt:
	./fmt

clippy:
	cargo clippy --target=$(ARCH)

typos:
	typos

test:
	cargo test --lib

check: fmt clippy typos test
