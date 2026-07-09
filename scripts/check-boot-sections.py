#!/usr/bin/env python3
"""Check that boot-owned code/data is explicitly placed in init sections."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BOOT_SRC = ROOT / "boot" / "src"

FN_RE = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?'
    r'(?:(?:const|async|unsafe)\s+)*'
    r'(?:extern\s+(?:"[^"]+"\s+)?)?'
    r"fn\s+([A-Za-z_][A-Za-z0-9_]*)\b"
)
STATIC_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\b"
)
ASM_SECTION_RE = re.compile(r"^\s*\.section\s+([^,\s]+)")

ALLOWED_ASM_SECTIONS = {
    ".text.init",
    ".init.text",
    ".init.rodata",
    ".init.data",
    ".init.bss",
}


def has_link_section_attribute(lines: list[str], item_line: int) -> bool:
    index = item_line - 1
    while index >= 0:
        line = lines[index].strip()
        if not line or line.startswith("///") or line.startswith("//!"):
            index -= 1
            continue
        if line.startswith("#["):
            if "link_section" in line:
                return True
            index -= 1
            continue
        return False

    return False


def function_has_body(lines: list[str], item_line: int) -> bool:
    for line in lines[item_line : min(len(lines), item_line + 80)]:
        code = line.split("//", 1)[0]
        if "{" in code:
            return True
        if code.rstrip().endswith(";"):
            return False

    return False


def check_rust_file(path: Path) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    failures: list[str] = []

    for index, line in enumerate(lines):
        fn_match = FN_RE.match(line)
        if fn_match and function_has_body(lines, index):
            if not has_link_section_attribute(lines, index):
                failures.append(
                    f"{path.relative_to(ROOT)}:{index + 1}: fn {fn_match.group(1)} "
                    "is missing #[unsafe(link_section = \".init.text\")]"
                )
            continue

        static_match = STATIC_RE.match(line)
        if static_match and not has_link_section_attribute(lines, index):
            failures.append(
                f"{path.relative_to(ROOT)}:{index + 1}: static {static_match.group(1)} "
                "is missing an explicit #[unsafe(link_section = ...)]"
            )

    return failures


def check_assembly_file(path: Path) -> list[str]:
    failures: list[str] = []
    lines = path.read_text(encoding="utf-8").splitlines()

    for index, line in enumerate(lines):
        match = ASM_SECTION_RE.match(line)
        if match and match.group(1) not in ALLOWED_ASM_SECTIONS:
            allowed = ", ".join(sorted(ALLOWED_ASM_SECTIONS))
            failures.append(
                f"{path.relative_to(ROOT)}:{index + 1}: assembly section "
                f"{match.group(1)} is not one of: {allowed}"
            )

    return failures


def main() -> int:
    failures: list[str] = []

    for path in sorted(BOOT_SRC.rglob("*.rs")):
        failures.extend(check_rust_file(path))

    for path in sorted(BOOT_SRC.rglob("*.s")):
        failures.extend(check_assembly_file(path))

    if failures:
        print("boot section check failed:", file=sys.stderr)
        print(
            "boot/src code is expected to live in reclaimable .init.* sections.",
            file=sys.stderr,
        )
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("boot section check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
