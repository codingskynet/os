SRC_ROOTFS := rootfs
STAGING := ../artifacts/rootfs
INITARFS := ../artifacts/initarfs
ROOTFS_BIN := $(STAGING)/bin

# Built packages that install into $(ROOTFS_BIN).
SUBDIRS := simple rust-std ports/micropython/ports/os
# Host-side tools built by packages but not installed into the rootfs.
CLEAN_SUBDIRS := $(SUBDIRS) ports/micropython/mpy-cross

ARCH ?= riscv64gc-unknown-none-elf
# The top-level kernel DEBUG setting propagates through MAKEFLAGS. Keep the
# freestanding userland build independent so DEBUG=1 kernel smoke tests do not
# accidentally enable Zig's sanitizer-instrumented C debug mode.
USERLAND_DEBUG ?= 0

.PHONY: staging build initarfs clean ports-prepare ports-sync ports-status ports-verify ports-clean FORCE

# Sync committed static files into the staging rootfs, removing anything stale.
staging: FORCE
	mkdir -p $(STAGING)
	rsync -a --delete $(SRC_ROOTFS)/ $(STAGING)/
	mkdir -p $(ROOTFS_BIN)

# Recursively install packages into staging, building their binaries as needed.
build: staging
	+set -e; for d in $(SUBDIRS); do \
		$(MAKE) -C $$d install \
			ROOTFS_BIN="$(abspath $(ROOTFS_BIN))" \
			ARCH="$(ARCH)" \
			DEBUG="$(USERLAND_DEBUG)"; \
	done

initarfs: $(INITARFS)

$(INITARFS): build
	mkdir -p $(dir $@)
	COPYFILE_DISABLE=1 tar --format ustar -cf $@.tmp -C $(STAGING) .
	cmp -s $@.tmp $@ || mv $@.tmp $@
	rm -f $@.tmp

clean:
	+set -e; for d in $(CLEAN_SUBDIRS); do \
		if [ -f "$$d/Makefile" ]; then \
			$(MAKE) -C $$d clean \
				ROOTFS_BIN="$(abspath $(ROOTFS_BIN))" \
				ARCH="$(ARCH)"; \
		fi; \
	done
	rm -rf $(STAGING) $(INITARFS) $(INITARFS).tmp

FORCE:

# Pass PORTS="micropython ..." to operate on selected ports only.
ports-prepare:
	python3 ../scripts/ports.py prepare $(PORTS)

ports-sync:
	python3 ../scripts/ports.py sync $(PORTS)

ports-status:
	python3 ../scripts/ports.py status $(PORTS)

ports-verify:
	python3 ../scripts/ports.py verify $(PORTS)

ports-clean:
	python3 ../scripts/ports.py clean $(PORTS)
