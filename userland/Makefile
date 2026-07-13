SRC_ROOTFS := rootfs
STAGING := ../artifacts/rootfs
INITARFS := ../artifacts/initarfs
ROOTFS_BIN := $(STAGING)/bin

# Built packages that install into $(ROOTFS_BIN).
SUBDIRS := simple

ARCH ?= riscv64gc-unknown-none-elf

.PHONY: staging build initarfs ports-prepare ports-sync ports-status ports-verify ports-clean FORCE

# Sync committed static files into the staging rootfs, removing anything stale.
staging: FORCE
	mkdir -p $(STAGING)
	rsync -a --delete $(SRC_ROOTFS)/ $(STAGING)/
	mkdir -p $(ROOTFS_BIN)

# Recursively build packages and install binaries into staging.
build: staging
	+set -e; for d in $(SUBDIRS); do \
		$(MAKE) -C $$d build \
			ROOTFS_BIN="$(abspath $(ROOTFS_BIN))" \
			ARCH="$(ARCH)"; \
	done

initarfs: $(INITARFS)

$(INITARFS): build
	mkdir -p $(dir $@)
	COPYFILE_DISABLE=1 tar --format ustar -cf $@.tmp -C $(STAGING) .
	cmp -s $@.tmp $@ || mv $@.tmp $@
	rm -f $@.tmp

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
