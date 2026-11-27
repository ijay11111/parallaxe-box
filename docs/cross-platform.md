# 三端（桌面 / Android / Web）与阻断能力说明

> 阶段 7 · 2026-11-23 · 对应 ADR-006（`docs/decisions.md`）与阻断四层策略。
> 本文件只描述**已经实现并有仓库证据**的能力；未真机验证的部分明确标注，不写成“通过”。

## 一、三端现状

| 端 | 构建方式 | 当前状态 | 证据 |
|---|---|---|---|
| 桌面 | `cargo build --release -p parallaxe-cli --features distribution-release` | Windows GUI+CLI+Agent 单文件构建通过（`parallaxe.exe` 12.7 MB，`--version` = 0.1.0，selftest 28/0，google-all 107/0/3）。Linux musl x86_64 + aarch64 无头类型检查通过 | 阶段 9 构建记录、release-smoke 4/4 |
| Web | 在 `crates/gui` 运行 `trunk build --offline --locked --features distribution-release` | 构建成功（含官方 Favicon）；Chrome 无头 1440px 与 390px 渲染正常（`overflowX: false`）、中文无方块、`SCOPE_WASM_OFFLINE` 可见、零控制台错误 | T-9-01/02 截图与 DOM 汇总（内部根） |
| Android | `cargo ndk -t x86_64 -t arm64-v8a build -p parallaxe-gui --features distribution-release` + `_apk_build/build_apk_gameactivity.py` | 双 ABI APK 在 MuMu 12（Android 15）安装、启动、进程存活；权限仅 INTERNET；横屏双列/竖屏单列、中文正常 | 阶段 9 真机证据（内部根） |
| iOS | — | 不在本阶段范围（ADR-006 D-4） | — |

### 跨端一致性

- 判定、状态机、报告字段、退出码、i18n 机器码全部来自 `parallaxe-core`；三端只做投影。
- Web 界面直接回放 core 的合成样本，不调用在线传输。`SCOPE_WASM_OFFLINE` 是离线范围原因码。
- 桌面与 GUI parity 由 `crates/gui/tests/parity_replay.rs` 覆盖（含导出 md/txt 与 core 逐字节一致）。

### 已知缺项（必须在验收时保持诚实）

1. Web 端未接入浏览器 `fetch` 探测；本轮是离线解释 + 剪贴板/浏览器文件保存接口导出。390px 窄宽横向溢出已修复并经无头 Chrome 复测（`scrollWidth == innerWidth == 390`）。
2. Android 已产出 GameActivity 入口双 ABI APK，并在 MuMu（x86_64 与 arm64-v8a）完成安装、启动、权限与横竖屏布局验证；内嵌 Noto Sans SC 子集由 `crates/gui/tests/font_coverage.rs` 守护（i18n 与 GUI 字符串缺字即测试失败），符号字形缺失时由内嵌 Noto Sans Math 回退。规则导出弹窗已改为响应式安全区布局（360×640 / 390×844 / 412×732 竖屏与 800×360 横屏实图与自动化边界测试通过，已重打双 ABI APK）；因 Android APK 严格仅声明 `INTERNET` 零存储权限，手机端规则导出请使用弹窗内的「复制到剪贴板」，落盘文件仍用桌面 GUI 或 CLI `--out-dir`。修复后的重打 APK 尚未在真机/模拟器二次手测，真实 ARM 物理机与设备级完整事件回放仍未验证。
3. 桌面 GUI 回放 10 项，CLI / agent / GUI 另有进程比对。

## 二、阻断能力（四层策略）

| 层级 | 实现 | 入口 | 边界 |
|---|---|---|---|
| 主方案 A：不污染式规则导入 | `guard export --format clash\|singbox\|domainlist --mode provider --out-dir <目录>` | CLI；GUI 复制剪贴板 | 独立 rule-provider + Merge/Script 片段；不改用户节点/分流；无回环服务、无临时订阅端口 |
| 系统级阻断 | 测试版不开放 | `guard status` 只读审计 | 不写入、删除或查询实时防火墙规则 |
| 第二备选：整包替换 | `guard export --mode full --acknowledge-overwrite --out-dir <目录>` | CLI（GUI 放次级菜单，不主动推荐） | 只生成完整规则，不替用户替换；导出强警示“覆盖原有分流、请先备份” |
| 第三备选：用户自主阻断 | **不进软件 UI**，仅文档链接 | [用户自主阻断指南](self-block-guide.md) | 普通用户不展示；进阶用户经此文档自行配置，本软件不代为执行 |

### 测试版的导出与审计

测试版不含系统级阻断。`guard status` 只读查看既有审计。调用旧的系统写入命令会收到 `SYSBLOCK_BETA_DISABLED`。本版未执行 Windows / Linux 真实防火墙写入或撤销验证。

内置目录缺乏专用路径证据，计划保持 `safe_to_apply=false`。若候选、闭包和保护检查均通过，唯一剩余原因是 `NO_SAFE_BLOCK`，可将静态文件手工导出到显式选择的目录，结果包含 `EXPORT_WARNING_NO_SAFE_BLOCK`、`applied=false`。用户自行审阅并决定是否导入。

### 红线

- 不按 IP / CDN IP / ASN 泛阻断；拒绝 IP 字面量、私网/回环地址、非候选 FQDN、命令注入标签。
- 规则清单只含 FQDN，不含 Cookie / 账号 / 凭据 / 完整响应正文。
- 导出前若存在路线变化 / 本地 QoS 疑似 / 主要信号过期，自动暂停规则生成（`EXPORT_PAUSED_FORENSIC`）。

## 三、网络取证安全（T-7-09）

- **低噪声预算**（`Budget` 默认）：同 host/节点/协议间隔 3500ms、批次 5、UDP/QUIC 并发 1、TCP 并发 3、批次冷却 45s、单节点复测 1 次；由统一调度器分配发送时刻。
- **零发包风险预览**：`check --plan [--json]` 展示目标/节点/协议分布、预计新连接/会话、并发与耗时，`zero_packet=true`。
- **节点绑定与路线一致性**：连续请求的节点 ID / 远端 / 出口变化 → 整组标记 `ROUTE_INCONSISTENT`，不合并结论。
生产探测循环调用服务返回契约并记录风险标记。离线回放验证了同协议队列暂停与显式恢复。真实节点切换后的分组、真实运营商 QoS 均未实测。

- **返回契约**：200 ≠ OK；429/503/挑战页 ≠ 地区受限；失效信号进 `SIGNAL_STALE`；明确禁止页面语言、域名尾缀、GeoIP、单独 Server 头、节点名、货币格式作为地区判据。HTTP 403/429 会立即停止启动批次后续请求，已发送请求允许完成。
- **QoS**：TCP 正常 + 连续/跨机场 UDP 超时 → 暂停同协议、标记本地 QoS 疑似、不重发、未测节点不标故障、需显式恢复。
- **双栈/DNS**：IPv4/IPv6 各态分列；主动指定地址族复测不覆盖首轮；DNS 波次级缓存不跨 run。
- **离线回放**：`crates/core/tests/t7_09_replay.rs` 固化 5 个真实案例（地区 JPN + Gemini 超时、TCP/UDP、IPv6 慢回落、路线不一致、保护域审计）。

## 四、结果导出（T-7-10）

- CLI：`--format md` / `--format txt`（`--locale` 生效）。
- core：`reports::to_markdown(report, locale)` / `reports::to_text(report, locale)`，与 `to_table_locale` 同一 10 列与同一 i18n key；机器码列不本地化。
- GUI：导出按钮新增 `Markdown (.md)` 与 `纯文本 (.txt)`，直接调用 core，不另拼字符串。
- 规范：Markdown 标准表格 + `|---|` + 稳定列序；`|` 转义 `\|`；空值写 `—`；txt 等宽对齐（中文按 2 宽）；无 ANSI；不输出完整正文/真实 IP/Cookie。

## 五、待办（上线前）

- [x] `Cargo.toml` 的 `repository` 已更新为 `https://github.com/ijay11111/parallaxe-box`；第三备选改为仓库内文档链接 `docs/self-block-guide.md`（无需外部 URL）。
- [ ] Web 端接入浏览器 `fetch` 探测并在 Chrome/Firefox 截图验证。
- [ ] Android 生成 APK 并真机验证（安全区、触控、DPR、返回键、键盘遮挡）。
- [ ] 系统阻断在管理员/root 环境做一次真实写入 → 撤销 → 状态比对（本轮只做 dry-run 与单元测试）。
- [x] `NO_SAFE_BLOCK` 可带警示手动导出，见上文。
