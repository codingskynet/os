# Single-file freestanding Rust programs for the initarfs rootfs.
#
# Parent (userland/Makefile) passes ROOTFS_BIN and ARCH.

ARCH ?= riscv64gc-unknown-none-elf
ROOTFS_BIN ?= $(error ROOTFS_BIN is required; build via `make -C userland build`)
ULIB_RLIB ?= ../../artifacts/userland/libulib.rlib

PROGRAMS := $(basename $(wildcard *.rs))
BINARIES := $(addprefix $(ROOTFS_BIN)/,$(PROGRAMS))

RUSTFLAGS ?= -C opt-level=s -C panic=abort

.PHONY: install clean FORCE

install: $(BINARIES)

# Real file rule (not a phony) so parallel builds order binaries after the
# rlib. FORCE delegates to the sub-make every time; it only rewrites the rlib
# when lib sources changed, so binaries relink only when needed.
$(ULIB_RLIB): FORCE
	+$(MAKE) -C lib build \
		ULIB_RLIB="$(abspath $(ULIB_RLIB))" \
		ARCH="$(ARCH)"

$(ROOTFS_BIN)/%: %.rs $(ULIB_RLIB)
	mkdir -p $(ROOTFS_BIN)
	rustc --edition=2024 --target=$(ARCH) $(RUSTFLAGS) \
		--extern ulib=$(ULIB_RLIB) -o $@ $<

clean:
	+$(MAKE) -C lib clean ULIB_RLIB="$(abspath $(ULIB_RLIB))"
	rm -f $(BINARIES)

FORCE:
