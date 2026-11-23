#!/usr/bin/env python3
"""界面优化检验（只读、可复现、报告优先）。

检查三类可量化指标：
1. `theme.rs` 语义令牌的 WCAG 对比度（正文 4.5:1，大字/边框 3:1）；
2. 明暗主题的**结构一致性**（像素级卡片掩码 IoU + 三卡列宽/间距）；
3. GUI 源码的硬编码颜色与 9-State 兜底风险（人工复核清单）。

用法：
  python tests/release-smoke/ui_audit.py
  python tests/release-smoke/ui_audit.py --strict      # 对比度或结构不达标时返回 1
  python tests/release-smoke/ui_audit.py --out tests/release-smoke/latest
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = Path(__file__).resolve().parent / "latest"
SHOTS = ROOT / "tests" / "release-smoke" / "latest" / "screenshots"  # 本地生成，不随源码发布


def parse_palette(text: str) -> dict[str, dict[str, tuple[int, int, int]]]:
    out: dict[str, dict[str, tuple[int, int, int]]] = {}
    for mode in ("Dark", "Light"):
        m = re.search(rf"ThemeMode::{mode} => Palette \{{(.*?)\n            \}},", text, re.S)
        if not m:
            continue
        vals: dict[str, tuple[int, int, int]] = {}
        for field, r, g, b in re.findall(
            r"(\w+): Color32::from_rgb\((\d+),\s*(\d+),\s*(\d+)\)", m.group(1)
        ):
            vals[field] = (int(r), int(g), int(b))
        out[mode] = vals
    return out


def luminance(rgb: tuple[int, int, int]) -> float:
    def channel(value: int) -> float:
        v = value / 255.0
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    r, g, b = (channel(x) for x in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a: tuple[int, int, int], b: tuple[int, int, int]) -> float:
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


TEXT_PAIRS = [
    ("text_primary", "canvas", 4.5),
    ("text_primary", "card", 4.5),
    ("text_secondary", "card", 4.5),
    ("text_secondary", "canvas", 4.5),
    ("text_muted", "card", 4.5),
    ("text_muted", "sidebar", 4.5),
    ("text_primary", "table_even_bg", 4.5),
    ("accent_fg", "accent", 4.5),
    ("warning_fg", "warning_bg", 4.5),
    ("danger_fg", "danger_bg", 4.5),
    ("info_fg", "info_bg", 4.5),
    ("ok_fg", "ok_bg", 4.5),
]

# 边框是「克制装饰」而非识别组件的唯一依据，单独报告、不计入硬失败。
INFO_PAIRS = [
    ("border", "canvas", 3.0),
    ("border", "card", 3.0),
]


def card_mask(path: Path, rgb: tuple[int, int, int], tol: int = 6):
    from PIL import Image
    import numpy as np

    image = np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)
    return (np.abs(image - np.array(rgb, dtype=np.int16)).max(axis=2) <= tol)


def structure_audit(palette: dict) -> list[dict]:
    from PIL import Image
    import numpy as np

    dark_path = SHOTS / "stage5-minimal-dark.png"
    light_path = SHOTS / "stage5-minimal-light.png"
    results: list[dict] = []
    if not dark_path.exists() or not light_path.exists():
        return [
            {
                "name": "明暗结构一致性",
                "ok": False,
                "detail": "缺少本地截图（请用 tools/capture_gui.py 生成到 tests/release-smoke/latest/screenshots/）；该项 SKIP，不影响构建",
                "skipped": True,
            }
        ]

    dark_size = Image.open(dark_path).size
    light_size = Image.open(light_path).size
    results.append(
        {
            "name": "明暗截图尺寸一致",
            "ok": dark_size == light_size,
            "detail": f"{dark_size} vs {light_size}",
        }
    )

    dark_rgb = palette["Dark"]["card"]
    light_rgb = palette["Light"]["card"]
    # 浅色卡片是纯白，与页面其他白色元素易混；用卡片填充色做掩码仍可定位三卡。
    dm = card_mask(dark_path, dark_rgb)
    lm = card_mask(light_path, light_rgb)
    inter = np.logical_and(dm, lm).sum()
    union = np.logical_or(dm, lm).sum()
    iou = float(inter / union) if union else 0.0
    results.append(
        {
            "name": "卡片掩码 IoU（明 vs 暗）",
            "ok": iou >= 0.95,
            "detail": f"{iou:.4f}（阈值 0.95）",
        }
    )

    # 三卡列宽与间距（取内容区，避开顶栏）
    top, bottom = 170, min(dark_size[1], 820)
    cols = dm[top:bottom].sum(axis=0)
    on = cols > 20
    runs: list[tuple[int, int]] = []
    start = None
    for i, v in enumerate(on):
        if v and start is None:
            start = i
        elif not v and start is not None:
            if i - start > 20:
                runs.append((start, i - 1))
            start = None
    widths = [b - a + 1 for a, b in runs]
    gaps = [runs[i + 1][0] - runs[i][1] - 1 for i in range(len(runs) - 1)]
    results.append(
        {
            "name": "三卡等宽",
            "ok": len(set(widths)) == 1 if widths else False,
            "detail": f"宽度 {widths} px（物理像素）",
        }
    )
    gap_logical = [round(g / 1.25, 1) for g in gaps]
    results.append(
        {
            "name": "卡片间距 ≥16 逻辑像素",
            "ok": bool(gaps) and all(g >= 16 * 1.25 for g in gaps),
            "detail": f"间距 {gaps} px @125% = {gap_logical} 逻辑像素",
        }
    )
    return results


def source_audit() -> list[dict]:
    results = []
    view_files = sorted((ROOT / "crates" / "gui" / "src" / "views").glob("*.rs"))
    hits = []
    for path in view_files:
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            # 只关心颜色构造；`Color32` 类型名/导入行不算。
            if "Color32::" in line:
                hits.append(f"{path.name}:{lineno}: {line.strip()}")
    # `use` 导入、TRANSPARENT、调色板插值、类型声明都不算硬编码语义颜色
    semantic = []
    for hit in hits:
        code = hit.split(": ", 1)[1].strip()
        if code.startswith("use "):
            continue
        if "TRANSPARENT" in code or "lerp_color" in code or code.startswith("color: egui::Color32"):
            continue
        semantic.append(hit)
    results.append(
        {
            "name": "views/ 硬编码语义颜色",
            "ok": not semantic,
            "detail": f"命中 {len(hits)} 行，其中语义硬编码 {len(semantic)} 行"
            + ("" if not semantic else "：" + "；".join(semantic[:5])),
        }
    )

    # 9-State 映射必须穷举；这里只做启发式提示，需人工确认。
    risky = []
    for path in [ROOT / "crates" / "gui" / "src" / "theme.rs"] + view_files:
        text = path.read_text(encoding="utf-8")
        if "fn state_badge" in text:
            body = text.split("fn state_badge", 1)[1].split("\n}\n", 1)[0]
            if "_ =>" in body:
                risky.append(f"{path.name}: state_badge 含 `_ =>`")
    finding = ROOT / "crates" / "gui" / "src" / "views" / "service_matrix.rs"
    if finding.exists():
        text = finding.read_text(encoding="utf-8")
        for m in re.finditer(r"match r\.state \{(.{0,400}?)\}", text, re.S):
            if "_ =>" in m.group(1):
                line = text.count("\n", 0, m.start()) + 1
                risky.append(f"service_matrix.rs:{line}: 9-State 着色 match 含 `_ =>`")
    results.append(
        {
            "name": "9-State 匹配无兜底（启发式）",
            "ok": not risky,
            "detail": "；".join(risky) if risky else "state_badge 穷举；未发现其他 State 兜底",
        }
    )
    return results


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser(description="Parallaxe box 界面优化检验")
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument("--strict", action="store_true", help="任一硬指标失败时返回 1")
    args = ap.parse_args()

    out_dir = Path(args.out).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    theme_src = (ROOT / "crates" / "gui" / "src" / "theme.rs").read_text(encoding="utf-8")
    palette = parse_palette(theme_src)

    contrast_rows = []
    failures = 0
    for mode in ("Dark", "Light"):
        vals = palette.get(mode, {})
        for fg, bg, threshold in TEXT_PAIRS:
            if fg not in vals or bg not in vals:
                continue
            ratio = contrast(vals[fg], vals[bg])
            ok = ratio >= threshold
            if not ok:
                failures += 1
            contrast_rows.append(
                {
                    "theme": mode,
                    "foreground": fg,
                    "background": bg,
                    "ratio": round(ratio, 2),
                    "required": threshold,
                    "ok": ok,
                }
            )

    info_rows = []
    for mode in ("Dark", "Light"):
        vals = palette.get(mode, {})
        for fg, bg, threshold in INFO_PAIRS:
            if fg not in vals or bg not in vals:
                continue
            info_rows.append(
                {
                    "theme": mode,
                    "foreground": fg,
                    "background": bg,
                    "ratio": round(contrast(vals[fg], vals[bg]), 2),
                    "required": threshold,
                    "ok": contrast(vals[fg], vals[bg]) >= threshold,
                }
            )

    structure = structure_audit(palette)
    sources = source_audit()
    hard_fail = failures + sum(1 for r in structure if not r.get("ok") and not r.get("skipped"))

    summary = {
        "schema": "parallaxe.ui-audit/v1",
        "palette": {mode: {k: list(v) for k, v in vals.items()} for mode, vals in palette.items()},
        "contrast": contrast_rows,
        "contrast_informational": info_rows,
        "structure": structure,
        "sources": sources,
        "hard_failures": hard_fail,
        "ok": hard_fail == 0,
    }
    (out_dir / "ui-audit.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    lines = [
        "# 界面优化检验（可量化部分）",
        "",
        "## 1. WCAG 对比度",
        "",
        "| 主题 | 前景 | 背景 | 实测 | 要求 | 结果 |",
        "|---|---|---|---:|---:|---|",
    ]
    for row in contrast_rows:
        lines.append(
            f"| {row['theme']} | `{row['foreground']}` | `{row['background']}` | "
            f"{row['ratio']:.2f} | {row['required']:.1f} | {'✅' if row['ok'] else '❌'} |"
        )
    lines += ["", "### 参考项（不计入硬失败）", "", "| 主题 | 前景 | 背景 | 实测 | 要求 | 结果 |", "|---|---|---|---:|---:|---|"]
    for row in info_rows:
        lines.append(
            f"| {row['theme']} | `{row['foreground']}` | `{row['background']}` | "
            f"{row['ratio']:.2f} | {row['required']:.1f} | {'✅' if row['ok'] else '⚠️'} |"
        )
    lines += ["", "## 2. 明暗结构一致性", "", "| 检查 | 实测 | 结果 |", "|---|---|---|"]
    for row in structure:
        mark = "⏭️" if row.get("skipped") else ("✅" if row.get("ok") else "❌")
        lines.append(f"| {row['name']} | {row['detail']} | {mark} |")
    lines += ["", "## 3. 源码红线（启发式）", "", "| 检查 | 实测 | 结果 |", "|---|---|---|"]
    for row in sources:
        lines.append(
            f"| {row['name']} | {row['detail']} | {'✅' if row.get('ok') else '❌'} |"
        )
    lines += [
        "",
        f"> 结论：{'通过' if hard_fail == 0 else f'{hard_fail} 项未达标'}（第 3 节为启发式提示，不计入硬失败）。",
        "> `accent_fg on accent` 为按钮文字对比度；浅色主题若低于 4.5，应加深强调色或改用深色文字。",
    ]
    (out_dir / "ui-audit.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

    for row in contrast_rows:
        if not row["ok"]:
            print(
                f"[FAIL] {row['theme']} {row['foreground']} on {row['background']}: "
                f"{row['ratio']:.2f} < {row['required']:.1f}"
            )
    for row in structure + sources:
        if row.get("skipped"):
            mark = "SKIP"
        else:
            mark = "PASS" if row.get("ok") else "FAIL"
        print(f"[{mark}] {row['name']}: {row['detail']}")
    print(f"结论: {hard_fail} 项未达标 -> {out_dir / 'ui-audit.md'}")
    return 0 if (hard_fail == 0 or not args.strict) else 1


if __name__ == "__main__":
    sys.exit(main())
