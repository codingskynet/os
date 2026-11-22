FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential gcc-riscv64-linux-gnu bison flex libssl-dev libgnutls28-dev \
    ca-certificates curl device-tree-compiler u-boot-tools python3 python3-setuptools \
    swig bc python3-dev file && rm -rf /var/lib/apt/lists/*
ENV CROSS_COMPILE=riscv64-linux-gnu-
WORKDIR /build
RUN curl -fLsS https://codeload.github.com/riscv-software-src/opensbi/tar.gz/057eb10b6d523540012e6947d5c9f63e95244e94 -o opensbi.tar.gz && \
    mkdir opensbi && tar xf opensbi.tar.gz -C opensbi --strip-components=1 && \
    sed -i '/^CONFIG_FDT_SERIAL/d' opensbi/platform/generic/configs/defconfig && \
    make -C opensbi -j4 PLATFORM=generic FW_TEXT_START=0x40000000 && \
    ! grep -q '^CONFIG_FDT_SERIAL=y' opensbi/build/platform/generic/kconfig/.config && \
    cp opensbi/build/platform/generic/firmware/fw_dynamic.bin .
RUN curl -fLsS https://codeload.github.com/smaeul/u-boot/tar.gz/2e89b706f5c956a70c989cd31665f1429e9a0b48 -o uboot.tar.gz && \
    mkdir uboot && tar xf uboot.tar.gz -C uboot --strip-components=1
COPY sun20i-d1-devterm-v3.14.dts uboot/arch/riscv/dts/
RUN cd uboot && make devterm_r_01_defconfig && \
    sed -i 's/^#if !CONFIG_IS_ENABLED(DM_SERIAL)/#if CONFIG_IS_ENABLED(SERIAL) \&\& !CONFIG_IS_ENABLED(DM_SERIAL)/' include/configs/sunxi-common.h && \
    sed -i '/^config SPL_SPRINTF$/a\\tdefault y if TARGET_SUN20I_D1' lib/Kconfig && \
    scripts/config --disable DEBUG_UART --disable SPL_SERIAL --disable SPL_DM_SERIAL --enable SPL_LOAD_FIT_OPENSBI_OS_BOOT && make olddefconfig && \
    ! grep -q '^CONFIG_SPL_SERIAL=y' .config && \
    make -j4 spl/sunxi-spl.bin dts/dt.dtb && \
    cp spl/sunxi-spl.bin ../ && cp dts/dt.dtb ../devterm.dtb
COPY fel-entry.S ./
RUN riscv64-linux-gnu-as -march=rv64imac_zicsr_zifencei -mabi=lp64 -o fel-entry.o fel-entry.S
COPY boot.its pack.sh ./
ENTRYPOINT ["sh", "/build/pack.sh"]
