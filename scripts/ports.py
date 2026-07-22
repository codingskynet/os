#!/usr/bin/env python3
"""Materialize and maintain pinned upstream source trees from ports.toml."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any


PROJECT_ROOT = Path(__file__).resolve().parent.parent
USERLAND_ROOT = PROJECT_ROOT / "userland"
MANIFEST = USERLAND_ROOT / "ports.toml"
SHA1_RE = re.compile(r"^[0-9a-f]{40}$")
NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


class PortsError(RuntimeError):
    pass


@dataclass(frozen=True)
class Port:
    name: str
    url: str
    rev: str
    dest: Path
    patches: Path
    submodules: tuple[str, ...]


def run(args: Sequence[str], cwd: Path | None = None, capture: bool = False) -> str:
    try:
        result = subprocess.run(
            list(args), cwd=cwd, check=True, text=True,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.PIPE if capture else None,
        )
    except FileNotFoundError as error:
        raise PortsError(f"required command not found: {args[0]}") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip()
        suffix = f": {detail}" if detail else ""
        raise PortsError(f"command failed: {' '.join(args)}{suffix}") from error
    return result.stdout.strip() if capture else ""


def parse_manifest() -> dict[str, Any]:
    try:
        import tomllib  # type: ignore[import-not-found]
    except ImportError:
        return parse_manifest_subset(MANIFEST.read_text(encoding="utf-8"))
    with MANIFEST.open("rb") as file:
        return tomllib.load(file)


def parse_manifest_subset(text: str) -> dict[str, Any]:
    """Parse the small TOML subset used here on Python versions before 3.11."""
    data: dict[str, Any] = {"port": []}
    current: dict[str, Any] = data
    for line_number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line == "[[port]]":
            current = {}
            data["port"].append(current)
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_-]*)\s*=\s*(.+)", line)
        if not match:
            raise PortsError(f"unsupported TOML syntax at ports.toml:{line_number}")
        key, raw_value = match.groups()
        if raw_value.startswith("["):
            try:
                value = json.loads(raw_value)
            except json.JSONDecodeError as error:
                raise PortsError(f"unsupported value at ports.toml:{line_number}") from error
            if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
                raise PortsError(f"unsupported value at ports.toml:{line_number}")
        elif re.fullmatch(r'"(?:[^"\\]|\\.)*"', raw_value):
            value: Any = bytes(raw_value[1:-1], "utf-8").decode("unicode_escape")
        elif raw_value in ("true", "false"):
            value = raw_value == "true"
        elif re.fullmatch(r"[0-9]+", raw_value):
            value = int(raw_value)
        else:
            raise PortsError(f"unsupported value at ports.toml:{line_number}")
        if key in current:
            raise PortsError(f"duplicate key {key!r} at ports.toml:{line_number}")
        current[key] = value
    return data


def resolve_inside_userland(value: Any, field: str, name: str) -> Path:
    if not isinstance(value, str) or not value:
        raise PortsError(f"port {name!r}: {field} must be a non-empty string")
    path = (USERLAND_ROOT / value).resolve()
    try:
        path.relative_to(USERLAND_ROOT)
    except ValueError as error:
        raise PortsError(f"port {name!r}: {field} must stay inside userland") from error
    return path


def load_ports(selected: Sequence[str]) -> list[Port]:
    data = parse_manifest()
    if data.get("version") != 1:
        raise PortsError("ports.toml: version must be 1")
    entries = data.get("port")
    if not isinstance(entries, list):
        raise PortsError("ports.toml: at least one [[port]] entry is required")
    ports: list[Port] = []
    seen = set()
    for entry in entries:
        name = entry.get("name") if isinstance(entry, dict) else None
        if not isinstance(name, str) or not NAME_RE.fullmatch(name):
            raise PortsError("each port needs a simple, non-empty name")
        if name in seen:
            raise PortsError(f"duplicate port name: {name}")
        seen.add(name)
        url, rev = entry.get("url"), entry.get("rev")
        if not isinstance(url, str) or not url:
            raise PortsError(f"port {name!r}: url must be a non-empty string")
        if not isinstance(rev, str) or not SHA1_RE.fullmatch(rev):
            raise PortsError(f"port {name!r}: rev must be a lowercase 40-character commit SHA")
        submodules = entry.get("submodules", [])
        if not isinstance(submodules, list) or not all(
            isinstance(path, str)
            and path
            and not Path(path).is_absolute()
            and ".." not in Path(path).parts
            for path in submodules
        ):
            raise PortsError(
                f"port {name!r}: submodules must be relative paths that stay inside the checkout"
            )
        if len(submodules) != len(set(submodules)):
            raise PortsError(f"port {name!r}: duplicate submodule path")
        ports.append(Port(name, url, rev,
                          resolve_inside_userland(entry.get("dest"), "dest", name),
                          resolve_inside_userland(entry.get("patches"), "patches", name),
                          tuple(submodules)))
    if not selected:
        return ports
    unknown = sorted(set(selected) - seen)
    if unknown:
        raise PortsError(f"unknown port(s): {', '.join(unknown)}")
    wanted = set(selected)
    return [port for port in ports if port.name in wanted]


def git_output(port: Port, *args: str) -> str:
    return run(("git", *args), cwd=port.dest, capture=True)


def require_checkout(port: Port) -> None:
    if not (port.dest / ".git").exists():
        raise PortsError(f"{port.name}: checkout is missing; run make ports-prepare")


def require_clean(port: Port) -> None:
    require_checkout(port)
    if git_output(port, "status", "--porcelain"):
        raise PortsError(f"{port.name}: checkout has uncommitted or untracked changes")


def patch_names(port: Port) -> list[str]:
    series = port.patches / "series"
    if not series.exists():
        return []
    names = []
    for raw in series.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            if Path(line).name != line or not line.endswith(".patch"):
                raise PortsError(f"{port.name}: invalid patch name in series: {line}")
            names.append(line)
    if len(names) != len(set(names)):
        raise PortsError(f"{port.name}: duplicate patch in series")
    return names


def apply_patches(port: Port, checkout: Path) -> None:
    names = patch_names(port)
    missing = [name for name in names if not (port.patches / name).is_file()]
    if missing:
        raise PortsError(f"{port.name}: missing patch(es): {', '.join(missing)}")
    extra = sorted(path.name for path in port.patches.glob("*.patch") if path.name not in names)
    if extra:
        raise PortsError(f"{port.name}: patch(es) absent from series: {', '.join(extra)}")
    if names:
        # `git am` creates commits and therefore requires a committer identity.
        # Keep port preparation independent of each developer's global Git
        # configuration and of the intentionally blank identity on CI runners.
        run((
            "git",
            "-c", "user.name=OS port importer",
            "-c", "user.email=ports@localhost",
            "am", "--", *(str(port.patches / name) for name in names),
        ), cwd=checkout)


def prepare_submodules(port: Port) -> None:
    if port.submodules:
        run(("git", "submodule", "update", "--init", "--depth=1", "--", *port.submodules),
            cwd=port.dest)


# Shallow-fetch the pinned revision and apply its stored patch series.
def prepare(port: Port) -> None:
    if port.dest.exists():
        verify(port)
        prepare_submodules(port)
        print(
            f"{port.name}: checkout already exists and is synchronized at "
            f"{port.dest.relative_to(PROJECT_ROOT)}"
        )
        return
    port.dest.parent.mkdir(parents=True, exist_ok=True)
    try:
        run(("git", "init", "--quiet", str(port.dest)))
        run(("git", "remote", "add", "origin", port.url), cwd=port.dest)
        try:
            run(("git", "fetch", "--depth=1", "origin", port.rev), cwd=port.dest)
        except PortsError as error:
            raise PortsError(
                f"{port.name}: the remote could not shallow-fetch pinned commit {port.rev}; "
                "check that the commit exists and the Git server permits fetching it by SHA"
            ) from error
        run(("git", "checkout", "--quiet", "--detach", "FETCH_HEAD"), cwd=port.dest)
        prepare_submodules(port)
        apply_patches(port, port.dest)
    except Exception:
        if port.dest.exists():
            shutil.rmtree(port.dest)
        raise
    print(f"{port.name}: prepared at {port.dest.relative_to(PROJECT_ROOT)}")


# Export checkout commits after the pinned revision as the stored patch series.
def sync(port: Port) -> None:
    require_clean(port)
    run(("git", "merge-base", "--is-ancestor", port.rev, "HEAD"), cwd=port.dest, capture=True)
    commits = git_output(port, "rev-list", "--reverse", f"{port.rev}..HEAD").splitlines()
    for commit in commits:
        tree = git_output(port, "rev-parse", f"{commit}^{{tree}}")
        parent_tree = git_output(port, "rev-parse", f"{commit}^^{{tree}}")
        if tree == parent_tree:
            raise PortsError(f"{port.name}: empty commit cannot be exported: {commit[:12]}")
    with tempfile.TemporaryDirectory(prefix=f"ports-{port.name}-", dir=USERLAND_ROOT) as temporary:
        output = Path(temporary)
        run(("git", "format-patch", "--no-signature", "--subject-prefix=PATCH",
             "--output-directory", str(output), f"{port.rev}..HEAD"),
            cwd=port.dest, capture=True)
        generated = sorted(output.glob("*.patch"))
        for patch in generated:
            contents = patch.read_text(encoding="utf-8")
            contents, replacements = re.subn(
                r"^Subject: \[PATCH(?: [0-9]+/[0-9]+)?\] ",
                "Subject: ",
                contents,
                count=1,
                flags=re.MULTILINE,
            )
            if replacements != 1:
                raise PortsError(f"{port.name}: cannot remove generated patch subject prefix")
            patch.write_text(contents, encoding="utf-8")
        staging = output / "patches"
        staging.mkdir()
        for patch in generated:
            shutil.move(str(patch), staging / patch.name)
        (staging / "series").write_text(
            "".join(f"{patch.name}\n" for patch in generated), encoding="utf-8"
        )
        if port.patches.exists():
            shutil.rmtree(port.patches)
        port.patches.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(staging), port.patches)
    print(f"{port.name}: exported {len(generated)} patch(es)")


# Report the checkout's base, commit count, patch count, and dirty state.
def status(port: Port) -> None:
    require_checkout(port)
    dirty = bool(git_output(port, "status", "--porcelain"))
    head = git_output(port, "rev-parse", "HEAD")
    try:
        run(("git", "merge-base", "--is-ancestor", port.rev, "HEAD"), cwd=port.dest, capture=True)
        commits = git_output(port, "rev-list", "--count", f"{port.rev}..HEAD")
        base = "ok"
    except PortsError:
        commits, base = "?", "mismatch"
    print(f"{port.name}: base={base} head={head[:12]} commits={commits} "
          f"patches={len(patch_names(port))} dirty={'yes' if dirty else 'no'}")


# Reapply stored patches in a temporary checkout and compare the resulting tree.
def verify(port: Port) -> None:
    require_clean(port)
    with tempfile.TemporaryDirectory(prefix=f"ports-verify-{port.name}-", dir=USERLAND_ROOT) as temporary:
        checkout = Path(temporary) / "checkout"
        run(("git", "clone", "--quiet", "--no-checkout", "--no-hardlinks",
             str(port.dest), str(checkout)))
        run(("git", "checkout", "--quiet", "--detach", port.rev), cwd=checkout)
        apply_patches(port, checkout)
        expected = git_output(port, "rev-parse", "HEAD^{tree}")
        actual = run(("git", "rev-parse", "HEAD^{tree}"), cwd=checkout, capture=True)
        if actual != expected:
            raise PortsError(f"{port.name}: patches do not reproduce the checkout tree")
    print(f"{port.name}: verified {len(patch_names(port))} patch(es)")


# Remove a checkout only when it is clean and fully reproduced by stored patches.
def clean(port: Port) -> None:
    verify(port)
    shutil.rmtree(port.dest)
    print(f"{port.name}: removed {port.dest.relative_to(PROJECT_ROOT)}")


COMMANDS = {
    "prepare": prepare,
    "sync": sync,
    "status": status,
    "verify": verify,
    "clean": clean,
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=COMMANDS)
    parser.add_argument("ports", nargs="*", help="port names (default: all)")
    args = parser.parse_args()
    try:
        for port in load_ports(args.ports):
            COMMANDS[args.command](port)
    except PortsError as error:
        print(f"ports: error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
