#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
target=riscv32imac-unknown-none-elf
out="$root/artifacts/tangnano"
mkdir -p "$out/rootfs"
rsync -a --delete userland/rootfs/ "$out/rootfs/"
mkdir -p "$out/rootfs/bin" "$out/rootfs/etc"
if [ ! -f userland/ports/micropython/ports/os/Makefile ]; then
    make ports-prepare PORTS=micropython
fi
cross=${RV32_CROSS_COMPILE:-$root/../TangNano-9K-example/linux-spi-lcd/vendor/xpack-riscv-none-elf-gcc-15.2.0-1/bin/riscv-none-elf-}
if ! command -v "${cross}gcc" >/dev/null 2>&1; then
    echo 'Set RV32_CROSS_COMPILE to a RISC-V GCC toolchain prefix (with target headers and libgcc).' >&2
    exit 1
fi
make -C userland/ports/micropython/ports/os -j"${JOBS:-4}" \
    ARCH="$target" BUILD=build-rv32 CROSS_COMPILE="$cross" \
    install ROOTFS_BIN="$out/rootfs/bin"
# Rebuild core/alloc without C: the CPU has I/M/A and Sv32, but no C/F/D.
common="-C target-feature=-c,-zca -C relocation-model=static -C code-model=medium -C link-arg=--no-relax"
printf '%s\n' "${TANG_BOARD_DESCRIPTION:-Tang Nano 9K: VexRiscv RV32IMA, Sv32, 27MHz, 8MiB PSRAM}" > "$out/rootfs/etc/board"
COPYFILE_DISABLE=1 tar --format ustar -cf "$root/artifacts/initarfs" -C "$out/rootfs" .
RUSTFLAGS="$common -C link-arg=-Map=$out/kernel.map" \
 cargo build -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem \
 --release --target "$target" -p board-litex-vexriscv --bin board-litex-vexriscv
cp "target/$target/release/board-litex-vexriscv" "$out/kernel.elf"
sysroot=$(rustc --print sysroot)
host=$(rustc -vV | sed -n 's/^host: //p')
"$sysroot/lib/rustlib/$host/bin/llvm-objcopy" -O binary "$out/kernel.elf" "$out/kernel.bin"
python3 - "$out" <<'PY'
import binascii, pathlib, struct, sys
out = pathlib.Path(sys.argv[1])
for elf in [out/'kernel.elf', *sorted((out/'rootfs/bin').iterdir())]:
    data = elf.read_bytes()
    assert data[:5] == b'\x7fELF\x01', f'{elf}: expected ELF32'
    flags = struct.unpack_from('<I', data, 36)[0]
    assert flags & 0xf == 0, f'{elf}: expected uncompressed RV32 soft-float ABI'
payload = (out/'kernel.bin').read_bytes()
assert 32 <= len(payload) <= 4*1024*1024-8
(out/'kernel-flash.bin').write_bytes(struct.pack('<II', len(payload), binascii.crc32(payload))+payload)
print(f'Built {out}/kernel.bin: {len(payload)} bytes; entry 0x40000000')
PY
