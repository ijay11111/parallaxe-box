# Agent `--stdio` NDJSON 控制面协议（Agent Interface）

> 本篇定义 `parallaxe agent --stdio` 的完整输入输出协议、13 个合法操作、流式取消语义与安全边界。  
> 源码实现：`crates/agent/src/lib.rs`（AI 授权桥协议在 `crates/core/src/ai_bridge.rs`）；协议契约：[docs/architecture/02-data-model.md](architecture/02-data-model.md)。

---

## 1. 设计目标与安全边界

`parallaxe agent --stdio` 为编码 Agent、自动化脚本或本地集成方提供**无需网络监听**的结构化控制面：
- **传输方式**：仅标准输入（`stdin`）与标准输出（`stdout`），**一行一个 UTF-8 JSON 对象（NDJSON）**。
- **严禁事项**：不监听任何 TCP/UDP 端口（包括 loopback）、不提供 shell 命令执行、不接收任意 URL/请求头/凭据、不接受任意磁盘写入路径、不提供规则 `apply`。
- **单活动运行**：单个 Agent 会话内 `max_active_runs = 1`；已有 `check` 运行时，仅接受针对该 `run_id` 的 `cancel`，其他请求返回 `BUSY`。
- **进程生命周期**：`stdin` 断开（EOF）时自动请求取消活动运行并退出，绝不留驻后台守护进程；单次 `check` 返回退出码 `2` 或 `3` 不会终止 Agent 会话。
- **自定义导入边界**：按阶段 4/5 裁决，**本版本 Agent 不开放自定义网址/检查方式导入操作**；v1 的 9 个内置操作保持原样，v2 追加的 4 个 `ai_*` 操作只接受**目录目标 ID**，绝不接受 URL。
- **AI 启动检测（v2）**：agent 自身仍为**零发包**；需要真实发包时只能提交一次性请求，由 GUI 在用户确认（路径 A）或用户已打开会话级「自主检测」开关（路径 B）后执行，详见第 6 节。

---

## 2. 协议信封格式

### 2.1 请求（写入 `stdin`）

```json
{"schema":"parallaxe.agent/v1","id":"req-001","op":"list_targets","args":{}}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `schema` | string | 必须精确为 `"parallaxe.agent/v1"`，否则返回 `UNSUPPORTED_SCHEMA` |
| `id` | string | 调用方提供的非空字符串请求 ID；无法解析 JSON 或缺失 `id` 时回包 `"id": null` 且会话继续 |
| `op` | string | 必须是 9 个白名单操作之一 |
| `args` | object | 操作参数对象（缺省按 `{}` 处理） |

### 2.2 直接响应（从 `stdout` 读取）

成功：
```json
{"id":"req-001","ok":true,"data":{...}}
```

失败：
```json
{"id":"req-001","ok":false,"error":{"code":"INVALID_ARGUMENT","message_safe":"缺少 profile","message_localized":"Missing profile"}}
```

> `message_safe` 保持原有稳定值（脚本可依赖）；`message_localized` 是按 CLI `--locale` 渲染的可选展示字段，未知动态消息的明细可能仍为中文。`parallaxe.agent/v1` 的 schema 与 op 数量不变。

### 2.3 异步事件通知（从 `stdout` 读取）

当执行 `check`、`selftest`、`guard_plan` 等产生核心事件流的操作时，先返回 `ok: true` 的 `run_id` 回执，再逐行输出异步事件通知：

```json
{"id":"req-002","record":{"schema":"parallaxe.event/v1","run_id":"run-check-...","seq":1,"ts":"2026-11-22T00:00:00Z","is_fixture":true,"event":"RunStarted","payload":{...}}}
```

> 说明：通知中的 `id` 始终绑定**启动该运行的请求 ID**；`record` 是 `parallaxe-core` 产出的完整事件信封，与 CLI `--jsonl` 和 GUI 事件流逐字段一致。

---

## 3. 全部 13 个合法操作（`OPERATIONS`，`protocol_version: 2`）

> v1 的 9 个操作字段与行为完全不变；v2 只追加 `ai_status` / `ai_request_check` / `ai_request_status` / `ai_autonomous`，`schema` 仍为 `parallaxe.agent/v1`。

| `op` | `args` 参数 | `data` 返回摘要 | 是否发事件流 |
|---|---|---|---|
| `list_targets` | `group?: string`（省略返回全部 135 目录项） | `protocol_version: 2`, `operations[13]`, `max_active_runs: 1`, `targets: [{id, group, protected, admission_status, reason}]` | 否 |
| `load_profile` | `profile: string`（必填） | `profile`, `selected_ids[]`, `budget: {run_timeout_ms, request_timeout_ms, max_concurrency, per_host}` | 否 |
| `check` | `profile?`, `group?: string \| string[]`, `target?: string \| string[]`, `fixture?`, `expect?`, `full?: bool`, `ipv4?: bool`, `fast?: bool`, `timeout_ms?: u64`, `max_concurrency?: u64`, `min_interval_ms?: u64`, `inter_target_interval_ms?: u64`, `batch_size?: u64`, `batch_cooldown_ms?: u64`, `node_retest?: u64`, `plan?: bool`, `confirm_budget?: bool`, `node_sweep?: bool` | 首个事件到达时回执 `{"run_id": "..."}`；随后逐条推送核心事件至 `RunFinished` 或 `RunCancelled` | **是（流式）** |
| `explain` | `run_id: string`, `target: string` | `{"result": ResultRecord}` 或 `{"uncovered": UncoveredItem}`（从本会话历史运行读取，不补探） | 否 |
| `selftest` | `suite?: string` | `{"run_id", "passed", "failed"}` + 完整自检事件流（断网运行） | 是 |
| `guard_plan` | `source_run_id: string`, `targets: string[]` | `{"run_id": "run-guard-..."}` + `RunStarted` / `GuardPlanReady` / `RunFinished`（只读预览，不落盘） | 是 |
| `export_report` | `run_id: string`, `format: "json" \| "html" \| "csv" \| "md"` | `{"format", "content"}`（直接返回脱敏字符串内容，不接受任意磁盘路径） | 否 |
| `cancel` | `run_id: string` | 运行中返回 `{"status": "REQUESTED"}` 并触发真取消；已结束返回 `{"status": "ALREADY_TERMINAL"}`；未知 ID 返回 `UNKNOWN_RUN` | 否 |
| `isolate` | `action?: "plan" \| "explain"`, `protect?: string[]`, `defer?: string[]` | `status: "NO_EXISTING_PATH"`, `paths: []`, `limitations[]`, `selected_by: "USER"` | 否 |
| `ai_status` | `{}` | `available`, `host`, `autonomous_enabled`, `session_only`, `imported_targets[]`（仅 ID）、`pending_requests`, `qos` | 否 |
| `ai_request_check` | `profile?` **或** `targets?: string[]`（目录 ID，禁 URL）、`purpose?` | `request_id`, `status: "PENDING"`, `mode: "USER_CONFIRMATION" \| "AUTONOMOUS"`, `expires_at`, `min_interval_ms`, `batch_size` | 否（真实执行由 GUI 桥完成） |
| `ai_request_status` | `request_id: string`, `wait_ms?: u64`（≤30000） | `status: PENDING \| APPROVED \| COMPLETED \| DENIED \| FAILED \| EXPIRED`；COMPLETED 时返回 `run_id`，报告自动装载进本会话 | 否 |
| `ai_autonomous` | `enabled: bool`（**只接受 `false`**）、`reason?` | `status: "REVOKED"`, `autonomous: false`, `confirmed_by_host` | 否 |

### `check` 的安全参数禁令（`POLICY_DENIED`）

在 `check` 请求的 `args` 中，**严禁传入 `state_dir`、`proxy`、`interface`**（单条 Agent 请求不能追加磁盘状态目录、自定义代理或网卡绑定），一旦出现立刻返回 `POLICY_DENIED`；任何未登记的多余键均返回 `INVALID_ARGUMENT`。

---

## 4. 流式 `check` 与运行中 `cancel` 交互示例

阶段 4 通过 `runtime::run_check_streaming` 实现了真正的运行中流式推送与取消：

### 示例 1：能力握手 + 离线 Fixture 探测 + 导出报告

输入（逐行写入 `stdin`）：
```json
{"schema":"parallaxe.agent/v1","id":"1","op":"list_targets","args":{"group":"google.ai"}}
{"schema":"parallaxe.agent/v1","id":"2","op":"check","args":{"fixture":"gemini-positive"}}
```

输出（从 `stdout` 逐行读取，示意）：
```json
{"id":"1","ok":true,"data":{"protocol_version":2,"operations":["list_targets","load_profile","check","explain","selftest","guard_plan","export_report","cancel","isolate","ai_status","ai_request_check","ai_request_status","ai_autonomous"],"max_active_runs":1,"targets":[...]}}
{"id":"2","ok":true,"data":{"run_id":"run-check-1234-1700000000-0"}}
{"id":"2","record":{"schema":"parallaxe.event/v1","run_id":"run-check-1234-1700000000-0","seq":1,"ts":"...","is_fixture":true,"event":"RunStarted","payload":{...}}}
{"id":"2","record":{"schema":"parallaxe.event/v1","run_id":"run-check-1234-1700000000-0","seq":2,"ts":"...","is_fixture":true,"event":"PreflightReady","payload":{...}}}
...
{"id":"2","record":{"schema":"parallaxe.event/v1","run_id":"run-check-1234-1700000000-0","seq":7,"ts":"...","is_fixture":true,"event":"RunFinished","payload":{"report":{...}}}}
```

拿到 `run_id` 后可继续查询或导出：
```json
{"schema":"parallaxe.agent/v1","id":"3","op":"explain","args":{"run_id":"run-check-1234-1700000000-0","target":"google.gemini"}}
{"schema":"parallaxe.agent/v1","id":"4","op":"export_report","args":{"run_id":"run-check-1234-1700000000-0","format":"json"}}
```

### 示例 2：运行中途实时取消

当 `check` 返回 `{"id":"2","ok":true,"data":{"run_id":"run-check-..."}}` 且事件流尚未结束时，发送：
```json
{"schema":"parallaxe.agent/v1","id":"c1","op":"cancel","args":{"run_id":"run-check-..."}}
```

Agent 立即回复 `{"id":"c1","ok":true,"data":{"status":"REQUESTED"}}`，通过 `watch::Sender<bool>` 通知核心调度器停止派发后续目标，已启动目标结清后以 `RunCancelled` 事件收尾（绝不丢失已完成结果，也不伪造 `RunFinished`）。

---

## 5. 标准错误码表

| 错误码 | 含义 |
|---|---|
| `INVALID_ARGUMENT` | 非法 JSON、缺少 `id`/`op`、未知 `op`、未知目标/Profile、互斥选择器冲突或非法参数类型 |
| `UNSUPPORTED_SCHEMA` | `schema` 不是 `"parallaxe.agent/v1"` |
| `POLICY_DENIED` | 越权参数（如 `check` 携带 `state_dir`/`proxy`/`interface`；`ai_autonomous` 试图打开自主检测；授权范围与请求不匹配） |
| `BUSY` | 已有活动运行期间发送了除匹配 `cancel` 以外的请求 |
| `UNKNOWN_RUN` | `explain` / `export_report` / `guard_plan` / `cancel` 引用了不存在的 `run_id` |
| `BRIDGE_UNAVAILABLE` | 未检测到 GUI 授权桥（GUI 未启动或状态超过 120 秒未刷新）；此时保持零发包 |
| `AUTONOMOUS_DISABLED` | 自主检测未开启且请求试图按自主模式执行（保留给宿主/未来桥实现，当前由 `mode` 字段显式表达） |
| `INTERNAL` | 运行期内部异常（经脱敏，不泄露敏感信息） |

---

## 6. AI 启动检测授权桥（任务 A/B）

### 6.1 两条路径

- **路径 A（默认）**：agent 发 `ai_request_check` → 用户在自己打开的 Parallaxe box 桌面 GUI 里看到「AI 检测一次性授权」人话弹窗 → 点确认 → GUI 用**已导入的用户目标**真实发包 → 脱敏报告回传 → agent 用 `ai_request_status` 取回 `run_id`，再用 `explain` / `export_report` 讲结果。
- **路径 B（opt-in）**：用户在 GUI 设置打开「AI 自主调试 → 自主检测」（默认关闭）后，`ai_request_check` 返回 `mode: "AUTONOMOUS"`，GUI 在 QoS 红线内自动执行并标注 `origin=AGENT_AUTONOMOUS` / `autonomous_mode=true`；agent 可用 `ai_autonomous {enabled:false}` **自主关闭**该开关，权限即时收回。

### 6.2 桥的文件协议（同用户、无监听端口）

目录：`%TEMP%\\parallaxe-ai-bridge`（可用环境变量 `PARALLAXE_AI_BRIDGE_DIR` 覆盖；自动化/测试显式指定）。

| 文件 | 写入方 | schema | 说明 |
|---|---|---|---|
| `status.json` | GUI（只读投影） | `parallaxe.ai.status/v1` | `autonomous_enabled`、`session_only: true`、`imported_targets[]`（仅 ID，不含 URL） |
| `requests/<id>.json` | agent | `parallaxe.ai.request/v1` | 范围（profile 或目标 ID）、`nonce`、QoS 下限、`expires_at`（120 秒） |
| `decisions/<id>.json` | GUI 用户确认 | `parallaxe.ai.decision/v1` | `decided_by: "USER"`、`user_confirmed: true`、`scope_digest`、`autonomous` |
| `results/<id>.json` | GUI | `parallaxe.ai.result/v1` | `COMPLETED / DENIED / FAILED` + **脱敏报告**（`set_show_ip(false)`） |
| `controls/*.json` | agent | `parallaxe.ai.control/v1` | 只允许 `DISABLE_AUTONOMOUS`；GUI 消费后立即删除 |

约束：
1. 单次有效：请求 120 秒、决定 180 秒、结果 1 小时后清理；
2. agent 校验 `nonce` + `scope_digest`，授权范围被替换即 `POLICY_DENIED`；
3. 目标只接受目录 ID（小写字母/数字/`.`/`_`/`-`），**任何 URL、控制字符、`@` 都会被拒绝**；
4. QoS 强制下限：`min_interval_ms ≥ 3500`、`batch_size ≤ 10`、单次 ≤20 个目标、优先 TCP、遇 403/429 停止；
5. 报告只回传脱敏内容，不写任意磁盘路径。

### 6.3 典型会话

```json
{"schema":"parallaxe.agent/v1","id":"a1","op":"ai_status","args":{}}
{"schema":"parallaxe.agent/v1","id":"a2","op":"ai_request_check","args":{"targets":["user.example-home"],"purpose":"用户确认后观测首跳"}}
{"schema":"parallaxe.agent/v1","id":"a3","op":"ai_request_status","args":{"request_id":"ai-req-...","wait_ms":30000}}
{"schema":"parallaxe.agent/v1","id":"a4","op":"explain","args":{"run_id":"run-live-ai-bridge-a","target":"user.example-home"}}
{"schema":"parallaxe.agent/v1","id":"a5","op":"export_report","args":{"run_id":"run-live-ai-bridge-a","format":"json"}}
{"schema":"parallaxe.agent/v1","id":"a6","op":"ai_autonomous","args":{"enabled":false,"reason":"批量测试结束"}}
```

### 6.4 已知限制（发布前如实披露）

1. 桥是**同用户本地文件信任通道**：同一用户下的其他进程理论上可以伪造 `decisions/*.json`；本测试版不做签名，宿主（GUI）始终是实际发包者与责任边界。
2. 内置 107 个 Google 目录目标在 `assets/targets.toml` 中保持 `allow_online=false` 且 `admission.grants=[]`。**源码普通构建**因此无论 A/B 都不会对内置目录真实发包；**正式发行包构建**由编译期 `distribution-release` 标记放行，AI 自主开启时可操作内置全量，关闭时仍需逐次 F3。普通导入目标始终走用户导入 + F3 一次性会话许可。
3. GUI 关闭后，桥状态 120 秒后失效；agent 的请求在 TTL 内没有用户确认会 `EXPIRED`，不产生任何发包。
4. 目前 App 只有桌面 GUI 承载桥；Android/Web 无授权桥，`ai_status.available=false`。
