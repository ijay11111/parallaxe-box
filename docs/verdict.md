# 9-State 判定模型、分桶冲突投票与退出码（Verdict）

> 本篇说明 Parallaxe box 如何把零散证据归约为统一的 9 态结论、如何处理多信号冲突以及如何按 ADR-003 计算进程退出码。  
> 源码实现：`crates/core/src/verdict.rs`、`crates/core/src/contracts.rs`、`crates/core/src/reports.rs`；裁决依据：[ADR-003](../docs/decisions.md)。

---

## 1. 为什么固定为 9 个顶层状态？

传统脚本习惯把结果简化成「绿/红」二值（或者把 HTTP 200 当绿、其他当红），这会掩盖大量工程事实：网页能打开但没有地区字段怎么办？两个信号互相打架怎么办？本机网络断了还是远端把你封了？

Parallaxe box 在 `contracts::State` 中**穷举锁定恰好 9 个顶层状态**（GUI 必须穷举 `match`，严禁 `_ =>` 兜底，且仅 `OK` 使用绿色）：

| State（JSON 枚举） | 语义定位 | 触发条件 | 常见误区澄清 |
|---|---|---|---|
| `OK` | 明确满足所测主张 | 提取到有效目标信号且无同桶冲突，且未触发停步或本机故障 | **绝不因单纯 HTTP 200 就标 `OK`** |
| `RESTRICTED` | 明确服务地区受限 | 服务表面给出明确、直接的地区受限文案或政策证据 | 403 无说明不是 `RESTRICTED` |
| `BLOCKED` | 明确拒绝或策略阻断 | 服务明确给出风控阻断/策略拒绝证据 | 单次 429 限流或验证码归入 `UNKNOWN` 迹象，不自动升为 `BLOCKED` |
| `REACHABLE_ONLY` | 仅证明匿名连通 | 收到合法首跳响应（如 204、普通公开页、仅有 `PREFERENCE` 版次无出口信号），但无法推导地区 | 对静态资源或登录入口，`REACHABLE_ONLY` 属于正常可接受结果 |
| `LOCAL_PATH_ERROR` | 本机网络路径故障 | 可归因的本机解析失败、建连失败、TLS 握手失败、路径超时或双栈故障 | 与远端策略封禁严格分开 |
| `CONFLICT` | 同桶多信号互相矛盾 | 同一服务/表面/归因层/地址族/路径下出现两个以上互相冲突的地区值（如 `US` 与 `CN`） | **绝不取平均、绝不强选赢家**，平票主值留空并完整列出双方证据 |
| `UNKNOWN` | 证据不足或触发安全停步 | 登录跳转、同意页、验证码、401/403/429、页面截断、字段缺失、任务取消等 | 诚实承认不知道，附带具体 `reason`（如 `REDIRECT_TO_LOGIN`、`FIELD_MISSING`） |
| `SIGNAL_STALE` | 信号解析规则连续失效 | 仅在显式 `--state-dir` 开启时，同键连续 3 次合格完整样本缺失字段 | 默认关闭 `--state-dir` 时永不触发，改报 `UNKNOWN + FIELD_MISSING` |
| `UNSUPPORTED` | 所请求能力当前不支持 | 如请求了不可验证的 `--interface` 绑定、未编译的 SOCKS 传输等 | 不静默降级为直连，明确报告不支持 |

> 注：挑战（`CAPTCHA`）、限流（`RATE_LIMITED`）、超时（`TRANSPORT_TIMEOUT`）、取消（`CANCELLED`）、无法定论（`INCONCLUSIVE`）均通过 **`reason` 与诊断子字段** 表达，不扩张顶层 9 态枚举。

---

## 2. 单目标四层呈现结构（`ResultRecord`）

每一条 `ResultRecord` 除顶层 `state`、`reason`、`confidence`、`source`、`duration_ms`、`request_count` 和 7 项 `region_layers` 外，还固定拆分 4 个正交视角，防止事实互相串味：

1. **`transport`**：
   - `attempts[]`：逐次尝试记录（`family`、`phase`、`connect_ms`、`first_byte_ms`、`http_status`、`fallback`、`request_sent`）；
   - `path_diagnostic`：路径级诊断（如 `IPv6_SLOW`、`IPv6_BROKEN`、`IPV6_ONLY_EGRESS` 等）。
2. **`service_observed`**：
   - `surface`、`observed_country`（未观察为 `null`）、`evidence_ids[]`、`reason`。
   - **铁律**：服务字段缺失时，绝不拿 Cloudflare trace、页面语言或 CDN 机房代填。
3. **`risk_signal`**：
   - `markers[]`（如 `RATE_LIMITED`、`DENIED_OR_AUTH`、`CHALLENGE`）、`evidence_ids[]`、`reason`。
   - 仅记录观察到的风险迹象，不等于「封国」或「账号异常」。
4. **`local_policy`**：
   - `protected`、`guard_eligible`、`plan_id`、`reason`。
   - `guard_eligible` 为真须同时满足：`state ∈ {RESTRICTED, BLOCKED}`、非受保护目标、可信度 ≥ `MEDIUM`。

> 若目标或检查方式来自 GUI 自定义导入，报告还会强制追加审计留痕字段：`origin = "USER_IMPORTED"`、`consent_version = "liability-v1"`、`imported_at`、`rule_digest`。

---

## 3. 分桶投票与冲突检测（`Agreement` & `Conflict`）

`crates/core/src/verdict.rs` 按五元组构造独立投票桶：

```text
bucket = target_id | surface | region_layer | observed_family | path_id
```

- **同响应不重复投票**：同一 `sample_id` 产出的相同地区值只计一票（例如 News 一次 302 中的 `hl`/`gl`/`ceid` 不会刷出三票）。
- **不可观测不入分母**：`null` 值、传输伪层（`TRANSPORT`）、以及仅表示版次的参数不会参与出口国家投票。
- **同桶冲突保留全貌**：若同一桶内出现 ≥ 2 个不同地区值：
  - 将冲突写入 `conflicts[]`（附带冲突双方的 `evidence_ids`）；
  - 在 `agreement[]` 中记录各方得票，`tie = true` 时 `winner = null`；
  - 目标顶层状态置为 **`CONFLICT`**（原因 `CONFLICTING_EVIDENCE`），`service_observed.observed_country = null`。
- **跨服务 / 跨层不同值不是伪冲突**：例如 `control.cf-trace` 的 `EGRESS_NETWORK = US` 与 `google.news` 的 `PREFERENCE = US:en` 分属不同服务与层，各自展示，不强行合成。

---

## 4. `--expect KEY=VALUE` 期望比对

CLI 支持通过 `--expect` 对已测结果做只读比对（绝不把预期值发给目标网站）：
- `state=<State>`：比对最终顶层 9 态；
- `service_country=<两位ISO国家码>`：仅比对 `service_observed.observed_country`（绝不把 `PREFERENCE` 版次当国家）；
- `family=V4|V6`：比对实际观察地址族。

输出 `expectation = { key, expected, actual, status }`，其中 `status ∈ { MATCH, MISMATCH, NOT_OBSERVABLE }`：
- 已知不匹配（`MISMATCH`）保留如实提示，不凭空把正常页面升格为 `RESTRICTED`；
- 缺失所要求量纲（`NOT_OBSERVABLE`）属于不确定，归入退出码 `3`。

---

## 5. 进程退出码聚合顺序（ADR-003 权威裁决）

退出码只在 `parallaxe-core`（`reports::aggregate_report`）中统一计算，GUI/CLI/Agent 直接消费。**严禁按数字取 `max`（否则 `3` 会错误覆盖 `2`）**，必须按以下优先级自上而下归约：

| 优先级 | 退出码 | 判定条件 | 说明 |
|---:|---:|---|---|
| **1** | **1** | 参数/用法错误、配置校验失败、外部覆盖层越权或内部运行致命错误（`Termination::Failed`） | 覆盖 clap 默认的退出码 2（ADR-005 D-16）；配置错误零发包 |
| **2** | **2** | 任一已测目标 `state ∈ { RESTRICTED, BLOCKED, LOCAL_PATH_ERROR }` | **明确异常优先于不确定项**：即使同时存在 `UNKNOWN` 或 `CONFLICT`，依然返回 `2`，报告内同时保留全部不确定项 |
| **3** | **3** | 无第 2 类异常，但满足任一条件：存在 `UNKNOWN` / `CONFLICT` / `SIGNAL_STALE` / `UNSUPPORTED`；期望比对为 `NOT_OBSERVABLE`；存在被选中却未覆盖目标（如默认空许可）；或运行被取消/超时/部分完成（`termination != Complete`） | 绝不把空许可、取消或不确定伪装成 `0`；`guard plan` 预览被拒同样返回 `3`（ADR-005 D-4） |
| **4** | **0** | 以上均未触发（所有选中且期望验证的目标均为 `OK` 或可接受的 `REACHABLE_ONLY`） | `selftest --offline` 全部断言通过（包括故意构造退出码 2/3 的测试）返回 `0`，任一断言失败返回 `1` |
