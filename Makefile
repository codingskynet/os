ARCH         		?= riscv64gc-unknown-none-elf
HOST_ARCH			?= $(shell rustc -vV | awk '/^host:/ { print $$2 }')
LINKER_SCRIPT		:= src/arch/rv64/kernel.ld

DEBUG				?= 0
FEATURES			?=

ifneq ($(strip $(FEATURES)),)
FEATURE_FLAGS		:= --features "$(FEATURES)"
else
FEATURE_FLAGS		:=
endif

ifeq ($(DEBUG),1)
PROFILE				:= debug
KERNEL_IMG			:= kernel-debug.img
CARGO_FLAGS			:=
OBJCOPY_FLAGS 		:=
PROFILE_RUSTFLAGS	:= -C opt-level=1 # prevent large usage of stack and abosolute jump table
else
PROFILE				:= release
KERNEL_IMG			:= kernel.img
CARGO_FLAGS			:= --release
OBJCOPY_FLAGS		:= --strip-all $(if $(filter $(HOST_ARCH), riscv64), --target=$(ARCH))
PROFILE_RUSTFLAGS	:=
endif

TARGET_DIR := target/$(ARCH)/$(PROFILE)
KERNEL_ELF := $(TARGET_DIR)/kernel

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
	RUSTFLAGS="$(RUSTFLAGS) $(PROFILE_RUSTFLAGS)" cargo rustc --target=$(ARCH) $(CARGO_FLAGS) $(FEATURE_FLAGS)

image: build
	rust-objcopy $(OBJCOPY_FLAGS) -O binary $(KERNEL_ELF) $(KERNEL_IMG)

run: image
	qemu-system-riscv64 \
		-machine virt \
		-m $(MEMORY) \
		-nographic \
		-bios none \
		-kernel $(KERNEL_IMG)

clean:
	rm -f kernel.img kernel-debug.img
	cargo clean

fmt:
	./fmt

clippy:
	cargo clippy --target=$(ARCH) $(FEATURE_FLAGS)

typos:
	typos

test:
	cargo test --lib $(FEATURE_FLAGS) --target=$(HOST_ARCH)

check: fmt clippy typos test
