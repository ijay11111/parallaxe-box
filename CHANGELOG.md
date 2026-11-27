# 更新日志（CHANGELOG）

本文件记录 Parallaxe box 对外发布的版本变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

> 版本口径（2026-11-24 发布裁决）：`Cargo.toml` 的 workspace 版本保持 `0.1.0`（二进制 `--version` 输出 `0.1.0`，
> 报告 schema 仍为 `parallaxe.report/v1` 不变）；**对外发布标签与本文件使用 `v0.1.0-beta.1`**。
> 这是第一个公开测试版，不代表稳定版承诺。

## 待发布

- **AI 启动检测（测试版核心功能）**：`parallaxe agent --stdio` 协议升级到 `protocol_version: 2`，在原有 9 个只读操作上追加
  `ai_status` / `ai_request_check` / `ai_request_status` / `ai_autonomous` 四个授权桥操作。默认路径 A：AI 提交一次性请求 →
  GUI 弹出人话「F3 一键授权」→ 用户确认后才由 GUI 真实发包 → AI 用 `explain` / `export_report` 讲结果；
  路径 B（opt-in）：GUI 设置打开「AI 自主调试 → 自主检测」后，AI 可在 3.5 秒间隔 / 单批 ≤10 / 优先 TCP / 403-429 即停的
  QoS 红线内自主检测与导出，报告标注 `origin=AGENT_AUTONOMOUS`、`autonomous_mode=true`；开关仅会话内存、不落盘，
  AI 可自主关闭（`ai_autonomous {enabled:false}`），权限即时收回。授权桥是当前用户临时目录内的短生命周期文件
  （无监听端口、单次有效、带 TTL、范围摘要绑定），agent 仍不接受任何 URL、不接受任意磁盘路径。
- `parallaxe ai status [--json]`：只读查看授权桥连接状态、自主检测开关投影、可真实发包的已导入目标与待确认请求；不代替用户确认。
- 服务报告展示本次 DNS 候选与实际 socket 对端的一致性、对端角色和可用的国家码与 ASN，并支持四种报告格式。原始 IP 由 `--show-ip` 显式展示。
- 产品直接给出地区限制或策略拒绝时，报告保留本次判定的证据。Google 官方可用地区说明与观测结果并列展示。

### 发行包默认规则与模式边界（2026-11-26）

- **默认规则包 v2 预置**：107 项「我的授权版」随 Windows / Web / Android / Linux 发行包分发，安装启动即可见；重复内置网址改为显式更换，受保护项在源码零发包模式仍需二次确认。授权范围支持仅本次（退出失效）与永久（用户状态目录保存、可一键撤销）。
- **源码 / 发行包边界**：普通源码构建默认零发包；设置→关于软件可经测试者确认切换发行包模式。正式 Release 流水线显式使用 `distribution-release` 编译期 feature，关于页不再显示源码开关。AI 自主调试与模式开关独立；发行包模式下 AI 自主可跑内置 Google 全量，关闭自主时仍逐次 F3。
- **QoS 停止**：HTTP 403/429 触发后立即停止启动批次后续请求；已发送请求允许完成。发行包 GUI 全量运行使用至少 3.5 秒跨目标间隔并显式确认大批量预算。

### 放行前最后一轮修复（2026-11-25/26）

- **GUI 路径 A/B 真实发包修复**：此前 GUI 授权桥启动运行时未注入会话许可，用户导入目标默认 `allow_online=false`，核心按 `ALLOW_ONLINE_FALSE` 零发包——`stage10` 的"真实发包"证据只来自 CLI 夹具。现由 GUI 在用户确认（路径 A）或会话级自主开关（路径 B）后注入**一次性、仅本次运行、可撤销**的会话许可，运行结束即消费。失败时立即向 agent 回传错误，不再无限等待。
- **设置页改为多级页**：一级栏目列表（彩色图标 + 标题 + 副标题 + 右箭头），点击进入独立二级页（返回按钮 + 居中标题 + 专业选项警示条）。**语言项特殊处理**：保持顶栏常驻「中文/EN」胶囊与设置页行内直选，不进入二级页。
- **默认视图技术码清零**：运行历史、限速记录等改用人话；机器码只在「显示技术细节」开启时出现。责任声明中文原文与 `ANDROID_SIMULATED_USER` 观测边界标识保留。
- **Android 文本输入修复**：上游 `winit 0.30.13` 的 Android 后端只处理 `KeyEvent`、忽略 `TextEvent`，软键盘文本到不了 egui 输入框。通过仓库内 `vendor/winit` 补丁转发 IME 文本（前缀差分 Commit / Backspace），使手动导入与文本导入可输入。公开发布源码已自带该补丁， clone 后可直接构建。

### 测试版边界（诚实说明，发布必读）

- **源码构建的 107 个 Google 目标仍默认零发包**：`assets/targets.toml` 全量 `allow_online=false`、`admission.grants=[]`，外部目录覆盖层禁止携带许可。git clone 普通构建保持零发包；正式发行包由编译期标记解除内置门禁，并在默认规则包范围选择时保留仅本次/永久授权边界。普通导入目标始终需要 F3。
- **授权桥是同用户文件通道**：本版未对临时目录内的授权文件做数字签名；同一操作系统用户下的其他进程理论上可伪造授权决定。**GUI 是唯一发包者与责任边界**；签名加固列为后续版本。
- **B 模式由 GUI 发包**：即便开启自主检测，`agent check` 本身仍固定 `network=false`；实际发包发生在 GUI 进程，agent 负责请求与取回脱敏报告——这是保留 consent 闸门的设计，不是 agent 直连发包。
- **Android 本地 ZIP 选择不支持**：APK 严格仅 `INTERNET` 零存储权限，`folder_picker` 在移动端无原生实现；可用"载入内置示例 ZIP 包"或文本导入。Android SAF（`ACTION_OPEN_DOCUMENT`）可作为后续方案，本版未启用。
- **本轮矩阵实测受本机网络限制**：49 项导入目标中 36 项为 `TRANSPORT_TIMEOUT`，与本机对 `www.google.com` 直连超时一致；这批结果不构成"服务不可用"或地区判定结论。


## [v0.1.0-beta.1] - 2026-11-24

第一个公开测试版。桌面（Windows）GUI/CLI/Agent、Web（WASM）与 Android（双 ABI APK）
首次作为同一版本对外提供；Linux musl 两种架构提供无头 CLI 归档（本机完成交叉编译与标记校验，未在 Linux 实机运行）；
**测试版不包含系统级阻断**，规则只提供"手动导出"。

### Added

- **Google 全服务证据矩阵**：107 个 Google 公开表面（12 个服务家族）+ 28 个非 Google 对照目标，
  逐表面独立取证；七层地区归因，9 态判定（`OK` / `RESTRICTED` / `BLOCKED` / `REACHABLE_ONLY` /
  `LOCAL_PATH_ERROR` / `CONFLICT` / `UNKNOWN` / `SIGNAL_STALE` / `UNSUPPORTED`），不使用伪精确评分。
- **三端同核**：桌面 GUI（egui/eframe）、无头 CLI（`parallaxe`）与 `parallaxe agent --stdio`（NDJSON，
  9 种只读/预览操作）共享 `parallaxe-core` 事件流，三端一致性由 parity 回放测试守护。
- **Web 端（WASM）**：`trunk build` 产出的纯前端页面，可浏览目录、运行离线 Fixture 演练、
  导出 MD/TXT 报告；非网络环境的报告带 `SCOPE_WASM_OFFLINE` 机器码。
- **Android 端（x86_64 / arm64-v8a APK）**：GameActivity 原生入口，仅申请 `INTERNET` 权限；
  报告 scope 标注 `ANDROID_SIMULATED_USER`（模拟器/模拟用户环境，非系统级观测）；
  移动布局（横屏双列、竖屏单列）与内嵌 CJK 字体覆盖。
- **设置中心**：主题（明/暗）、密度、语言（中文/英文）可在 GUI 内切换并持久化。
- **插件系统（本地声明式，无脚本执行）**：`parallaxe.plugin/v1` manifest；一轻（`sample.light.report-badge`）
  一重（`sample.heavy.report-guide`，覆盖 9 种组件）第一方示例；CLI 启用/禁用与 sha256 审批；
  GUI 插件独立页渲染；`THIRD_PARTY_PLUGINS.md` 与 `tools/verify_plugins.py` 双向校验。
- **规则手动导出**：Guard 只读计划可导出为 Clash / hosts / domain-list 等规则文件。
  CLI 必须显式 `--out-dir <DIR>`，否则拒绝写入并提示"请显式指定导出文件夹"；
  GUI 只能通过系统目录选择对话框确认目标文件夹（取消即零写入，仅写入已存在的所选目录，
  不再提供默认路径或文本框静默写入）。
  存在 `NO_SAFE_BLOCK` 时仍允许手动导出，但结果带 `EXPORT_WARNING_NO_SAFE_BLOCK`、`applied=false`。
- **四列只读链路体检**：`resolver` / 实际连接边缘 / HTTP 出口 / 地址族分列呈现，默认零发包。
- **离线能力**：28 项 `selftest --offline`、9 个脱敏 Fixture、`google-all` 零发包基准（107/0/3）。
- **自定义导入（仅 GUI）**：两段责任声明门槛、一次性会话级授权、9 类白名单检查方式。

### Changed

- 英文界面文字溢出（窄屏/宽屏/移动）与 Web 390px 窄屏溢出修复，i18n 覆盖率由测试守护。
- Android 横屏不再误判为桌面布局：移动平台固定使用窄布局族（底部导航、无桌面 Tab、最多 2 列）；
  规则导出 / 整包替换 / F3 授权 / 关于弹窗统一改为响应式安全区布局（标题自动折行、内层纵向滚动、
  底部关闭按钮常驻，横竖屏旋转实时重算宽度，修复 Android 竖屏左右裁切与横屏顶部裁切）。
- 内嵌中文字体从 196 字形/60 汉字扩充到 1015 字形/817 汉字，并新增符号回退字体，
  `font_coverage` 测试防止再次裁窄。
- 探测间隔口径统一：同目标上限 10s、跨目标可调上限 10s、默认锁定 4000ms，低于 3s 给出软提示
  （批量探测仍要求人工遵守"间隔 3–5 秒、分批、优先 TCP"）。

### Security

- **测试版不含系统级阻断**：GUI 隐藏/禁用"启用本机阻断"入口；CLI `block --apply` / `unblock --apply`
  统一返回 `SYSBLOCK_BETA_DISABLED`（退出码 3，`applied=false`）。内置目录的只读计划仍为
  `safe_to_apply=false`（fail-closed）。真实防火墙写入与回滚**未实测**。
- **导出承诺边界**：本测试版**不承诺**"只有可安全应用的规则才会导出"。带 `NO_SAFE_BLOCK` 的规则
  允许由用户手动导出，文件内与返回值均带警示码，是否使用由用户自行判断。
- 默认零发包（`admission.grants` 为空、`allow_online = false`）；默认脱敏，不读取 Cookie / 账号 /
  浏览器配置 / 环境变量凭据；无遥测、无上报。
- Android APK 仅声明 `INTERNET`；无 VPN / root / 存储 / 账号权限；Android 端不加载插件（零插件面）。
- Guard 预览恒为 `applied=false`、`rules_written=0`；`guard rollback` 恒为 `NOTHING_APPLIED`。

### Known limitations / 未测项

- **真实运营商 QoS / UDP 限速未实测**：QoS 暂停/恢复路径仅有离线合成传输回放；
  真实节点切换后的分组与真实运营商限速需要真机 SIM 卡环境。
- **系统级阻断未实测**：测试版关闭入口；Windows/Linux 管理员权限下的防火墙写入、异常中断恢复
  仅有模拟后端回放证据，未在本机真实写入。
- **Android 仅在 MuMu 12 模拟器（Android 15，x86_64 宿主内置 arm64 支持）验证**：
  未在真实 ARM 物理手机、Android 12 及以下验证；中文软键盘输入未手测。
- **Web 端未做真实网络探测**：浏览器构建以离线/回放为主，跨域探测与剪贴板 API 受浏览器限制。
- **iOS 未构建**：无 macOS/Xcode 环境。
- **Android 规则导出以预览与剪贴板复制为主**：规则导出弹窗已改为响应式布局（竖屏/横屏均不再左右或顶部裁切，
  核心操作与关闭按钮均可见），因 Android APK 严格仅声明 `INTERNET` 权限（零存储权限），不支持系统目录写入，
  点击「选择导出目录并写入...」时会提示改用剪贴板复制；如需直接落盘文件请使用桌面 GUI 或 CLI `--out-dir`。
- **Linux 未附二进制**：本版只完成 `x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl` 的类型检查；
  musl 定位为无头 CLI，GUI 需要系统图形栈。
- GUI 的文件夹选择对话框依赖系统目录选择器（Windows PowerShell FolderBrowserDialog、Linux zenity/kdialog、
  macOS osascript）；无图形会话的环境请使用 CLI 的 `--out-dir`，GUI 不会退回静默写默认路径。

### Verification（本版本实测）

- `cargo test --workspace --offline --locked -j 4`：216 passed / 0 failed（35 个测试目标，发行前从重置 target 重跑）。
- `cargo fmt --all -- --check` = 0；`cargo clippy --workspace --all-targets -D warnings` = 0。
- `parallaxe selftest --offline` = 28 / 0；`check --headless --profile google-all` = 107 选中 / 0 覆盖 / 退出码 3。
- 三端 parity 回放 10/10；i18n / schema / 字体覆盖 / 布局 / 插件契约测试全部通过。

[Unreleased]: https://github.com/ijay11111/parallaxe-box/compare/v0.1.0-beta.1...HEAD
[v0.1.0-beta.1]: https://github.com/ijay11111/parallaxe-box/releases/tag/v0.1.0-beta.1
