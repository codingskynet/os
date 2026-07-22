ROOTFS_BIN ?= $(error ROOTFS_BIN is required; build via `make -C userland build`)

RUST_PORT := $(abspath ../ports/rust)
TARGET_SPEC := $(RUST_PORT)/os/riscv64gc-unknown-os.json
LINKER_SCRIPT := $(RUST_PORT)/os/os.ld
TARGET_DIR ?= $(abspath ../../artifacts/userland/rust-std-target)
STARTUP_OBJ := $(TARGET_DIR)/startup.o
TARGET_NAME := riscv64gc-unknown-os
PROGRAM := rust-std-demo
ARTIFACT := $(TARGET_DIR)/$(TARGET_NAME)/release/$(PROGRAM)

ZIG ?= zig
RUSTFLAGS := \
	-Z force-unstable-if-unmarked \
	-C code-model=medium \
	-C relocation-model=static \
	-C link-arg=$(STARTUP_OBJ) \
	-C link-arg=--script=$(LINKER_SCRIPT) \
	-C link-arg=--no-relax \
	-C link-arg=--gc-sections \
	-C link-arg=--strip-all \
	-C link-arg=-z \
	-C link-arg=max-page-size=4096

.PHONY: install build clean

install: build
	mkdir -p $(ROOTFS_BIN)
	cp $(ARTIFACT) $(ROOTFS_BIN)/rust-std-demo

build: $(STARTUP_OBJ)
	RUSTC_BOOTSTRAP=1 RUSTFLAGS="$(RUSTFLAGS)" \
		cargo build -Zjson-target-spec --locked --release --manifest-path Cargo.toml \
		--target $(TARGET_SPEC) --target-dir $(TARGET_DIR)
	test -x $(ARTIFACT)

$(STARTUP_OBJ): $(RUST_PORT)/os/startup.S
	mkdir -p $(dir $@)
	ZIG_GLOBAL_CACHE_DIR=$(TARGET_DIR)/zig-cache $(ZIG) cc \
		-c -target riscv64-freestanding -mabi=lp64d \
		-mno-relax -o $@ $<

clean:
	rm -rf $(TARGET_DIR)
