#!/usr/bin/env python3
"""校验正式发行产物中的编译期标记与 SHA256SUMS（只读，除非显式 --write-sha256）。"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import tarfile
import zipfile
from pathlib import Path

MARKERS = {
    "source": b"parallaxe.source-build/v1",
    "distribution": b"parallaxe.distribution-release/v1",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def zip_binary_members(path: Path) -> list[tuple[str, bytes]]:
    with zipfile.ZipFile(path) as archive:
        names = [
            name
            for name in archive.namelist()
            if not name.endswith("/")
            and (
                name.endswith(".exe")
                or name.endswith(".wasm")
                or name.endswith(".so")
                or name == "classes.dex"
            )
        ]
        return [(name, archive.read(name)) for name in names]


def tar_binary_members(path: Path) -> list[tuple[str, bytes]]:
    members: list[tuple[str, bytes]] = []
    with tarfile.open(path, "r:*") as archive:
        for member in archive.getmembers():
            if member.isfile() and (member.mode & 0o111):
                handle = archive.extractfile(member)
                if handle is not None:
                    members.append((member.name, handle.read()))
    return members


def validate_artifact(path: Path, expected: bytes) -> list[str]:
    errors: list[str] = []
    if path.suffix == ".zip":
        members = zip_binary_members(path)
        if path.name.endswith("-web.zip"):
            if not any(name.endswith(".wasm") for name, _ in members):
                errors.append("Web 包缺少 .wasm")
            with zipfile.ZipFile(path) as archive:
                if not any(name.endswith("index.html") for name in archive.namelist()):
                    errors.append("Web 包缺少 index.html")
        elif "-android-" in path.name:
            if not any(name == "classes.dex" for name, _ in members):
                errors.append("APK 缺少 classes.dex")
            if not any(name.endswith("libparallaxe_gui.so") for name, _ in members):
                errors.append("APK 缺少 libparallaxe_gui.so")
        elif not any(name.endswith("parallaxe.exe") for name, _ in members):
            errors.append("Windows 包缺少 parallaxe.exe")
    elif path.name.endswith((".tar.gz", ".tgz")):
        members = tar_binary_members(path)
        if not members:
            errors.append("Linux 包缺少可执行文件")
    elif path.suffix == ".exe":
        if expected not in path.read_bytes():
            errors.append(f"可执行文件缺少标记 {expected.decode('ascii')}")
        return errors
    else:
        return errors

    if not members:
        errors.append("没有可校验的二进制成员")
    elif not any(expected in data for _, data in members):
        errors.append(f"二进制成员缺少标记 {expected.decode('ascii')}")
    return errors


def parse_sums(path: Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line in path.read_text(encoding="ascii").splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"([0-9a-fA-F]{64})\s+\*?(.+)", line.strip())
        if not match:
            raise ValueError(f"SHA256SUMS 行格式错误：{line!r}")
        entries[match.group(2)] = match.group(1).lower()
    return entries


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-dir", required=True, type=Path)
    parser.add_argument("--expect", choices=["source", "distribution"], default="distribution")
    parser.add_argument("--write-sha256", action="store_true")
    args = parser.parse_args()

    release_dir = args.release_dir.resolve()
    if not release_dir.is_dir():
        print(f"ERROR: 发行目录不存在：{release_dir}", file=sys.stderr)
        return 2

    artifacts = sorted(
        path
        for path in release_dir.iterdir()
        if path.is_file()
        and path.name != "SHA256SUMS.txt"
        and path.suffix in {".zip", ".apk", ".gz", ".tgz", ".exe"}
    )
    errors: list[str] = []
    if not artifacts:
        errors.append("发行目录没有 ZIP/APK/tar.gz 产物")
    for artifact in artifacts:
        try:
            artifact_errors = validate_artifact(artifact, MARKERS[args.expect])
        except (OSError, zipfile.BadZipFile, tarfile.TarError) as exc:
            artifact_errors = [f"归档读取失败：{exc}"]
        errors.extend(f"{artifact.name}: {error}" for error in artifact_errors)
        print(f"{'FAIL' if artifact_errors else 'PASS'} {artifact.name}")

    sums_path = release_dir / "SHA256SUMS.txt"
    if args.write_sha256:
        files = sorted(path for path in release_dir.iterdir() if path.is_file() and path != sums_path)
        sums_path.write_text(
            "".join(f"{sha256(path)}  {path.name}\n" for path in files),
            encoding="ascii",
        )
        print(f"WROTE {sums_path}")
    elif sums_path.exists():
        try:
            listed = parse_sums(sums_path)
            for name, expected_hash in listed.items():
                path = release_dir / name
                if not path.is_file():
                    errors.append(f"SHA256SUMS 引用缺失文件：{name}")
                elif sha256(path) != expected_hash:
                    errors.append(f"SHA256 不匹配：{name}")
            actual_top_level = {
                path.name for path in release_dir.iterdir() if path.is_file() and path != sums_path
            }
            missing_from_manifest = sorted(actual_top_level - set(listed))
            if missing_from_manifest:
                errors.append(f"SHA256SUMS 缺少文件：{', '.join(missing_from_manifest)}")
        except (OSError, UnicodeError, ValueError) as exc:
            errors.append(f"SHA256SUMS 校验失败：{exc}")
    else:
        errors.append("缺少 SHA256SUMS.txt")

    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(f"RELEASE_VERIFY_OK expect={args.expect} artifacts={len(artifacts)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
