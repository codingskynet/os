ROOT := ../..
TARGET := riscv64gc-unknown-none-elf
PROFILE := $(if $(filter 1,$(DEBUG)),debug,release)
ELF := $(ROOT)/target/$(TARGET)/$(PROFILE)/board-devterm-r01
OUT ?= $(ROOT)/artifacts/devterm-r01
IMAGE := $(OUT)/sd.img
XFEL ?= $(if $(wildcard $(ROOT)/artifacts/tools/xfel),$(abspath $(ROOT)/artifacts/tools/xfel),xfel)
PACK = docker run --rm -i -e LCD_HBP -e LCD_SCAN_WIDTH os-devterm-r01
LCD_HBP ?= 150
LCD_SCAN_WIDTH ?= 484
export LCD_HBP LCD_SCAN_WIDTH

.PHONY: image build firmware-image usb-boot
image: build firmware-image
	@set -eu; tmp=$$(mktemp "$(OUT)/sd.img.XXXXXX"); \
		trap 'rm -f "$$tmp"' EXIT; \
		$(PACK) < "$(ELF)" > "$$tmp"; \
		mv "$$tmp" "$(IMAGE)"
	@echo "Built $(IMAGE)"

firmware-image:
	docker build -q -t os-devterm-r01 firmware

usb-boot: build firmware-image
	@set -eu; tmp=$$(mktemp -d); trap 'rm -rf "$$tmp"' EXIT; \
		$(PACK) usb < "$(ELF)" > "$$tmp/boot.tar"; \
		tar -xf "$$tmp/boot.tar" -C "$$tmp"; \
		sh usb-boot.sh "$(XFEL)" "$$tmp"

build:
	+$(MAKE) -C $(ROOT) build BOARD=devterm-r01 ARCH=$(TARGET) BUILD=build-devterm-r01
	mkdir -p "$(OUT)"
	rust-objcopy -O binary "$(ELF)" "$(OUT)/kernel.bin"
