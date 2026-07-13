# Freestanding userland runtime (`ulib`): `_start`, panic handler, syscalls.
#
# Parent (simple/Makefile) passes ULIB_RLIB and ARCH.

ARCH ?= riscv64gc-unknown-none-elf
ULIB_RLIB ?= $(error ULIB_RLIB is required; build via `make -C userland build`)

RUSTFLAGS ?= -C opt-level=s -C panic=abort

.PHONY: build clean

build: $(ULIB_RLIB)

$(ULIB_RLIB): lib.rs syscall.rs
	mkdir -p $(dir $@)
	rustc --edition=2024 --target=$(ARCH) --crate-type=rlib --crate-name=ulib \
		$(RUSTFLAGS) -o $@ lib.rs

clean:
	rm -f $(ULIB_RLIB)
