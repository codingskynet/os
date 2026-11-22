#!/bin/sh
set -eu
xfel=$1
dir=$2
if ! command -v "$xfel" >/dev/null; then
    echo "Install xfel or pass XFEL=/path/to/xfel." >&2
    exit 1
fi
version=$("$xfel" version)
case "$version" in
    *'0x00185900(D1/F133)'*) echo "$version" ;;
    *) echo "D1 FEL device unavailable. Power on without an SD card and connect USB-C. $version" >&2; exit 1 ;;
esac
"$xfel" ddr d1
upload() {
    file=$dir/$2
    size=$(wc -c < "$file")
    echo "Uploading $2 to $1 ($((size)) bytes)"
    "$xfel" write "$1" "$file" > "$dir/xfel.log" || { cat "$dir/xfel.log" >&2; exit 1; }
    "$xfel" read "$1" "$((size))" "$file.readback" > "$dir/xfel.log" || { cat "$dir/xfel.log" >&2; exit 1; }
    cmp "$file" "$file.readback"
}
upload 0x40000000 fw_dynamic.bin
upload "$(cat "$dir/kernel.addr")" kernel.bin
upload 0x48000000 devterm.dtb
upload 0x48100000 fel-entry.bin
echo "RAM readback verified. Starting OpenSBI."
"$xfel" exec 0x48100000
