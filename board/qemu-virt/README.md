# QEMU virt

This RV64 target boots MicroPython on the emulated serial console. After the
[initial setup](../../README.md#run-in-qemu), run one of these from the repository root.

| Command | Purpose |
| --- | --- |
| `make run` | Boot the release kernel |
| `make run DEBUG=1` | Boot with kernel debug assertions |
| `make run FEATURES=smoke` | Boot with the smoke tests enabled |

To exit QEMU from the serial console, press Ctrl-A, then X.
