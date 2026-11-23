# 三端（桌面 / Android / Web）与阻断能力说明

> 阶段 7 · 2026-11-23 · 对应 ADR-006（`docs/decisions.md`）与阻断四层策略。
> 本文件只描述**已经实现并有仓库证据**的能力；未真机验证的部分明确标注，不写成“通过”。

## 一、三端现状

| 端 | 构建方式 | 当前状态 | 证据 |
|---|---|---|---|
| 桌面（Win/Linux/mac） | `cargo build -p parallaxe-cli`（默认带 GUI） | ✅ 完整 GUI + CLI；阶段 6 已有 170 项测试中的绝大部分 | CI `build` 矩阵、`cargo test --workspace` |
| Web | `cargo check -p parallaxe-gui --target wasm32-unknown-unknown`；`trunk build`（`crates/gui/Trunk.toml`） | ✅ core 与 GUI 编译通过；Web 端为**离线解释**界面：不发网络请求、导出降级为剪贴板；❌ 尚未做浏览器真机截图验证 | 本次 `wasm32` check = 0 |
| Android | `cargo ndk -t arm64-v8a check -p parallaxe-gui` | ✅ `android_main` + `cdylib` 入口编译通过；❌ 未生成 APK、未在真机/模拟器验证安全区/触控/DPR/返回键 | 本次 NDK check = 0；`crates/gui/src/lib.rs#android_main` |
| iOS | — | 不在本阶段范围（ADR-006 D-4） | — |

### 跨端一致性

- 判定、状态机、报告字段、退出码、i18n 机器码全部来自 `parallaxe-core`；三端只做投影。
- wasm 端没有系统 DNS / 原生 TCP 能力时，`core` 返回明确的 `WASM_OFFLINE`，**不把“未实现”伪装成网络故障**，判定语义不变。
- 桌面与 GUI parity 由 `crates/gui/tests/parity_replay.rs` 覆盖（含导出 md/txt 与 core 逐字节一致）。

### 已知缺项（必须在验收时保持诚实）

1. Web 端未接入浏览器 `fetch` 探测；本轮是离线解释 + 剪贴板导出。
2. Android 仅编译验证，未出 APK、未真机验证；`assets/fonts/NotoSansSC-Regular-subset.ttf` 子集可能缺字，缺字回退系统字体（Android 无系统路径时可能出现方块）。
3. 三端“10/10 parity”指核心事件/报告投影；不是三套 UI 像素一致。

## 二、阻断能力（四层策略）

| 层级 | 实现 | 入口 | 边界 |
|---|---|---|---|
| 主方案 A：不污染式规则导入 | `guard export --format clash\|singbox\|domainlist --mode provider` | CLI；GUI 复制剪贴板 | 独立 rule-provider + Merge/Script 片段；不改用户节点/分流；无回环服务、无临时订阅端口 |
| 主方案 B：Win/Linux 系统阻断 | `guard block ... [--apply --confirm]` / `guard unblock` | CLI（GUI 按钮由 GUI 组接入） | 仅 Windows/Linux；必须 `safe_to_apply && closure_complete && reasons.is_empty()`；只阻断判定过的精确 FQDN 解析 IP；写前审计、失败回滚、按标签一键撤销 |
| 第二备选：整包替换 | `guard export --mode full --acknowledge-overwrite` | CLI（GUI 放次级菜单，不主动推荐） | 只生成完整规则，不替用户替换；导出强警示“覆盖原有分流、请先备份” |
| 第三备选：用户自主阻断 | **不进软件 UI**，仅文档链接 | [用户自主阻断指南](self-block-guide.md) | 普通用户不展示；进阶用户经此文档自行配置，本软件不代为执行 |

### 系统阻断的安全顺序

1. 先 `guard plan` 生成只读预览（`safe_to_apply` 通常为 `false`）。
2. 只有计划通过全部闸门后才允许 `guard block --apply --confirm`；缺少 `--confirm` 直接拒绝。
3. 写入前检查同名规则是否已存在；存在则拒绝，避免接管他人规则。
4. 逐条执行命令；任一步失败立即执行回滚命令，并把 `APPLY_FAILED_ROLLED_BACK` 写入审计。
5. `guard unblock --rule-tag <tag> --apply` 按标签删除；重复撤销返回 `ALREADY_ABSENT`，保持零残留。

> 当前内置目录没有“专用路径”证据，`safe_to_apply` 几乎恒为 `false`，因此真实系统阻断会返回 `SYSBLOCK_NOT_SAFE`。这是**有意的诚实行为**，不是缺陷；是否放宽由用户与 WorkBuddy 裁决。

### 红线

- 不按 IP / CDN IP / ASN 泛阻断；拒绝 IP 字面量、私网/回环地址、非候选 FQDN、命令注入标签。
- 规则清单只含 FQDN，不含 Cookie / 账号 / 凭据 / 完整响应正文。
- 导出前若存在路线变化 / 本地 QoS 疑似 / 主要信号过期，自动暂停规则生成（`EXPORT_PAUSED_FORENSIC`）。

## 三、网络取证安全（T-7-09）

- **低噪声预算**（`Budget` 默认）：同 host/节点/协议间隔 3500ms、批次 5、UDP/QUIC 并发 1、TCP 并发 3、批次冷却 45s、单节点复测 1 次；由统一调度器分配发送时刻。
- **零发包风险预览**：`check --plan [--json]` 展示目标/节点/协议分布、预计新连接/会话、并发与耗时，`zero_packet=true`。
- **节点绑定与路线一致性**：连续请求的节点 ID / 远端 / 出口变化 → 整组标记 `ROUTE_INCONSISTENT`，不合并结论。
- **返回契约**：200 ≠ OK；429/503/挑战页 ≠ 地区受限；失效信号进 `SIGNAL_STALE`；明确禁止页面语言、域名尾缀、GeoIP、单独 Server 头、节点名、货币格式作为地区判据。
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
- [ ] 是否放宽 `NO_SAFE_BLOCK`（仅影响导出，不影响系统写入）由 WorkBuddy/用户裁决。
