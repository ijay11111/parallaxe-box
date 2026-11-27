#!/usr/bin/env python3
"""Check `THIRD_PARTY_PLUGINS.md` against the manifests it claims to describe.

Every plugin shipped in this repository must appear in the public notice table
with a truthful author, source and license, and with the SHA-256 of the exact
`manifest.json` bytes. This tool fails closed: a missing row, an extra row, a
field that disagrees with the manifest, or a stale digest is an error.

It also validates every manifest with the same rules `pack_plugin.py` uses, so a
broken sample cannot hide behind a matching notice row.

Usage:
    python tools/verify_plugins.py [--root PATH] [--notices PATH]

Exit codes: 0 all consistent, 1 any inconsistency, 2 unusable inputs.
No network access, no writes.
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

from pack_plugin import MANIFEST_NAME, PluginError, parse_manifest

COLUMNS = [
    "插件 ID",
    "名称",
    "作者",
    "许可证",
    "来源",
    "用途",
    "权限",
    "manifest 路径",
    "sha256",
]
EMPTY_PERMISSIONS = {"", "无", "none", "None", "-"}


def clean(cell: str) -> str:
    return cell.strip().strip("`").strip()


def read_rows(notices: Path) -> list[dict[str, str]]:
    if not notices.is_file():
        raise SystemExit(f"error: {notices} does not exist")
    lines = notices.read_text(encoding="utf-8").splitlines()
    start = None
    for index, line in enumerate(lines):
        if not line.strip().startswith("|"):
            continue
        cells = [clean(cell) for cell in line.strip().strip("|").split("|")]
        if cells and cells[0] == COLUMNS[0]:
            if cells != COLUMNS:
                raise SystemExit(
                    "error: unexpected column layout in notice table\n"
                    f"  expected {COLUMNS}\n  found    {cells}"
                )
            start = index + 1
            break
    if start is None:
        raise SystemExit("error: plugin table not found in THIRD_PARTY_PLUGINS.md")

    rows: list[dict[str, str]] = []
    for line in lines[start:]:
        stripped = line.strip()
        if not stripped.startswith("|"):
            if rows:
                break
            continue
        cells = [clean(cell) for cell in stripped.strip("|").split("|")]
        if not cells or all(set(cell) <= set("-: ") for cell in cells):
            continue
        if len(cells) != len(COLUMNS):
            raise SystemExit(f"error: row has {len(cells)} cells, expected {len(COLUMNS)}: {line}")
        rows.append(dict(zip(COLUMNS, cells)))
    if not rows:
        raise SystemExit("error: plugin table has no entries")
    return rows


def discover(root: Path) -> list[Path]:
    sample_root = root / "plugins"
    if not sample_root.is_dir():
        raise SystemExit(f"error: {sample_root} does not exist")
    found = sorted(sample_root.rglob(MANIFEST_NAME))
    if not found:
        raise SystemExit(f"error: no manifests under {sample_root}")
    return found


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--notices", type=Path, default=None)
    args = parser.parse_args(argv)

    root = args.root.resolve()
    notices = args.notices or (root / "THIRD_PARTY_PLUGINS.md")
    try:
        rows = read_rows(notices)
    except OSError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    problems: list[str] = []
    by_path: dict[str, dict[str, str]] = {}
    for row in rows:
        relative = row["manifest 路径"].replace("\\", "/")
        if relative in by_path:
            problems.append(f"duplicate notice row for {relative}")
            continue
        by_path[relative] = row
        if not relative.startswith("plugins/"):
            problems.append(f"{relative}: notice rows must live under plugins/")
        if not (root / relative).is_file():
            problems.append(f"{relative}: listed in the notice table but missing on disk")

    seen_ids: dict[str, str] = {}
    for manifest_path in discover(root):
        relative = manifest_path.relative_to(root).as_posix()
        row = by_path.get(relative)
        if row is None:
            problems.append(f"{relative}: manifest has no row in THIRD_PARTY_PLUGINS.md")
            continue
        raw = manifest_path.read_bytes()
        try:
            manifest = parse_manifest(raw)
        except PluginError as error:
            problems.append(f"{relative}: invalid manifest: {error}")
            continue

        plugin_id = manifest["id"]
        if plugin_id in seen_ids:
            problems.append(
                f"{relative}: id {plugin_id!r} is also used by {seen_ids[plugin_id]}"
            )
        seen_ids[plugin_id] = relative

        if row["插件 ID"] != plugin_id:
            problems.append(f"{relative}: id {row['插件 ID']!r} != manifest {plugin_id!r}")
        for column, field in (
            ("名称", "name"),
            ("作者", "author"),
            ("许可证", "license"),
            ("来源", "source"),
        ):
            if row[column] != manifest[field]:
                problems.append(
                    f"{relative}: {column} {row[column]!r} != manifest {manifest[field]!r}"
                )
        permissions = ",".join(manifest.get("permissions", []))
        declared = row["权限"].replace(" ", "").replace("，", ",")
        if declared in EMPTY_PERMISSIONS:
            declared = ""
        if declared != permissions:
            problems.append(
                f"{relative}: permissions {row['权限']!r} != manifest {permissions!r}"
            )
        digest = hashlib.sha256(raw).hexdigest()
        if row["sha256"].lower() != digest:
            problems.append(
                f"{relative}: sha256 {row['sha256']} != actual {digest}"
            )
        if not row["用途"]:
            problems.append(f"{relative}: 用途 must explain what the plugin does")

    if problems:
        for problem in problems:
            print(f"FAIL {problem}", file=sys.stderr)
        print(
            f"{len(problems)} problem(s); notices are not consistent with manifests",
            file=sys.stderr,
        )
        return 1

    print(f"OK {len(seen_ids)} plugin(s) consistent with {notices.name}")
    for plugin_id, relative in sorted(seen_ids.items()):
        print(f"  {plugin_id}  {relative}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
