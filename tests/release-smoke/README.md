# 发行烟雾与界面检验

本目录是**离线**的发布前自检工具，不访问任何外部站点，不读取账号/Cookie，不写入系统规则。

| 文件 | 用途 |
|---|---|
| `release_smoke.py` | 断言 `google-all=107/0/3`、`selftest=28/0`、parity 10/10、导入零发包、无头构建 |
| `ui_audit.py` | 量化 UI：WCAG 对比度、明暗结构 IoU、卡片宽度/间距、硬编码颜色与状态兜底提示 |
| `latest/` | 最近一次运行的日志、`summary.json`、`summary.md`、`ui-audit.md`（生成物，不入库） |

## 用法

```bash
# 在仓库根目录
python tests/release-smoke/release_smoke.py
python tests/release-smoke/ui_audit.py
```

常用参数：

```bash
# 复用已构建二进制，不重新编译
python tests/release-smoke/release_smoke.py --no-build

# 平台构建任务：只跑二进制基线，不跑 cargo 测试
python tests/release-smoke/release_smoke.py --bin target/<triple>/release/parallaxe --no-build --no-cargo-tests

# UI 检验升级为阻塞（任一硬指标失败返回 1）
python tests/release-smoke/ui_audit.py --strict
```

## 口径说明

- `coverage.uncovered` 同时包含未选中的 28 个对照目标（`NOT_SELECTED`），因此断言「已选未覆盖 = 107」与「未覆盖两栏合计 = 135」，不要把总量直接和 107 比较。
- `ui_audit.py` 默认是**报告型**：浅色主题对比度等视觉取舍不会阻塞构建；需要门禁时加 `--strict`。
- 截图结构检验使用本地素材 `tests/release-smoke/latest/screenshots/`（由 `tools/capture_gui.py` 生成，`.gitignore` 排除、不随源码发布）；缺失时该项标记 SKIP，不影响构建。
