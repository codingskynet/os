ARCH         		?= riscv64gc-unknown-none-elf
HOST_ARCH			?= $(shell rustc -vV | awk '/^host:/ { print $$2 }')
LINKER_SCRIPT		:= boot/src/arch/rv64/kernel.ld

DEBUG				?= 0
FEATURES			?=

ifneq ($(strip $(FEATURES)),)
FEATURE_FLAGS		:= --features "$(FEATURES)"
else
FEATURE_FLAGS		:=
endif

ifeq ($(DEBUG),1)
PROFILE				:= debug
KERNEL_BASENAME		:= kernel-debug
CARGO_FLAGS			:=
OBJCOPY_FLAGS 		:=
PROFILE_RUSTFLAGS	:= -C opt-level=1 -C debug-assertions=on # opt-level=1 prevents large usage of stack and absolute jump table
else
PROFILE				:= release
KERNEL_BASENAME		:= kernel
CARGO_FLAGS			:= --release
OBJCOPY_FLAGS		:= --strip-all $(if $(filter $(HOST_ARCH), riscv64), --target=$(ARCH))
PROFILE_RUSTFLAGS	:=
endif

TARGET_DIR := target/$(ARCH)/$(PROFILE)
KERNEL_ARTIFACT := $(TARGET_DIR)/kernel
ARTIFACTS_DIR := artifacts
KERNEL_ELF := $(ARTIFACTS_DIR)/$(KERNEL_BASENAME).elf
KERNEL_DEBUG := $(ARTIFACTS_DIR)/$(KERNEL_BASENAME).debug
KERNEL_BIN := $(ARTIFACTS_DIR)/$(KERNEL_BASENAME).bin

MEMORY     := 64M

RUSTFLAGS := \
	-C code-model=medium \
	-C relocation-model=static \
	-C link-arg=--script=$(LINKER_SCRIPT) \
	-C link-arg=--no-relax \
	-C link-arg=--orphan-handling=error

.PHONY: all setup build image run clean fmt clippy typos test doc-check doc-kernel check check-boot-sections

all: build image

setup:
	@echo "==> Installing dependencies..."
	@scripts/setup.sh

build:
	RUSTFLAGS="$(RUSTFLAGS) $(PROFILE_RUSTFLAGS)" cargo rustc -p boot --bin kernel --target=$(ARCH) $(CARGO_FLAGS) $(FEATURE_FLAGS)

$(ARTIFACTS_DIR):
	mkdir -p $@

image: build | $(ARTIFACTS_DIR)
	cp $(KERNEL_ARTIFACT) $(KERNEL_ELF)
	rust-objcopy --only-keep-debug $(KERNEL_ARTIFACT) $(KERNEL_DEBUG)
	rust-objcopy $(OBJCOPY_FLAGS) -O binary $(KERNEL_ARTIFACT) $(KERNEL_BIN)

run: image
	qemu-system-riscv64 \
		-machine virt \
		-m $(MEMORY) \
		-nographic \
		-bios none \
		-kernel $(KERNEL_BIN)

clean:
	rm -rf $(ARTIFACTS_DIR)
	cargo clean

fmt:
	./fmt

clippy:
	cargo clippy -p boot --bin kernel --target=$(ARCH) $(FEATURE_FLAGS)

typos:
	typos

check-boot-sections:
	python3 scripts/check-boot-sections.py

test:
	cargo test -p runtime-test --lib --target=$(HOST_ARCH)

doc-check:
	cargo test --doc -p runtime $(FEATURE_FLAGS) --target=$(ARCH)
	cargo doc --no-deps -p boot --bin kernel $(FEATURE_FLAGS) --target=$(ARCH)

doc-kernel:
	cargo doc --open -p boot --bin kernel --no-deps $(FEATURE_FLAGS) --target=$(ARCH)

check: fmt check-boot-sections clippy typos test doc-check
