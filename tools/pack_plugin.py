#!/usr/bin/env python3
"""Deterministic local plugin packaging for the `parallaxe.plugin/v1` protocol.

A plugin is a directory holding exactly one `manifest.json`. This tool adds an
optional distribution form: `<plugin-id>.plugin`, a stored (uncompressed) ZIP
whose only entry is `manifest.json`, plus a `<plugin-id>.plugin.sha256` sidecar
in `sha256sum -c` format.

Determinism: entries are written in a fixed order with a fixed timestamp and no
compression, so packing the same directory twice yields byte-identical output.

The host loader only reads directories. A `.plugin` file therefore has to be
unpacked with `unpack` before it can be enabled; `verify` proves that the bundle
matches its sidecar digest without touching the filesystem.

Validation here mirrors `crates/core/src/plugins.rs`. Rust remains the authority;
this copy exists so that packaging fails closed before a bundle is produced.
Run `python tools/verify_plugins.py` and `cargo test -p parallaxe-core --test
plugin_samples` for the authoritative checks.

No network access, no elevation, no writes outside the requested output paths.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tempfile
import urllib.parse
import zipfile
from pathlib import Path

SCHEMA = "parallaxe.plugin/v1"
MAX_MANIFEST_BYTES = 64 * 1024
MANIFEST_NAME = "manifest.json"
PERMISSIONS = {"report_counts"}
OPERATION_TYPES = {"resolver", "hook", "coverage_badge"}
HOOK_EVENTS = {"start", "complete", "error"}
COMPONENT_TYPES = {
    "heading",
    "badge",
    "callout",
    "markdown",
    "code",
    "progress",
    "table",
    "form",
    "button",
}
ID_RE = re.compile(r"^[a-z0-9.-]{1,128}$")
# Fixed DOS epoch: the earliest timestamp a ZIP entry can carry.
FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)


class PluginError(Exception):
    """Invalid plugin content or bundle."""


def utf8_len(text: str) -> int:
    return len(text.encode("utf-8"))


def valid_text(value: object, maximum: int, where: str) -> None:
    if not isinstance(value, str):
        raise PluginError(f"{where} must be a string")
    if not value.strip():
        raise PluginError(f"{where} must not be blank")
    if utf8_len(value) > maximum:
        raise PluginError(f"{where} exceeds {maximum} UTF-8 bytes")
    for char in value:
        if ord(char) < 0x20 and char not in "\n\t":
            raise PluginError(f"{where} contains a control character")


def require_keys(obj: object, expected: set[str], where: str) -> dict:
    if not isinstance(obj, dict):
        raise PluginError(f"{where} must be an object")
    unknown = set(obj) - expected
    missing = expected - set(obj)
    if unknown:
        raise PluginError(f"{where} has unknown fields: {', '.join(sorted(unknown))}")
    if missing:
        raise PluginError(f"{where} is missing fields: {', '.join(sorted(missing))}")
    return obj


def validate_source(value: object) -> None:
    valid_text(value, 4096, "source")
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in ("http", "https"):
        raise PluginError("source must be an absolute http or https URL")
    if not parsed.hostname:
        raise PluginError("source must name a host")
    if parsed.username is not None or parsed.password is not None:
        raise PluginError("source must not embed credentials")


def validate_operation(operation: object, permissions: set[str], index: int) -> None:
    where = f"operations[{index}]"
    if not isinstance(operation, dict) or "type" not in operation:
        raise PluginError(f"{where} must be an object with a type")
    kind = operation["type"]
    if kind not in OPERATION_TYPES:
        raise PluginError(f"{where} has unknown type {kind!r}")
    if kind == "resolver":
        require_keys(operation, {"type", "input", "output"}, where)
        valid_text(operation["input"], 512, f"{where}.input")
        valid_text(operation["output"], 512, f"{where}.output")
    elif kind == "hook":
        require_keys(operation, {"type", "event", "text"}, where)
        if operation["event"] not in HOOK_EVENTS:
            raise PluginError(f"{where}.event must be one of {sorted(HOOK_EVENTS)}")
        valid_text(operation["text"], 512, f"{where}.text")
    else:
        require_keys(operation, {"type", "prefix"}, where)
        valid_text(operation["prefix"], 512, f"{where}.prefix")
        if "report_counts" not in permissions:
            raise PluginError(
                f"{where} needs the report_counts permission to read counts"
            )


def validate_component(component: object, where: str) -> None:
    if not isinstance(component, dict) or "type" not in component:
        raise PluginError(f"{where} must be an object with a type")
    kind = component["type"]
    if kind not in COMPONENT_TYPES:
        raise PluginError(f"{where} has unknown component type {kind!r}")
    if kind in {"heading", "badge", "callout", "markdown", "code"}:
        require_keys(component, {"type", "text"}, where)
        valid_text(component["text"], 4096, f"{where}.text")
    elif kind == "progress":
        require_keys(component, {"type", "label", "value"}, where)
        valid_text(component["label"], 256, f"{where}.label")
        value = component["value"]
        if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= 100:
            raise PluginError(f"{where}.value must be an integer from 0 to 100")
    elif kind in {"form", "button"}:
        require_keys(component, {"type", "label"}, where)
        valid_text(component["label"], 256, f"{where}.label")
    else:
        require_keys(component, {"type", "headers", "rows"}, where)
        headers, rows = component["headers"], component["rows"]
        if not isinstance(headers, list) or not 1 <= len(headers) <= 16:
            raise PluginError(f"{where}.headers must hold 1 to 16 entries")
        for index, header in enumerate(headers):
            valid_text(header, 256, f"{where}.headers[{index}]")
        if not isinstance(rows, list) or len(rows) > 64:
            raise PluginError(f"{where}.rows must hold at most 64 entries")
        for row_index, row in enumerate(rows):
            if not isinstance(row, list) or len(row) != len(headers):
                raise PluginError(
                    f"{where}.rows[{row_index}] must match the header width"
                )
            for cell_index, cell in enumerate(row):
                valid_text(cell, 256, f"{where}.rows[{row_index}][{cell_index}]")


def validate_page(page: object, index: int, seen: set[str]) -> None:
    where = f"pages[{index}]"
    require_keys(page, {"id", "title", "components"}, where)
    page_id = page["id"]
    if not isinstance(page_id, str) or not ID_RE.match(page_id):
        raise PluginError(f"{where}.id must match {ID_RE.pattern}")
    if page_id in seen:
        raise PluginError(f"duplicate page id {page_id!r}")
    seen.add(page_id)
    valid_text(page["title"], 128, f"{where}.title")
    components = page["components"]
    if not isinstance(components, list) or len(components) > 64:
        raise PluginError(f"{where}.components must hold at most 64 entries")
    for component_index, component in enumerate(components):
        validate_component(component, f"{where}.components[{component_index}]")


def validate_manifest(document: object) -> dict:
    required = {"schema", "id", "name", "version", "author", "license", "source"}
    optional = {"permissions", "operations", "pages"}
    if not isinstance(document, dict):
        raise PluginError("manifest must be an object")
    unknown = set(document) - required - optional
    missing = required - set(document)
    if unknown:
        raise PluginError(f"manifest has unknown fields: {', '.join(sorted(unknown))}")
    if missing:
        raise PluginError(f"manifest is missing fields: {', '.join(sorted(missing))}")
    manifest = document
    if manifest["schema"] != SCHEMA:
        raise PluginError(f"schema must be {SCHEMA!r}")
    if not isinstance(manifest["id"], str) or not ID_RE.match(manifest["id"]):
        raise PluginError(f"id must match {ID_RE.pattern}")
    valid_text(manifest["name"], 128, "name")
    valid_text(manifest["version"], 64, "version")
    valid_text(manifest["author"], 256, "author")
    valid_text(manifest["license"], 128, "license")
    validate_source(manifest["source"])

    permissions = manifest.get("permissions", [])
    if not isinstance(permissions, list) or len(permissions) > 1:
        raise PluginError("permissions must be a list of at most one entry")
    for permission in permissions:
        if permission not in PERMISSIONS:
            raise PluginError(f"unknown permission {permission!r}")

    operations = manifest.get("operations", [])
    if not isinstance(operations, list) or len(operations) > 64:
        raise PluginError("operations must be a list of at most 64 entries")
    for index, operation in enumerate(operations):
        validate_operation(operation, set(permissions), index)

    pages = manifest.get("pages", [])
    if not isinstance(pages, list) or len(pages) > 16:
        raise PluginError("pages must be a list of at most 16 entries")
    seen: set[str] = set()
    for index, page in enumerate(pages):
        validate_page(page, index, seen)
    return manifest


def parse_manifest(raw: bytes) -> dict:
    if len(raw) > MAX_MANIFEST_BYTES:
        raise PluginError(f"manifest exceeds {MAX_MANIFEST_BYTES} bytes")
    try:
        document = json.loads(raw.decode("utf-8"))
    except UnicodeDecodeError as error:
        raise PluginError(f"manifest is not valid UTF-8: {error}") from error
    except json.JSONDecodeError as error:
        raise PluginError(f"manifest is not valid JSON: {error}") from error
    return validate_manifest(document)


def read_plugin_dir(directory: Path) -> tuple[bytes, dict]:
    if directory.is_symlink() or not directory.is_dir():
        raise PluginError(f"{directory} is not a plain directory")
    manifest_path = directory / MANIFEST_NAME
    if manifest_path.is_symlink():
        raise PluginError(f"{manifest_path} must not be a symbolic link")
    if not manifest_path.is_file():
        raise PluginError(f"{manifest_path} does not exist")
    entries = sorted(entry.name for entry in directory.iterdir())
    if entries != [MANIFEST_NAME]:
        raise PluginError(
            f"a v1 plugin directory holds only {MANIFEST_NAME}; found {entries}"
        )
    raw = manifest_path.read_bytes()
    return raw, parse_manifest(raw)


def pack(directory: Path, out_dir: Path) -> tuple[Path, Path]:
    raw, manifest = read_plugin_dir(directory)
    out_dir.mkdir(parents=True, exist_ok=True)
    bundle = out_dir / f"{manifest['id']}.plugin"
    info = zipfile.ZipInfo(MANIFEST_NAME, date_time=FIXED_ZIP_TIME)
    info.compress_type = zipfile.ZIP_STORED
    info.external_attr = 0o644 << 16
    info.create_system = 3
    with zipfile.ZipFile(bundle, "w") as archive:
        archive.writestr(info, raw)
    digest = hashlib.sha256(raw).hexdigest()
    sidecar = bundle.with_name(bundle.name + ".sha256")
    # 显式 LF：Windows 上 Path.write_text 默认转成 CRLF，会让 Linux 的
    # `sha256sum -c` 把 \r 当成文件名的一部分而校验失败。
    with sidecar.open("w", encoding="ascii", newline="\n") as handle:
        handle.write(f"{digest}  {bundle.name}\n")
    return bundle, sidecar


def read_bundle(bundle: Path) -> bytes:
    if not bundle.is_file():
        raise PluginError(f"{bundle} does not exist")
    try:
        with zipfile.ZipFile(bundle) as archive:
            names = archive.namelist()
            if names != [MANIFEST_NAME]:
                raise PluginError(
                    f"a v1 bundle holds exactly one entry, {MANIFEST_NAME}; found {names}"
                )
            for name in names:
                if name.startswith("/") or ".." in Path(name).parts:
                    raise PluginError(f"unsafe entry name {name!r}")
            return archive.read(MANIFEST_NAME)
    except zipfile.BadZipFile as error:
        raise PluginError(f"{bundle} is not a ZIP archive: {error}") from error


def verify(bundle: Path) -> tuple[bytes, dict, str, str]:
    raw = read_bundle(bundle)
    manifest = parse_manifest(raw)
    digest = hashlib.sha256(raw).hexdigest()
    sidecar = bundle.with_name(bundle.name + ".sha256")
    if not sidecar.is_file():
        raise PluginError(f"missing sidecar {sidecar.name}")
    declared = sidecar.read_text(encoding="ascii").split()
    if not declared:
        raise PluginError(f"{sidecar.name} is empty")
    if declared[0] != digest:
        raise PluginError(
            f"digest mismatch: manifest {digest} != sidecar {declared[0]}"
        )
    if len(declared) > 1 and declared[1] != bundle.name:
        raise PluginError(f"sidecar names {declared[1]!r}, not {bundle.name!r}")
    return raw, manifest, digest, sidecar.name


def unpack(bundle: Path, out_dir: Path) -> Path:
    raw, manifest, _, _ = verify(bundle)
    target = out_dir / manifest["id"]
    if target.exists():
        raise PluginError(f"{target} already exists; remove it first")
    target.mkdir(parents=True)
    (target / MANIFEST_NAME).write_bytes(raw)
    return target


def pack_and_verify(directory: Path, out_dir: Path) -> tuple[Path, str]:
    bundle, sidecar = pack(directory, out_dir)
    raw, _, digest, _ = verify(bundle)
    with tempfile.TemporaryDirectory() as scratch:
        restored = unpack(bundle, Path(scratch))
        if (restored / MANIFEST_NAME).read_bytes() != raw:
            raise PluginError("roundtrip changed the manifest bytes")
    return bundle, digest


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="pack_plugin.py",
        description="Pack, verify and unpack local parallaxe.plugin/v1 plugins.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    pack_command = commands.add_parser("pack", help="create <id>.plugin and .sha256")
    pack_command.add_argument("directory", type=Path)
    pack_command.add_argument("--out-dir", type=Path, default=Path("dist"))

    verify_command = commands.add_parser("verify", help="validate a .plugin bundle")
    verify_command.add_argument("bundle", type=Path)

    unpack_command = commands.add_parser("unpack", help="extract a .plugin bundle")
    unpack_command.add_argument("bundle", type=Path)
    unpack_command.add_argument("--out-dir", type=Path, required=True)

    roundtrip_command = commands.add_parser(
        "roundtrip", help="pack, verify, unpack and compare bytes"
    )
    roundtrip_command.add_argument("directory", type=Path)
    roundtrip_command.add_argument("--out-dir", type=Path, default=Path("dist"))

    args = parser.parse_args(argv)
    try:
        if args.command == "pack":
            bundle, sidecar = pack(args.directory, args.out_dir)
            print(f"packed  {bundle}")
            print(f"sidecar {sidecar}")
        elif args.command == "verify":
            _, manifest, digest, _ = verify(args.bundle)
            print(f"verified {manifest['id']} {manifest['version']} sha256={digest}")
        elif args.command == "unpack":
            target = unpack(args.bundle, args.out_dir)
            print(f"unpacked {target}")
        else:
            bundle, digest = pack_and_verify(args.directory, args.out_dir)
            print(f"roundtrip ok {bundle.name} sha256={digest}")
    except PluginError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except OSError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
