#!/bin/sh
set -eu
cd /build
cat > kernel.elf
riscv64-linux-gnu-readelf -lW kernel.elf | awk '$1 == "LOAD" {print $4, $6}' > kernel.segments
# The linker places _start at the first byte of the first PT_LOAD segment.
read -r kernel_load mem_size < kernel.segments
kernel_load=$(printf '0x%x' "$kernel_load")
riscv64-linux-gnu-objcopy -O binary kernel.elf kernel.bin
firmware_reg=$(fdtget -t u devterm.dtb /reserved-memory/opensbi@40000000 reg)
read -r firmware_base firmware_size <<EOF
$firmware_reg
EOF
firmware_end=$((firmware_base + firmware_size))
test "$(wc -c < fw_dynamic.bin)" -le "$firmware_size"
# Include zero-filled segments when checking the space below the boot DTB.
while read -r address mem_size; do
    if [ "$((address))" -lt "$firmware_end" ] || [ "$((address + mem_size))" -gt $((0x48000000)) ]; then
        echo "Kernel segment at $address (size $mem_size) overlaps reserved boot memory." >&2
        exit 1
    fi
done < kernel.segments
# The full U-Boot DT includes packaging templates with placeholder node names.
fdtput -r devterm.dtb /binman /__symbols__
lcd=/reserved-memory/framebuffer@7fd00000
case "${LCD_HBP:=150}" in
    14[6-9]|15[0-4]) fdtput -t u devterm.dtb "$lcd" clockwork,lcd-hbp "$LCD_HBP" ;;
    *) echo "LCD_HBP must be an integer from 146 to 154" >&2; exit 1 ;;
esac
case "${LCD_SCAN_WIDTH:=484}" in
    48[0-8]) fdtput -t u devterm.dtb "$lcd" clockwork,lcd-scan-width "$LCD_SCAN_WIDTH" ;;
    *) echo "LCD_SCAN_WIDTH must be an integer from 480 to 488" >&2; exit 1 ;;
esac
if [ "${1:-sd}" = usb ]; then
    # FEL skips SPL, which normally adds the detected DRAM bank.
    fdtput -p -c devterm.dtb /memory@40000000
    fdtput -t s devterm.dtb /memory@40000000 device_type memory
    fdtput -t x devterm.dtb /memory@40000000 reg 40000000 40000000
    test "$(wc -c < devterm.dtb)" -le $((0x48100000 - 0x48000000))
    riscv64-linux-gnu-ld --no-relax -Ttext=0x48100000 -e _start \
        --defsym="kernel_load=$kernel_load" -o fel-entry.elf fel-entry.o
    riscv64-linux-gnu-objcopy -O binary fel-entry.elf fel-entry.bin
    echo "$kernel_load" > kernel.addr
    exec tar -cf - fw_dynamic.bin kernel.bin kernel.addr devterm.dtb fel-entry.bin
fi
# Match board/sunxi/board-riscv.c's spl_mmc_get_uboot_raw_sector().
# sunxi-spl.bin is already padded to the length in its eGON header.
spl_bytes=$(wc -c < sunxi-spl.bin)
test $((spl_bytes % 512)) -eq 0
spl_sectors=$((spl_bytes / 512))
if [ "$spl_sectors" -lt 64 ]; then spl_sectors=64; fi
fit_sector=$((16 + spl_sectors))
# External FIT data keeps SPL's metadata buffer small.
sed "s/@KERNEL_LOAD@/$kernel_load/g" boot.its > kernel.its
mkimage -E -B 0x200 -f kernel.its boot.itb >&2
# D1 BROM reads SPL at sector 16. The FIT follows the padded SPL.
dd if=sunxi-spl.bin of=sd.img bs=512 seek=16 status=none
dd if=boot.itb of=sd.img bs=512 seek="$fit_sector" conv=notrunc status=none
cat sd.img
