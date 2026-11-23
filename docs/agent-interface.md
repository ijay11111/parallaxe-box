# Agent `--stdio` NDJSON 控制面协议（Agent Interface）

> 本篇定义 `parallaxe agent --stdio` 的完整输入输出协议、9 个合法操作、流式取消语义与安全边界。  
> 源码实现：`crates/agent/src/lib.rs`；协议契约：[docs/architecture/02-data-model.md](architecture/02-data-model.md)。

---

## 1. 设计目标与安全边界

`parallaxe agent --stdio` 为编码 Agent、自动化脚本或本地集成方提供**无需网络监听**的结构化控制面：
- **传输方式**：仅标准输入（`stdin`）与标准输出（`stdout`），**一行一个 UTF-8 JSON 对象（NDJSON）**。
- **严禁事项**：不监听任何 TCP/UDP 端口（包括 loopback）、不提供 shell 命令执行、不接收任意 URL/请求头/凭据、不接受任意磁盘写入路径、不提供规则 `apply`。
- **单活动运行**：单个 Agent 会话内 `max_active_runs = 1`；已有 `check` 运行时，仅接受针对该 `run_id` 的 `cancel`，其他请求返回 `BUSY`。
- **进程生命周期**：`stdin` 断开（EOF）时自动请求取消活动运行并退出，绝不留驻后台守护进程；单次 `check` 返回退出码 `2` 或 `3` 不会终止 Agent 会话。
- **自定义导入边界**：按阶段 4/5 裁决，**本版本 Agent 暂不开放自定义网址/检查方式导入操作**，严格维持下方 9 个内置操作。

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

## 3. 全部 9 个合法操作（`OPERATIONS`）

| `op` | `args` 参数 | `data` 返回摘要 | 是否发事件流 |
|---|---|---|---|
| `list_targets` | `group?: string`（省略返回全部 135 目录项） | `protocol_version: 1`, `operations[9]`, `max_active_runs: 1`, `targets: [{id, group, protected, admission_status, reason}]` | 否 |
| `load_profile` | `profile: string`（必填） | `profile`, `selected_ids[]`, `budget: {run_timeout_ms, request_timeout_ms, max_concurrency, per_host}` | 否 |
| `check` | `profile?`, `group?: string \| string[]`, `target?: string \| string[]`, `fixture?`, `expect?`, `full?: bool`, `ipv4?: bool`, `fast?: bool`, `timeout_ms?: u64`, `max_concurrency?: u64` | 首个事件到达时回执 `{"run_id": "..."}`；随后逐条推送核心事件至 `RunFinished` 或 `RunCancelled` | **是（流式）** |
| `explain` | `run_id: string`, `target: string` | `{"result": ResultRecord}` 或 `{"uncovered": UncoveredItem}`（从本会话历史运行读取，不补探） | 否 |
| `selftest` | `suite?: string` | `{"run_id", "passed", "failed"}` + 完整自检事件流（断网运行） | 是 |
| `guard_plan` | `source_run_id: string`, `targets: string[]` | `{"run_id": "run-guard-..."}` + `RunStarted` / `GuardPlanReady` / `RunFinished`（只读预览，不落盘） | 是 |
| `export_report` | `run_id: string`, `format: "json" \| "html" \| "csv"` | `{"format", "content"}`（直接返回脱敏字符串内容，不接受任意磁盘路径） | 否 |
| `cancel` | `run_id: string` | 运行中返回 `{"status": "REQUESTED"}` 并触发真取消；已结束返回 `{"status": "ALREADY_TERMINAL"}`；未知 ID 返回 `UNKNOWN_RUN` | 否 |
| `isolate` | `action?: "plan" \| "explain"`, `protect?: string[]`, `defer?: string[]` | `status: "NO_EXISTING_PATH"`, `paths: []`, `limitations[]`, `selected_by: "USER"` | 否 |

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
{"id":"1","ok":true,"data":{"protocol_version":1,"operations":["list_targets","load_profile","check","explain","selftest","guard_plan","export_report","cancel","isolate"],"max_active_runs":1,"targets":[...]}}
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
| `POLICY_DENIED` | 越权参数（如 `check` 携带 `state_dir`/`proxy`/`interface`） |
| `BUSY` | 已有活动运行期间发送了除匹配 `cancel` 以外的请求 |
| `UNKNOWN_RUN` | `explain` / `export_report` / `guard_plan` / `cancel` 引用了不存在的 `run_id` |
| `INTERNAL` | 运行期内部异常（经脱敏，不泄露敏感信息） |
