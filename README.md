# OS

The toy project for implementing OS

## How to Setup & Build & Run

```bash
make setup          # install dependencies including QEMU
make run            # build artifacts/kernel.{elf,debug,bin} + boot on QEMU
make run DEBUG=1    # build artifacts/kernel-debug.* + boot on QEMU
make build          # build the boot ELF in Cargo's target directory
make image          # copy the ELF and create debug + raw binary artifacts
make clean          # remove artifacts
```

Run the same formatting, section, lint, host-test, and Rustdoc checks used by
the project before submitting a change:

```bash
make check
```

## Managing userland ports

External userland sources are pinned in `userland/ports.toml`. Each port records
an immutable upstream commit, a generated checkout below `userland/ports`, and
a reviewable patch series below `userland/patches`. Generated checkouts are
intentionally excluded from this repository. The top-level Makefile delegates
the `ports-*` targets to `userland/Makefile`.

Prepare every checkout by shallow-fetching its pinned commit and applying its
patches. Only the pinned upstream commit is downloaded; upstream history is not
required for the local patch stack:

```bash
make ports-prepare
```

Pass `PORTS` to operate on one or more selected ports:

```bash
make ports-prepare PORTS=micropython
make ports-status PORTS=micropython
```

To modify a port, edit its generated checkout and commit the changes there.
Then export every commit after the pinned revision as a patch series:

```bash
cd userland/ports/micropython
git add <files>
git commit -m "Describe the OS port change"
cd ../../..
make ports-sync PORTS=micropython
make ports-verify PORTS=micropython
```

`ports-sync` refuses dirty checkouts, replaces the stored patch series, and
therefore requires all intended changes to be committed first. `ports-verify`
applies the stored patches to the pinned revision in a temporary checkout and
checks that the resulting Git tree matches the working checkout. Commit both
`userland/ports.toml` and `userland/patches/` when adding or updating a port.

Available commands are:

```bash
make ports-prepare  # shallow-fetch the pinned revision and apply patches
make ports-sync     # export local commits to userland/patches/<port>
make ports-status   # show base, commit, patch, and dirty state
make ports-verify   # verify that the patches reproduce the checkout
make ports-clean    # remove only clean, fully synchronized checkouts
```

`ports-clean` refuses to remove a checkout when it contains uncommitted or
untracked files, or when the stored patch series does not reproduce its current
Git tree. Use `PORTS=micropython` to clean only the selected checkout.
