# ClockworkPi DevTerm R-01

The R-01 boots into a MicroPython REPL shared by the built-in keyboard, LCD
and USB-C serial console. Complete the [host setup](../../README.md#run-in-qemu),
start Docker, and run the commands below from the repository root.

## Boot from microSD

```sh
make -C board/devterm-r01 image
```

Unmount the card's partitions and replace `/dev/SD_CARD` with the whole-card
device (Linux `/dev/sdX`, macOS `/dev/rdiskN`). **This overwrites the selected disk.**

```sh
sudo dd if=artifacts/devterm-r01/sd.img of=/dev/SD_CARD bs=1048576
sync
```

Insert the card with the DevTerm powered off, then switch it on.

## Boot from USB

Install [xfel](https://github.com/xboot/xfel) on the host. Remove the microSD,
connect the main USB-C port with a data cable, and power on the DevTerm.
This target loads the image into RAM.

```sh
make -C board/devterm-r01 usb-boot
```

## Console

On macOS, run `ls /dev/cu.usbmodem*` and open the matching port, for example
with `screen /dev/cu.usbmodem101 115200`. The USB console also carries LCD
initialization errors. At the REPL, `import sys; sys.exit(0)` powers off the
board. To reboot, enter `import machine; machine.reset()`.

## Hardware

| Component | R-01 configuration | Documentation |
| --- | --- | --- |
| CPU and memory | Allwinner D1, XuanTie C906, 1 GiB RAM on the v3.14 mainboard. The kernel uses Sv39 paging and SBI timer calls. | [Core schematic][core] · [Mainboard schematic][mainboard] · [C906 manual, chapters 5, 7, 9 and section 15.1.7][c906] · [SBI TIME][sbi-time] |
| Display | ICNL9707 panel on four-lane MIPI DSI. The 480 × 1280 XRGB8888 framebuffer is rotated for a 1280 × 480 console. | [DE2.0][de] · [D1-H manual][d1] · [ICNL9707 datasheet][chipone] |
| Power and backlight | AXP228 panel rails on TWI0. PWM4 on PD20 drives the OCP8178 at 20 kHz and 50% duty. | [AXP228 datasheet][axp] · [OCP8178 datasheet][backlight] |
| Keyboard | USB1 EHCI connects to a 480 Mb/s hub and a 12 Mb/s STM32 HID keyboard. | [EHCI 1.0][ehci] · [USB 2.0, chapter 11][usb] · [HID 1.11][hid] |
| Serial console | The main USB-C port exposes a full-speed CDC ACM device (`1209:0001`). UART0 is disabled. | [CDC 1.2][cdc] · [MUSB controller manual][musb] |

The framebuffer occupies 2,457,600 bytes at `0x7fd00000` with a 1,920-byte
stride. EHCI uses 8 KiB at `0x7ff60000`. Both buffers use non-cacheable mappings.

**LCD scan width.** Earlier panel tests showed that a 480-pixel scan clipped the
top four rows of the landscape console. The default `LCD_SCAN_WIDTH=484` keeps
the framebuffer 480 pixels wide and fills the four trailing scan columns with
black. The image build accepts scan widths from 480 to 488.

## Firmware and driver references

| Topic | Sources |
| --- | --- |
| Firmware build and handoff | [Build recipe](firmware/Dockerfile) · [Image packer](firmware/pack.sh) · [Kernel load address](board.ld) · [OpenSBI dynamic handoff][fw-dynamic] |
| microSD boot layout | [U-Boot R-01 configuration][defconfig] · [SPL sector selection][spl-layout] · [ClockworkPi image guide][vendor-image] |
| Board device tree | [Local R-01 DTS](firmware/sun20i-d1-devterm-v3.14.dts) · [ClockworkPi vendor DTS][vendor-dts] · [Linux R-01 DTS][linux-board] · [Linux carrier DTS][linux-carrier] |
| SoC device tree | [U-Boot DTS directory][uboot-dts] · [D1 SoC include][soc] · [Shared peripherals][peripherals] · [Devicetree Specification, chapter 3][dt-spec] |
| DSI protocol and registers | [MIPI DSI][dsi-spec] · [MIPI DCS][dcs-spec] · [A31 manual, MIPI DSI chapter][a31] · [Combo-PHY register map][phy-map] |
| Driver implementations | [Vendor panel sequence][old-panel] · [Backlight patch][old-bl] · [Linux DSI host][old-dsi] · [Linux sunxi MUSB][old-usb] |
| Licenses and credits | [U-Boot SPL license][uboot-license] · [OpenSBI license][opensbi-license] · [Daniel Hepper’s public-domain console font][font] |

[vendor-dts]: https://github.com/clockworkpi/DevTerm/blob/32558daddd09fd019c2d4f69159991f9c4f7e549/Code/patch/d1/board.dts
[uboot-dts]: https://github.com/smaeul/u-boot/tree/2e89b706f5c956a70c989cd31665f1429e9a0b48/arch/riscv/dts
[defconfig]: https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/configs/devterm_r_01_defconfig
[soc]: https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/arch/riscv/dts/sun20i-d1.dtsi
[peripherals]: https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/arch/riscv/dts/sunxi-d1s-t113.dtsi
[core]: https://github.com/clockworkpi/DevTerm/blob/32558daddd09fd019c2d4f69159991f9c4f7e549/Schematics/clockwork_DevTerm_R01_Core_for_Mainboard_V3.14_Schematic.pdf
[mainboard]: https://github.com/clockworkpi/DevTerm/blob/32558daddd09fd019c2d4f69159991f9c4f7e549/Schematics/clockwork_Mainboard_V3.14_V5_Schematic.pdf
[dt-spec]: https://www.devicetree.org/specifications/
[old-panel]: https://github.com/ylyamin/RT-Thread-on-Allwinner-D1H/blob/a3b7b42b3feb6b1c77acc7b6dc15c9f0e8429d7b/rt-thread/bsp/allwinner/libraries/sunxi-hal/hal/source/disp2/disp/lcd/icn9707_480x1280.c
[old-bl]: https://github.com/clockworkpi/DevTerm/blob/32558daddd09fd019c2d4f69159991f9c4f7e549/Code/patch/d1/bl.patch
[old-dsi]: https://github.com/torvalds/linux/blob/v6.12/drivers/gpu/drm/sun4i/sun6i_mipi_dsi.c
[phy-map]: https://github.com/ylyamin/RT-Thread-on-Allwinner-D1H/blob/a3b7b42b3feb6b1c77acc7b6dc15c9f0e8429d7b/rt-thread/bsp/allwinner/libraries/sunxi-hal/hal/source/disp2/disp/de/lowlevel_v2x/de_dsi_type.h
[font]: https://github.com/dhepper/font8x8/blob/8e279d2d864e79128e96188a6b9526cfa3fbfef9/font8x8_basic.h
[uboot-license]: https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/Licenses/README
[opensbi-license]: https://github.com/riscv-software-src/opensbi/blob/057eb10b6d523540012e6947d5c9f63e95244e94/COPYING.BSD
[fw-dynamic]: https://github.com/riscv-software-src/opensbi/blob/057eb10b6d523540012e6947d5c9f63e95244e94/docs/firmware/fw_dynamic.md
[de]: https://cs107e.github.io/readings/Allwinner_DE2.0_Spec_V1.0.pdf
[d1]: https://cs107e.github.io/readings/d1-h_user_manual_v1.0.pdf
[dsi-spec]: https://www.mipi.org/specifications/dsi
[dcs-spec]: https://www.mipi.org/specifications/display-command-set
[chipone]: https://github.com/clockworkpi/DevTerm/blob/32558daddd09fd019c2d4f69159991f9c4f7e549/Schematics/ICNL9707_Datasheet.pdf
[a31]: https://dl.linux-sunxi.org/A31/A3x_release_document/A31/IC/A31%20user%20manual%20V1.1%2020130630.pdf
[backlight]: https://www.orient-chip.com/Public/Uploads/uploadfile2/files/20231016/OCP8178DatasheetV1.9.pdf
[axp]: https://www.enimac.si/images/pdf/AXP228.pdf
[usb]: https://www.usb.org/document-library/usb-20-specification
[musb]: https://www.ti.com/lit/ug/sprugh3a/sprugh3a.pdf
[cdc]: https://www.usb.org/document-library/class-definitions-communication-devices-12
[old-usb]: https://github.com/torvalds/linux/blob/v6.12/drivers/usb/musb/sunxi.c
[ehci]: https://www.intel.com/content/www/us/en/products/docs/io/universal-serial-bus/ehci-specification.html
[hid]: https://www.usb.org/sites/default/files/documents/hid1_11.pdf
[c906]: https://cs107e.github.io/readings/XuanTie-OpenC906-UserManual.pdf
[sbi-time]: https://github.com/riscv-non-isa/riscv-sbi-doc/blob/master/src/ext-time.adoc
[linux-board]: https://github.com/torvalds/linux/blob/v6.16/arch/riscv/boot/dts/allwinner/sun20i-d1-devterm-v3.14.dts
[linux-carrier]: https://github.com/torvalds/linux/blob/v6.16/arch/riscv/boot/dts/allwinner/sun20i-d1-clockworkpi-v3.14.dts
[spl-layout]: https://github.com/smaeul/u-boot/blob/2e89b706f5c956a70c989cd31665f1429e9a0b48/board/sunxi/board-riscv.c
[vendor-image]: https://github.com/clockworkpi/DevTerm/wiki/Create-DevTerm-R01-OS-image-from-scratch
