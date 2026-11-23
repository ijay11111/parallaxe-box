# 07 三种入口，一份判定

研究日期：2026-11-21。本稿是接口取舍和未来验收用例，不是已实现的命令、协议 schema 或运行输出。推荐原生 egui/eframe 给人看、无头 CLI 给批处理、标准输入输出给交互 agent；三者都不需要嵌入浏览器、监听端口或读取浏览器状态。

## 用户从哪里进入

| 入口 | 能做什么 | 不应做什么 |
|---|---|---|
| 无头 CLI（clap，命令名 `parallaxe`） | `check --headless` 可选 `--profile simple`、`--profile google-all`、`--full`、`--group` 或 `--target`；`--fast` 缩小预算并列出跳过项，`--ipv4` 限族，`--proxy` 只选用户给定的无凭据路径，`--expect` 比对已选量纲而非向站点传地区；`explain`、`targets list`、`priorities list`、`dependencies explain`、`guard plan --dry-run`、`isolate plan/explain`、`selftest --offline`（`--selftest` 快捷入口） | 超时或代理远端地址族未知不填假值、不扫描代理；`--fast` 不把少测变成全绿。`--fixture` 不触网；`--max-concurrency`、`--timeout` 只能收紧硬预算 |
| GUI（`parallaxe gui`） | 目标树选组或单项、开始/取消、单项重测；事件时间线、覆盖/未覆盖、证据/来源/新鲜度/冲突和请求计数；服务解释、Guard 干运行、报告导出、离线 fixture 演示 | 按钮仅提交与 CLI 相同的意图。重测是新运行，不覆盖旧证据；不能从颜色、语言、动画或筛选顺序再判一次 |
| `parallaxe agent --stdio` | 每行一个 NDJSON 请求/响应或事件；无需网络的控制通道（在线 `check` 本身仍按预算发包）。`list_targets` 返回目录/保护，`load_profile` 选范围，`check` 启动，`explain` 读理由，`selftest` 离线，`guard_plan` 只预览，`export_report` 输出指定本地报告，`cancel` 取消指定运行，`isolate` 仅计划/解释已有路径 | 请求与响应关联到请求 ID、事件归属运行 ID；未知动作、无效参数或凭据输入拒绝、回报脱敏错误，不影响其他请求。无 shell、无任意 URL/代理控制器、无 `apply`、不启网络控制面 |

三模式共用目录快照：**简单**只取获准的核心预设；**自选**按 profile/组/目标；**全量**指全部获准目标（`google-all` 仅 Google，`full` 加外部对照），不是忽略条款的开关。每种报告均列“**本次覆盖**”（已尝试的 ID、成功/失败、族、路径、实际请求数）和“**本次未覆盖**”（未选、未授权、未验证、预算耗尽、已取消未启动，并列原因）。已尝试但没有地区信号归已覆盖，不能计为地区已判定。

## 事件和报告：一致性怎样被检验

只有不依赖 egui 的共享 core 能生成判定和证据。GUI/CLI/agent 都消费它给出的相同运行事件；`--jsonl`/agent 保留顺序，单份 `--json` 和 GUI 最终视图是同一终态的投影。建议每事件带运行 ID 和单调序号，避免并发目标交错串线。类型与消费者的共同解释如下：

| 事件 | 三端一致解释 |
|---|---|
| `RunStarted` → `PreflightReady` | 同一个范围、匿名约束、路径可见性及预算；预检不等于探测成功 |
| `TargetStarted` → `EvidenceObserved`（可零或多次）→ `TargetFinished` | 目标 ID、路径、脱敏证据、耗时、实际请求和 core 判定；UI 不猜没有出现的字段 |
| `CoverageUpdated`、`GuardPlanReady` | 分别更新两栏覆盖与只读计划，后者仅在请求预览时出现，不表示规则已应用 |
| `Error` | 带作用域和可恢复性。目标失败不终止别的目标；致命错误使本次运行失败并发终结通知，不再伪报成功 |
| `RunFinished` 或 `RunCancelled` | 每次运行一个终结事件；取消后停止派发，标注部分结果/剩余未测。已结束或不存在的运行不二次取消 |

报告 JSON envelope 保留 `{schema, tool, version, generated_at, results[]}`；每项含 `target/state`、归因层、来源、可信度、路径、`request_count`、`evidence[]`、原因和 `duration_ms`（毫秒）。与 PLAN 的 `elapsed_ms` 为同一目标耗时，需要兼容时仅作同值别名，不另外计时。服务时长不是全局墙钟时长。`transport / service_observed / risk_signal / local_policy` 分开展示。`--json` stdout 仅一个最终文档；`--jsonl`/agent stdout 一行一个对象，诊断走 stderr，无 ANSI；`--html` 离线单文件、`--csv` 表格导出均不改变判定，导出转义不可信文本。`--trace/--request-log/--redact` 不放开凭据/整份头；IP 默认遮蔽，显式 `--show-ip` 仅限用户本地输出，公开 fixture 不含真实 IP。

**纸面验收例（未运行）**：用 03 的 Drive 首跳脱敏摘录作离线输入，三端都应看到“收到 302 → 登录域；地区 `null`”，不是账号可用。另测两条同层冲突、挑战、部分运行取消；比较同一次重放的事件序号及最终 `target/state/evidence/reason/request_count` 和覆盖集合，而不是比较屏幕颜色或生成时间。任何端显示成 OK、删掉冲突或把未启动计为成功即验收失败。

## 退出码与状态（建议交互契约）

按 [ADR-003 裁决](../../docs/decisions.md)采用原规格 §9 退出码（PLAN §5 旧映射已作废）：`0` 全部可接受 / `1` 参数、用法、配置或内部运行错误 / `2` 明确 `RESTRICTED`/`BLOCKED`/`LOCAL_PATH_ERROR` / `3` `UNKNOWN`/`CONFLICT`/`SIGNAL_STALE`/`INCONCLUSIVE`。取消、部分运行与全局超时按已观察最高严重度归类并在覆盖/原因字段标注；同时命中 2 类与 3 类取 2。下表判据据此更新。`INCONCLUSIVE`、挑战、全局超时等用原因字段，不扩张顶层枚举。

| 退出码 | 判据及边界（ADR-003 裁决后） |
|---|---|
| 0 全部可接受 | 所有已测目标为 `OK` / 可接受的 `REACHABLE_ONLY`；成功的目录、解释、无问题的离线检查。0 不保证所有站点已测——未覆盖项按上节两栏列出 |
| 1 运行/用法错误 | 未知选项、非法 profile、矛盾路径输入、输入含凭据、内部不变量或序列化失败；拒绝启动或中止运行，报脱敏错误。服务 403/未知不是内部错误 |
| 2 明确异常 | 任一目标为 `RESTRICTED`/`BLOCKED`/`LOCAL_PATH_ERROR`（或用户要求地区却只得 `REACHABLE_ONLY`、要求能力为 `UNSUPPORTED`）。同时有 3 类项时取 2，报告仍列全部不确定项 |
| 3 不确定 | 无 2 类项，但存在 `UNKNOWN`/`CONFLICT`/`SIGNAL_STALE`/`INCONCLUSIVE`；或取消/部分运行/全局超时且未观察到明确异常。标原因与覆盖，不冒充 0 或内部崩溃 |

先校验输入（错误→1）；合法运行中内部失败→1；否则按已观察最高严重度 2→3→0。agent 长驻时报告的是每个请求/运行的结果，不能因为某个目标 UNKNOWN 退出整个会话。

## 给 agy 的布局交接

8 张参考图目录（内部设计资料，不随仓库公开）与 UI-REF（内部参考说明，不随仓库公开） 给出桌面/移动 × 明暗 × normal/minimal。此处依据文件名和布局说明，不声称完成像素比对。桌面：左目标树，中部运行与覆盖，右侧证据及解释；窄屏：单列，目标名、状态文字和覆盖数优先，详情展开，取消始终可达。normal 展开路径/时延/证据，minimal 只折叠详情，不删冲突与未知原因，不改变运行范围。暗/亮两套保留相同文字/图标和对比度；不可只靠红绿或把 `UNKNOWN` 画成成功。移动是窄屏布局参考，不承诺本阶段有移动端构建。
