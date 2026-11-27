#!/usr/bin/env python3
"""Parallaxe box GUI 截图工具（供无头 agent / 人工使用）。

启动 parallaxe gui，等待窗口出现，按标题定位窗口，截取该窗口保存为 PNG。
无需改动产品代码，也无需真实鼠标键盘交互（截图不抢焦点）。

用法:
  python tools/capture_gui.py --out tests/release-smoke/latest/screenshots/normal-dark.png
  python tools/capture_gui.py --out tests/release-smoke/latest/screenshots/minimal-light.png --density minimal --theme light
  python tools/capture_gui.py --out tests/release-smoke/latest/screenshots/narrow.png --width 800 --height 900
  python tools/capture_gui.py --list              # 只列出当前 GUI 窗口

参数:
  --out        输出 PNG 路径（默认 tests/release-smoke/latest/screenshots/capture.png；本地产物，不提交）
  --exe        可执行文件（默认 target/debug/parallaxe.exe）
  --title      窗口标题匹配串（默认 Parallaxe box）
  --width/--height  期望窗口尺寸（可选，尝试调整窗口大小）
  --density    normal|minimal（仅备注进文件名提示，不自动点击）
  --theme      dark|light（同上）
  --wait       等待窗口秒数（默认 8）
  --existing   不启动新进程，只截已存在的窗口
  --qa-view    通过 gui_walkthrough 走读真实指针事件截特定视图（推荐，防遮挡/防黑屏）

版本说明（2026-11-22 更新）：
  当前工具已升级为两路截图：
  1) 默认路：PrintWindow 读取目标进程客户区（PID 过滤、防其他窗口遮挡）；
  2) --qa-view 路：启动 gui_walkthrough.exe 走读真实 eframe 指针事件，
     直接帧缓冲输出（可覆盖弹窗等交互态），必要时先执行：
        cargo build -p parallaxe-gui --example gui_walkthrough
  若 --qa-view 报"请先 cargo build..."，那是演练程序未编译，不是工具过时。
  用 python tools/capture_gui.py --doctor 可自检依赖与演练程序状态。
"""
import argparse
import os
import json
import tempfile
from pathlib import Path
import subprocess
import sys
import time

from PIL import Image
import ctypes

try:
    import win32gui
    import win32ui
    import win32process
    HAS_WIN32 = True
except ImportError:
    HAS_WIN32 = False


def find_window(title_substr: str, pid=None):
    """返回 (hwnd, rect, title)；需要 pywin32。"""
    found = []

    def cb(hwnd, _):
        if not win32gui.IsWindowVisible(hwnd):
            return
        title = win32gui.GetWindowText(hwnd)
        if (pid is None or win32process.GetWindowThreadProcessId(hwnd)[1] == pid) and title_substr.lower() in title.lower():
            found.append((hwnd, win32gui.GetWindowRect(hwnd), title))

    win32gui.EnumWindows(cb, None)
    if not found:
        return None
    # 取面积最大的（排除小的图标窗口）
    found.sort(key=lambda x: (x[1][2] - x[1][0]) * (x[1][3] - x[1][1]), reverse=True)
    return found[0]


def capture_client(hwnd):
    """只读目标窗口的客户区；桌面被遮挡时也不把其他应用截入。"""
    _, _, w, h = win32gui.GetClientRect(hwnd)
    handle = win32gui.GetDC(hwnd)
    src = win32ui.CreateDCFromHandle(handle)
    dst = src.CreateCompatibleDC()
    bitmap = win32ui.CreateBitmap()
    try:
        bitmap.CreateCompatibleBitmap(src, w, h)
        dst.SelectObject(bitmap)
        if not ctypes.windll.user32.PrintWindow(hwnd, dst.GetSafeHdc(), 3):
            raise RuntimeError("窗口未提供客户区图像")
        im = Image.frombytes("RGB", (w, h), bitmap.GetBitmapBits(True), "raw", "BGRX")
        if im.getcolors(8) is not None:
            raise RuntimeError("窗口未返回可验证的内容；请用 --qa-view 读取窗口帧缓冲。")
        return im
    finally:
        dst.DeleteDC()
        del src
        win32gui.DeleteObject(bitmap.GetHandle())
        win32gui.ReleaseDC(hwnd, handle)


def resize(hwnd, w, h):
    try:
        win32gui.MoveWindow(hwnd, 100, 80, w, h, True)
    except Exception as e:
        print(f"[warn] resize failed: {e}", file=sys.stderr)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="tests/release-smoke/latest/screenshots/capture.png")
    ap.add_argument("--exe", default="target/debug/parallaxe.exe")
    ap.add_argument("--title", default="Parallaxe box")
    ap.add_argument("--width", type=int, default=0)
    ap.add_argument("--height", type=int, default=0)
    ap.add_argument("--density", choices=["normal", "minimal"], default=None)
    ap.add_argument("--theme", choices=["dark", "light"], default=None)
    ap.add_argument("--wait", type=float, default=8.0)
    ap.add_argument("--existing", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--qa-view", choices=["check-home", "check-home-light", "check-home-en", "check-pace", "default-bundle", "default-bundle-scope", "default-bundle-confirmed", "default-bundle-result", "default-bundle-export", "distribution-mode-confirm", "overview", "overview-en", "overview-elig", "minimal", "minimal-dark", "result", "plan", "guard-link", "guard-en", "guard-proactive", "guard-export-modal", "guard-export-bundle", "mobile-guard-export-bundle", "mobile-guard-export-en", "guard-sysblock-modal", "guard-full-modal", "settings-schedule", "settings-en", "settings-light", "settings-root", "settings-root-en", "settings-root-light", "settings-appearance", "settings-scope", "settings-advanced", "settings-advanced-en", "settings-ai", "settings-address", "settings-rules", "settings-notices", "settings-about", "settings-about-en", "settings-about-light", "plugin-page", "about-modal", "notices-modal", "timeline-export-md", "mobile-minimal", "mobile-minimal-light", "mobile-en", "mobile-guard", "mobile-import-manual", "mobile-import-text", "mobile-import-zip", "menus", "import", "import-en", "import-text", "import-zip", "import-checked", "dest-mismatch", "dest-mismatch-ip", "dest-mismatch-en", "dest-proxy", "product-restricted", "dest-guard-link-ip", "ai-f3-modal", "ai-f3-modal-en", "ai-settings-off", "ai-settings-on", "ai-self-disabled"],
                    help="通过真实 eframe 指针事件走读，仅读取目标帧缓冲（宽高为逻辑像素）")
    ap.add_argument("--doctor", action="store_true", help="自检依赖与演练程序状态，不做截图")
    ap.add_argument(
        "--strict",
        action="store_true",
        help="doctor 模式下任一依赖/演练程序缺失即返回退出码 3（供 CI 前置门禁使用）",
    )
    args = ap.parse_args()
    if args.doctor:
        root = Path(__file__).resolve().parent.parent
        problems = []
        print("capture_gui.py doctor")
        print("  platform:", sys.platform)
        if sys.platform == "win32":
            print("  pywin32:", "OK" if HAS_WIN32 else "MISSING (python -m pip install pywin32)")
            if not HAS_WIN32:
                problems.append("pywin32")
        else:
            print("  pywin32: N/A（截图工具仅支持 Windows）")
            problems.append("非 Windows 平台")
        try:
            import PIL
            print("  Pillow:", PIL.__version__)
        except Exception as e:
            print("  Pillow: MISSING", e)
            problems.append("Pillow")
        walkthrough = root / "target/debug/examples/gui_walkthrough.exe"
        exe = root / "target/debug/parallaxe.exe"
        print("  gui_walkthrough.exe:", "OK" if walkthrough.exists() else "未编译 (cargo build -p parallaxe-gui --example gui_walkthrough)")
        print("  parallaxe.exe:", "OK" if exe.exists() else "未编译 (cargo build -p parallaxe-cli)")
        if not walkthrough.exists():
            problems.append("gui_walkthrough.exe")
        if not exe.exists():
            problems.append("parallaxe.exe")
        if args.strict:
            if problems:
                print("DOCTOR_RESULT: MISSING (" + ", ".join(problems) + ")")
                return 3
            print("DOCTOR_RESULT: OK")
        return
    if args.qa_view:
        root = Path(__file__).resolve().parent.parent
        exe = root / "target/debug/examples/gui_walkthrough.exe"
        if not exe.exists():
            ap.error("请先 cargo build -p parallaxe-gui --example gui_walkthrough")
        out = Path(args.out).resolve()
        out.parent.mkdir(parents=True, exist_ok=True)
        # 演练只用内嵌的默认空许可和本地合成样本。计划文件写到专用临时目录，结束即清理。
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run([str(exe), args.qa_view, str(out), str(args.width or 1280),
                            str(args.height or 820)], cwd=tmp, check=True, timeout=35)
            if args.qa_view == "plan":
                doc = json.loads((Path(tmp) / "parallaxe-plan.json").read_text(encoding="utf-8"))
                if not (doc["operation"] == "PREVIEW" and doc["applied"] is False and
                        doc["target"] == "google.books" and doc["is_fixture"] and
                        doc["observed_country"] == "CN" and len(doc["choices"]) == 1):
                    raise ValueError("处置清单内容未通过验收")
                print("清单回读：Books / CN / 仅预览 / 已选 1 项 / 未应用")
        return

    if not HAS_WIN32:
        print("缺少 pywin32。请先: python -m pip install pywin32", file=sys.stderr)
        sys.exit(2)

    if args.list:
        found = []
        def cb(hwnd, _):
            if win32gui.IsWindowVisible(hwnd):
                t = win32gui.GetWindowText(hwnd)
                if args.title.lower() in t.lower():
                    found.append((hex(hwnd), win32gui.GetWindowRect(hwnd), t))
        win32gui.EnumWindows(cb, None)
        for row in found:
            print(row)
        if not found:
            print("(无匹配窗口)")
        return

    proc = None
    if not args.existing:
        if not os.path.exists(args.exe):
            print(f"找不到 {args.exe}，请先 cargo build", file=sys.stderr)
            sys.exit(2)
        print(f"[info] 启动 {args.exe} gui")
        proc = subprocess.Popen([args.exe, "gui"], cwd=os.path.dirname(os.path.abspath(args.exe)))

    try:
        deadline = time.time() + args.wait
        hwnd = None
        while time.time() < deadline:
            hit = find_window(args.title, proc.pid if proc else None)
            if hit:
                hwnd = hit[0]
                break
            time.sleep(0.2)
        if not hwnd:
            raise RuntimeError("未找到指定进程的 GUI 窗口")
        if args.width and args.height:
            resize(hwnd, args.width, args.height)
        time.sleep(1.2)
        img = capture_client(hwnd)

        out = args.out
        if args.density or args.theme:
            stem, ext = os.path.splitext(out)
            tag = "_".join(x for x in [args.density, args.theme] if x)
            out = f"{stem}-{tag}{ext}"
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
        img.save(out)
        print(f"[ok] 已保存 {out} ({img.size[0]}x{img.size[1]})")

    finally:
        if proc:
            proc.terminate()
            proc.wait(timeout=5)


if __name__ == "__main__":
    if sys.platform == "win32":
        ctypes.windll.user32.SetProcessDPIAware()
    main()

