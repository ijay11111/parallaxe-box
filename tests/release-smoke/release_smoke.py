#!/usr/bin/env python3
"""Parallaxe box 发行烟雾验收（离线、可重复、零真实网络请求）。

设计原则：
1. 只调用仓库内命令与已构建二进制，**不访问任何外部站点**；
2. 每条断言都打印「期望 / 实际」，失败即返回码 1，绝不把跳过写成通过；
3. 日志与机器可读摘要写入 `tests/release-smoke/latest/`，便于 CI 上传为制品；
4. 覆盖阶段 2/3/4/5 的既定基准：
   - `check --profile google-all` = 选中 107 / 覆盖 0 / 发包 0 / 退出码 3
   - `selftest --offline` = 28 passed / 0 failed
   - 三端 parity = 10/10（parity_replay）+ 三进程比对 1/1
   - 自定义导入未获 F3 授权前零发包（import_bundle / user_import 两个精确用例）
   - `--no-default-features` 纯无头构建可编译

用法（在仓库根目录）：
  python tests/release-smoke/release_smoke.py
  python tests/release-smoke/release_smoke.py --no-build            # 复用已有二进制
  python tests/release-smoke/release_smoke.py --bin target/debug/parallaxe --no-cargo-tests
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import platform
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
DEFAULT_OUT = HERE / "latest"

CATALOG_SIZE = 135
GOOGLE_ALL_SELECTED = 107
SELFTEST_PASSED = 28
PARITY_CASES = 10


class Check:
    """一条烟雾断言。"""

    def __init__(self, name: str, expected: str) -> None:
        self.name = name
        self.expected = expected
        self.ok = False
        self.actual = ""
        self.detail = ""
        self.exit_code: int | None = None
        self.duration_ms = 0
        self.log: str | None = None

    def as_dict(self) -> dict:
        return {
            "name": self.name,
            "expected": self.expected,
            "ok": self.ok,
            "actual": self.actual,
            "detail": self.detail,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "log": self.log,
        }


def _exe(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def _decode(data: bytes) -> str:
    for enc in ("utf-8", "gb18030"):
        try:
            return data.decode(enc)
        except UnicodeDecodeError:
            continue
    return data.decode("utf-8", "replace")


def run(cmd: list[str], out_dir: Path, log_name: str, timeout: int = 1800) -> subprocess.CompletedProcess:
    """执行命令并落盘完整 stdout/stderr。"""
    env = dict(os.environ)
    env.setdefault("CARGO_TERM_COLOR", "never")
    started = time.time()
    proc = subprocess.run(
        cmd,
        cwd=ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    elapsed = int((time.time() - started) * 1000)
    log = out_dir / f"{log_name}.log"
    log.write_text(
        "$ " + " ".join(cmd) + "\n\n"
        + _decode(proc.stdout)
        + ("\n--- stderr ---\n" + _decode(proc.stderr) if proc.stderr else ""),
        encoding="utf-8",
    )
    proc.elapsed_ms = elapsed  # type: ignore[attr-defined]
    proc.log_path = log  # type: ignore[attr-defined]
    return proc


def parse_json_report(stdout: bytes) -> dict:
    text = _decode(stdout).strip()
    if not text:
        raise ValueError("报告为空")
    return json.loads(text)


def expected_version() -> str | None:
    """从 `[workspace.package]` 读取版本，避免脚本里的版本号与 Cargo.toml 漂移。"""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    block = re.search(r"\[workspace\.package\](.*?)(?=\n\[)", text, re.S)
    if not block:
        return None
    version = re.search(r'^version\s*=\s*"([^"]+)"', block.group(1), re.M)
    return version.group(1) if version else None


def check_google_all(bin_path: Path, out_dir: Path) -> Check:
    c = Check(
        "google-all 零准入基线",
        "selected=107 / covered=0 / 已选未覆盖=107 / 未覆盖两栏合计=135 / network=0 / exit=3",
    )
    proc = run(
        [str(bin_path), "check", "--headless", "--profile", "google-all", "--json"],
        out_dir,
        "google-all",
    )
    c.exit_code = proc.returncode
    c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
    c.log = proc.log_path.name  # type: ignore[attr-defined]
    try:
        report = parse_json_report(proc.stdout)
        cov = report["coverage"]
        selected = cov["selected_count"]
        covered = len(cov["covered"])
        uncovered = len(cov["uncovered"])
        selected_uncovered = sum(1 for item in cov["uncovered"] if item.get("selected"))
        missing_reason = sum(1 for item in cov["uncovered"] if not item.get("reason"))
        network = report["network_request_count"]
        c.actual = (
            f"selected={selected} / covered={covered} / uncovered={uncovered} "
            f"(其中已选未覆盖={selected_uncovered}) / catalog={cov['catalog_size']} / "
            f"network={network} / exit={proc.returncode}"
        )
        problems = []
        if selected != GOOGLE_ALL_SELECTED:
            problems.append(f"selected {selected} != {GOOGLE_ALL_SELECTED}")
        if covered != 0:
            problems.append(f"covered {covered} != 0")
        # 覆盖/未覆盖两栏守恒：未覆盖 = 目录全集（已选未覆盖 107 + 未选 28）。
        if uncovered != CATALOG_SIZE:
            problems.append(f"uncovered {uncovered} != {CATALOG_SIZE}")
        if selected_uncovered != GOOGLE_ALL_SELECTED:
            problems.append(f"已选未覆盖 {selected_uncovered} != {GOOGLE_ALL_SELECTED}")
        if missing_reason != 0:
            problems.append(f"{missing_reason} 条未覆盖缺少原因")
        if cov["catalog_size"] != CATALOG_SIZE:
            problems.append(f"catalog {cov['catalog_size']} != {CATALOG_SIZE}")
        if network != 0:
            problems.append(f"network {network} != 0")
        if proc.returncode != 3:
            problems.append(f"exit {proc.returncode} != 3")
        c.ok = not problems
        c.detail = "；".join(problems)
    except Exception as exc:  # noqa: BLE001 - 解析失败也要如实记录
        c.actual = "报告解析失败"
        c.detail = str(exc)
    return c


def check_selftest(bin_path: Path, out_dir: Path) -> Check:
    c = Check("离线自检", "passed=28 / failed=0 / network=0 / exit=0")
    proc = run([str(bin_path), "selftest", "--offline", "--json"], out_dir, "selftest")
    c.exit_code = proc.returncode
    c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
    c.log = proc.log_path.name  # type: ignore[attr-defined]
    try:
        doc = parse_json_report(proc.stdout)
        network = doc["report"]["network_request_count"]
        c.actual = f"passed={doc['passed']} / failed={doc['failed']} / network={network} / exit={proc.returncode}"
        problems = []
        if doc["passed"] != SELFTEST_PASSED:
            problems.append(f"passed {doc['passed']} != {SELFTEST_PASSED}")
        if doc["failed"] != 0:
            problems.append(f"failed {doc['failed']} != 0")
        if network != 0:
            problems.append(f"network {network} != 0")
        if proc.returncode != 0:
            problems.append(f"exit {proc.returncode} != 0")
        c.ok = not problems
        c.detail = "；".join(problems)
    except Exception as exc:  # noqa: BLE001
        c.actual = "报告解析失败"
        c.detail = str(exc)
    return c


def check_fixture(bin_path: Path, out_dir: Path) -> Check:
    c = Check("离线 fixture 演练", "state=OK / network=0 / exit=0")
    proc = run(
        [str(bin_path), "check", "--headless", "--fixture", "gemini-positive", "--json"],
        out_dir,
        "fixture-gemini-positive",
    )
    c.exit_code = proc.returncode
    c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
    c.log = proc.log_path.name  # type: ignore[attr-defined]
    try:
        report = parse_json_report(proc.stdout)
        states = [r["state"] for r in report["results"]]
        network = report["network_request_count"]
        c.actual = f"states={states} / network={network} / exit={proc.returncode}"
        problems = []
        if states != ["OK"]:
            problems.append(f"states {states} != ['OK']")
        if network != 0:
            problems.append(f"network {network} != 0")
        if proc.returncode != 0:
            problems.append(f"exit {proc.returncode} != 0")
        c.ok = not problems
        c.detail = "；".join(problems)
    except Exception as exc:  # noqa: BLE001
        c.actual = "报告解析失败"
        c.detail = str(exc)
    return c


def _cargo_test(
    name: str,
    expected: str,
    args: list[str],
    out_dir: Path,
    expect_passed: int,
    expect_failed: int = 0,
    expect_token: str | None = None,
    log_name: str | None = None,
) -> Check:
    c = Check(name, expected)
    # `--offline --locked` 必须位于 `--` 之前，否则会被当成测试二进制参数。
    if "--" in args:
        split = args.index("--")
        cargo_args = args[:split] + ["--offline", "--locked"] + args[split:]
    else:
        cargo_args = args + ["--offline", "--locked"]
    # 日志文件名保持 ASCII，避免跨平台 artifact 打包时的编码问题。
    slug = log_name or re.sub(r"[^0-9A-Za-z._-]+", "-", name).strip("-") or "cargo-test"
    proc = run(["cargo", "test", *cargo_args], out_dir, slug)
    c.exit_code = proc.returncode
    c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
    c.log = proc.log_path.name  # type: ignore[attr-defined]
    text = _decode(proc.stdout) + _decode(proc.stderr)
    results = [
        (int(p), int(f))
        for p, f in re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", text)
    ]
    total_passed = sum(p for p, _ in results)
    total_failed = sum(f for _, f in results)
    c.actual = f"passed={total_passed} / failed={total_failed} / exit={proc.returncode}"
    problems = []
    if total_passed < expect_passed:
        problems.append(f"passed {total_passed} < {expect_passed}")
    if total_failed != expect_failed:
        problems.append(f"failed {total_failed} != {expect_failed}")
    if proc.returncode != 0:
        problems.append(f"exit {proc.returncode} != 0")
    if expect_token and expect_token not in text:
        problems.append(f"输出缺少用例标记 {expect_token}")
    c.ok = not problems
    c.detail = "；".join(problems)
    return c


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    ap = argparse.ArgumentParser(description="Parallaxe box 发行烟雾验收")
    ap.add_argument("--bin", default=None, help="已有 parallaxe 可执行文件路径")
    ap.add_argument("--no-build", action="store_true", help="不重新构建，直接使用 --bin 或 target/debug")
    ap.add_argument("--no-cargo-tests", action="store_true", help="跳过 cargo 测试类断言（平台构建任务用）")
    ap.add_argument("--out", default=str(DEFAULT_OUT), help="输出目录（默认 tests/release-smoke/latest）")
    args = ap.parse_args()

    out_dir = Path(args.out).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    checks: list[Check] = []
    started = _dt.datetime.now().astimezone()

    print("== Parallaxe box 发行烟雾验收 ==")
    print(f"平台: {platform.platform()} / Python {platform.python_version()}")
    print(f"仓库: {ROOT}")
    print(f"输出: {out_dir}")

    bin_path = Path(args.bin).resolve() if args.bin else ROOT / "target" / "debug" / _exe("parallaxe")

    if not args.no_build:
        c = Check("纯无头构建", "cargo build --no-default-features 退出码 0")
        proc = run(
            [
                "cargo",
                "build",
                "-p",
                "parallaxe-cli",
                "--no-default-features",
                "--offline",
                "--locked",
            ],
            out_dir,
            "build-headless",
        )
        c.exit_code = proc.returncode
        c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
        c.log = proc.log_path.name  # type: ignore[attr-defined]
        c.actual = f"exit={proc.returncode}"
        c.ok = proc.returncode == 0 and bin_path.exists()
        if proc.returncode == 0 and not bin_path.exists():
            c.detail = f"构建成功但未找到 {bin_path}"
        elif proc.returncode != 0:
            c.detail = "构建失败，见日志"
        checks.append(c)

    if not bin_path.exists():
        print(f"错误：找不到可执行文件 {bin_path}，请先构建或传 --bin", file=sys.stderr)
        return 1

    # 1) 目录/版本可读
    version = expected_version()
    c = Check("版本输出", f"退出码 0 且包含 {version or 'Cargo.toml 版本'}")
    proc = run([str(bin_path), "--version"], out_dir, "version")
    c.exit_code = proc.returncode
    c.duration_ms = proc.elapsed_ms  # type: ignore[attr-defined]
    c.log = proc.log_path.name  # type: ignore[attr-defined]
    version_text = _decode(proc.stdout).strip()
    c.actual = f"{version_text} / exit={proc.returncode}"
    c.ok = (
        proc.returncode == 0
        and version is not None
        and version in version_text
    )
    checks.append(c)

    # 2) 阶段 2 基线
    checks.append(check_google_all(bin_path, out_dir))
    checks.append(check_selftest(bin_path, out_dir))
    checks.append(check_fixture(bin_path, out_dir))

    # 3) 三端一致性（GUI 回放 10 个场景 + 三进程比对）
    if not args.no_cargo_tests:
        checks.append(
            _cargo_test(
                "GUI 回放 parity",
                f"parity_replay 10/10 通过",
                ["-p", "parallaxe-gui", "--test", "parity_replay", "--", "--test-threads=1"],
                out_dir,
                expect_passed=PARITY_CASES,
                log_name="parity-replay",
            )
        )
        checks.append(
            _cargo_test(
                "三端跨进程 parity",
                "gui_cli_agent_parity 1/1 通过",
                ["-p", "parallaxe-cli", "--test", "gui_cli_agent_parity"],
                out_dir,
                expect_passed=1,
                expect_token="tri_interface_live_process_parity",
                log_name="tri-interface-parity",
            )
        )
        # 4) 导入零发包（未获 F3 一次性授权前）
        checks.append(
            _cargo_test(
                "导入包未授权零发包",
                "imported_bundle_targets_send_zero_packets_before_f3_grant 通过",
                [
                    "-p",
                    "parallaxe-core",
                    "--test",
                    "import_bundle",
                    "--",
                    "--exact",
                    "imported_bundle_targets_send_zero_packets_before_f3_grant",
                ],
                out_dir,
                expect_passed=1,
                expect_token="imported_bundle_targets_send_zero_packets_before_f3_grant",
                log_name="import-bundle-zero-packet",
            )
        )
        checks.append(
            _cargo_test(
                "自定义导入一次性许可与撤销",
                "unauthorized_zero_packets_one_time_grant_revocation_and_audit_trail 通过",
                [
                    "-p",
                    "parallaxe-core",
                    "--test",
                    "user_import",
                    "--",
                    "--exact",
                    "unauthorized_zero_packets_one_time_grant_revocation_and_audit_trail",
                ],
                out_dir,
                expect_passed=1,
                expect_token="unauthorized_zero_packets_one_time_grant_revocation_and_audit_trail",
                log_name="user-import-grant",
            )
        )

    # 汇总
    passed = sum(1 for c in checks if c.ok)
    failed = len(checks) - passed
    finished = _dt.datetime.now().astimezone()
    summary = {
        "schema": "parallaxe.release-smoke/v1",
        "generated_at": finished.isoformat(),
        "started_at": started.isoformat(),
        "duration_ms": int((finished - started).total_seconds() * 1000),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "binary": str(bin_path),
        "baselines": {
            "google_all_selected": GOOGLE_ALL_SELECTED,
            "selftest_passed": SELFTEST_PASSED,
            "parity_cases": PARITY_CASES,
            "catalog_size": CATALOG_SIZE,
        },
        "passed": passed,
        "failed": failed,
        "ok": failed == 0,
        "checks": [c.as_dict() for c in checks],
    }
    (out_dir / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    lines = [
        "# 发行烟雾验收结果",
        "",
        f"- 时间：{summary['generated_at']}",
        f"- 平台：{summary['platform']}",
        f"- 二进制：`{bin_path}`",
        f"- 结论：**{'通过' if failed == 0 else '失败'}**（{passed} 通过 / {failed} 失败）",
        "",
        "| 断言 | 期望 | 实际 | 结果 |",
        "|---|---|---|---|",
    ]
    for c in checks:
        lines.append(
            f"| {c.name} | {c.expected} | {c.actual or '—'} | {'✅' if c.ok else '❌'} |"
        )
        if not c.ok and c.detail:
            lines.append(f"| ↳ 原因 | | {c.detail} | |")
    lines.append("")
    lines.append("> 本脚本只读取离线 fixture 与内嵌目录，`network_request_count` 必须为 0。")
    (out_dir / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

    print()
    for c in checks:
        mark = "PASS" if c.ok else "FAIL"
        print(f"[{mark}] {c.name}: {c.actual}")
        if not c.ok and c.detail:
            print(f"       期望 {c.expected}；原因：{c.detail}")
    print()
    print(f"汇总: {passed} 通过 / {failed} 失败 -> {out_dir / 'summary.md'}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
