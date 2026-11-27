# 第三方插件声明（THIRD_PARTY_PLUGINS）

> 更新日期：2026-11-24 · 适用协议：`parallaxe.plugin/v1` · 校验工具：`tools/verify_plugins.py` + `cargo test -p parallaxe-core --test plugin_samples`
> 本文件是**插件**的声明来源；依赖库（Cargo crate、字体等）仍由 `THIRD_PARTY_NOTICES.md` 负责，两者不重复。

## 一、与 `THIRD_PARTY_NOTICES.md` 的分工

| 文件 | 覆盖对象 | 何时更新 |
|---|---|---|
| `THIRD_PARTY_NOTICES.md` | 编译进二进制的 Cargo 依赖、内嵌字体与数据文件 | 依赖增删、版本变更 |
| `THIRD_PARTY_PLUGINS.md`（本文件） | 随仓库分发或随安装包分发的**插件**（`manifest.json` 为一个插件的全部可执行内容） | 新增/删除插件、插件版本或 `manifest.json` 任一字节变化 |

插件不是依赖：用户不启用时，宿主不加载、不执行任何插件内容，行为与无插件版本完全一致。

## 二、当前插件清单

本项目**未内置任何第三方插件**。下表只列出仓库自带的一轻一重两个**第一方示例**，用于协议自测与界面演示；它们同样是本地目录、可随时删除，不联网、不读凭据、不参与判定。

| 插件 ID | 名称 | 作者 | 许可证 | 来源 | 用途 | 权限 | manifest 路径 | sha256 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `sample.light.report-badge` | 报告覆盖徽标（示例轻插件） | Parallaxe box contributors | MIT OR Apache-2.0 | https://github.com/ijay11111/parallaxe-box/tree/main/plugins/samples/light-report-badge | 演示轻插件能力：覆盖计数徽标、画像名改写、三个生命周期文案 | `report_counts` | `plugins/samples/light-report-badge/manifest.json` | `13a83f19b656316fc3cf2a6e1ffc78c6e7de14dbb940ec9cf055460fd4c95bdf` |
| `sample.heavy.report-guide` | 报告阅读指南（示例重插件） | Parallaxe box contributors | MIT OR Apache-2.0 | https://github.com/ijay11111/parallaxe-box/tree/main/plugins/samples/heavy-report-guide | 演示重插件能力：自带一个只读页面，含标题、徽标、提示、进度条、表格、Markdown、代码块、表单与按钮的静态描述 | 无 | `plugins/samples/heavy-report-guide/manifest.json` | `32baa5e3fe9aa0adf219270cd904cfc9c84ec4aac773459828eebfd70f5036c6` |

## 三、字段含义与审计口径

- **sha256**：对 `manifest.json` **原始字节**计算，不规范化换行、不重新序列化 JSON。宿主在启用与每次调用前都会重算并比对，字节变化即拒绝加载（fail-closed）。
- **来源**：必须是 `http`/`https` 绝对地址，不得带用户名或密码；来源不是可执行代码地址，只是溯源信息。
- **许可证**：SPDX 表达式或等价短句，必须与实际授权一致；缺失该字段的 manifest 一律拒绝解析。
- **权限**：协议当前只有一个权限 `report_counts`（宿主提供“已覆盖/已选中”两个聚合计数）。声明该权限但缺少对应能力、或声明未知权限，均校验失败。
- **用途**：仅供阅读，不构成对插件行为的担保；插件的一切文本都是**展示建议**，不写入 `state`/`reason`/`exit_code`。
- **目录**：插件以本地目录形式安装（只读 `manifest.json`，拒绝符号链接与超限文件）。安装路径属于本机用户数据，**不得**写入公开诊断、报告或日志。

## 四、如何校验（任何人可复现）

```text
# 1) 声明与 manifest 逐字段、逐字节一致（缺行、多行、哈希不符都会失败）
python tools/verify_plugins.py

# 2) Rust 侧契约测试（解析、校验、哈希、轻/重形态区分）
cargo test -p parallaxe-core --test plugin_samples

# 3) 打包与验包往返（可选分发形态，.plugin + .sha256）
python tools/pack_plugin.py pack plugins/samples/light-report-badge --out-dir dist
python tools/pack_plugin.py verify dist/sample.light.report-badge.plugin
```

校验失败时的处理顺序：先修 `manifest.json`，再重算 sha256，最后同步本表格。**不得**为了通过校验而放宽校验工具。

## 五、新增第三方插件的准入要求

1. 必须提供真实的作者、来源 URL 与许可证，并确认其许可证允许再分发；
2. 不得要求 `Cookie`、凭据、浏览器配置、账号登录态，不得替用户以任何身份发请求；
3. 不得试图影响判定结果，只能消费报告或提供展示增强；
4. 必须在本表格登记一行，sha256 与实际文件一致，否则不进入发行件；
5. 与红线冲突的插件（例如带扫码登录、Cookie 导入的下载器形态）**不收录**，即使其页面形态可作参考。
