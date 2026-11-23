# Parallaxe box 架构说明（ARCHITECTURE）

> 本文面向开发者、安全审阅者与维护人，说明 Parallaxe box 的工作区划分、共享核心设计、数据契约、安全边界与资产机制。架构设计详见 [docs/architecture/01-modules.md](docs/architecture/01-modules.md) 至 `09-signals-and-fixtures.md`，关键工程裁决见 [ADR-001 ~ ADR-005](docs/decisions.md)。

---

## 1. 核心架构原则：一次判定，三个入口

Parallaxe box 采用**严格单向依赖的多 Crate Rust Workspace**（Rust 2021 edition，MSRV `1.78+`），交付单个跨平台可执行文件 `parallaxe`（并支持 `--no-default-features` 编译纯无头版）：

```text
                       ┌──────────────────────┐
                       │    parallaxe-cli     │  组合根（二进制 parallaxe）
                       │  clap 解析 / 入口分发  │  feature: default = ["gui"]
                       └──────┬───────┬───────┘
                              │       │ (optional: gui feature)
                  ┌───────────┘       └───────────┐
                  ▼                               ▼
     ┌────────────────────┐           ┌────────────────────┐
     │  parallaxe-agent   │           │   parallaxe-gui    │
     │  NDJSON --stdio    │           │  原生 egui/eframe   │
     │  9 op / 无网络监听   │           │  纯事件投影 / 零判定  │
     └─────────┬──────────┘           └─────────┬──────────┘
               │                                │
               └──────────────┬─────────────────┘
                              ▼
                  ┌───────────────────────┐
                  │     parallaxe-core    │  唯一事实与判定核心
                  │  零 egui / 零 clap     │  只读 / 匿名 / 有预算
                  └───────────────────────┘
```

### 三条架构铁律
1. **核心纯净**：`parallaxe-core` 绝不依赖 `egui`、`eframe`、`clap` 或任何图形库/浏览器引擎（由集成测试 `crates/core/tests/architecture.rs` 强约束）。
2. **三端零分叉**：CLI（表格/JSON/JSONL/HTML/CSV）、Agent（`--stdio` NDJSON）与 GUI（原生窗口）均通过共享事件流与终态 `Report` 投影呈现，严禁任何适配层自写判定规则、国家推导或退出码映射。
3. **无头与图形同源**：`cargo check -p parallaxe-cli --no-default-features` 可在不引入任何图形栈的情况下编译完整 CLI + Agent 核心。

---

## 2. Workspace Crate 与模块职责

### 2.1 四大 Crate 清单

| Crate | 路径 | 职责 | 关键约束 |
|---|---|---|---|
| `parallaxe-core` | `crates/core/` | 目录加载、匿名传输、信号提取、9-State 判定、Guard 闭包、四列链路体检、事件总线、脱敏报告、离线自检 | 零 GUI/CLI 依赖；默认空许可零发包；禁用 Cookie Store |
| `parallaxe-cli` | `crates/cli/` | 二进制 `parallaxe` 入口；解析命令行参数并投影 core 结果与退出码（0/1/2/3） | `default = ["gui"]`；`--no-default-features` 编译纯无头版 |
| `parallaxe-agent` | `crates/agent/` | `parallaxe agent --stdio` 的 NDJSON 会话管理与 9 种安全操作分发 | 无监听端口、无远程 shell、无导入 op、支持流式 `check` 与运行中 `cancel` |
| `parallaxe-gui` | `crates/gui/` | 原生 `egui/eframe` 图形界面（总览、服务矩阵、极简模式、处置中心、自定义导入、时间线与解释抽屉） | 消费同一核心事件流；9-State 穷举 `match` 零判定 |

### 2.2 `parallaxe-core` 内部模块分工

```text
crates/core/src/
├─ contracts.rs   纯值契约：9-State、7 层 Layer、Evidence、Path、ResultRecord、Coverage、GuardPlan、Report、Event
├─ targets.rs     include_str! 加载目录/规则等 5 份内嵌 TOML（i18n.rs 另载 2 份语言包、eligibility.rs 载 1 份资格说明）、外部覆盖层防提权校验、准入状态机、选集展开、受控用户导入与一次性会话许可
├─ probe.rs       受预算约束的匿名传输端口（dyn Transport + HttpTransport）：reqwest + rustls、显式空 Cookie、无跟跳、≤16 MiB 有界读体、v6→v4 受控回落
├─ network.rs     只读本机接口观测（if-addrs）与 6 种双栈路径诊断（不改路由/代理/DNS）
├─ signals.rs     安全停步检测（登录/同意/挑战/401/403/429）与 9 种白名单 reader 提取器（防 ReDoS）
├─ verdict.rs     按（服务/表面/层/地址族/路径）分桶投票、冲突检测、平票无赢家、四层投影、可选 --state-dir 规则新鲜度 N=3
├─ runtime.rs     运行协调器：run_check_streaming（OPT-1 流式事件 + watch::Receiver 实时取消）与 run_check 同步包装
├─ events.rs      EventLog 单写者顺序编号（seq 从 1 递增）与可选 mpsc 流式事件分发（EventLog::with_sink）
├─ guard.rs       有穷队列反向依赖 + 共享 CDN 影响闭包、三否决原因固定排序、只读审计 GuardAudit（applied=false, rules_written=0）
├─ linkcheck.rs   四列只读链路体检投影（resolver / connection_edge / http_egress / address_family）与显式 --resolve 系统 DNS 去重查询
├─ reports.rs     表格 / JSON / JSONL / 单文件离线 HTML / 防公式注入 CSV 投影与 ADR-003 退出码归约
├─ state.rs       显式 --state-dir 下的 freshness-v1.json 原子读写与 7 天过期窗口（默认关闭零写盘）
├─ fixtures.rs    9 个内置脱敏合成离线案例（mode=FIXTURE，network_request_count=0）
└─ selftest.rs    28 项断网契约自检套件（离线验证安全边界、预算、判定与三端契约）
```

---

## 3. 事件流与流式取消架构（OPT-1）

所有运行（在线、离线 fixture、Guard 预览、离线自检）都产出统一信封格式的不可变事件流（`schema = "parallaxe.event/v1"`），按 `(run_id, seq)` 单调递增：

```text
RunStarted
  → PreflightReady        (冻结选集、预算、准入与跳过列表；空许可时在此记录 0 获准)
  → [目标并发执行，全局 ≤6 / 单主机 ≤2]
       TargetStarted      (仅对实际/模拟尝试触发)
       EvidenceObserved*  (0..N 条已脱敏白名单证据)
       TargetFinished     (单目标 ResultRecord 终态)
       CoverageUpdated    (两栏完整快照：covered + uncovered)
  → GuardPlanReady?       (仅显式预览请求触发)
  → Error*                (运行内脱敏错误，区分 global/target/request)
  → RunFinished | RunCancelled   (互斥唯一终态，携带完整 Report 与 exit_code)
```

- **流式推送与真取消**：阶段 4 落地的 `runtime::run_check_streaming` 接收 `tokio::sync::watch::Receiver<bool>` 取消信号与 `mpsc::UnboundedSender<Event>` 事件通道。
- GUI 和 `parallaxe agent --stdio` 均消费该流式接口：用户点击取消或 Agent 收到针对活动 `run_id` 的 `cancel` 时，调度器立即停止派发新目标，未完成目标标为 `CANCELLED`，最终发出 `RunCancelled`（绝不伪造为 `RunFinished` 或退出码 `0`）。

---

## 4. 内嵌资产与数据契约

### 4.1 单文件内嵌资产（`assets/`）

`crates/core/src/targets.rs` 在编译期通过 `include_str!` 嵌入 5 份目录/规则 TOML；`i18n.rs` 另嵌入 2 份语言包，`eligibility` 模块嵌入 1 份资格说明（合计 8 份，零外部运行依赖）：

| 文件 | 用途 | 关键规模 / 基线 |
|---|---|---|
| `assets/targets.toml` | 注册目录 + 准入表 | 107 Google 目标 + 28 对照目标 = 135 项；`admission.grants = []`；`allow_online = false` |
| `assets/signal-rules.toml` | 停步策略 + 信号规则 | 12 条规则（11 条启用 + 1 条禁用负例 `rule.legacy-region`） |
| `assets/profiles.toml` | 预设范围 + 硬预算声明 | 6 个 profile（`simple`/`google-all`/`full`/`business`/`study`/`media`） |
| `assets/dependencies.toml` | 保护种子 + CDN 分类 + 依赖边 | 35 精确保护域 + 10 保护后缀 + 4 保护家族 + 7 CDN 分类项 + 4 条已验证显式边 |
| `assets/priorities.example.toml` | 用户优先级格式示例 | `example_only = true`（不作为用户状态自动加载） |
| `assets/eligibility-notes.toml` | 静态资格说明（访问≠订阅） | 23 条闸门说明；覆盖 12 服务族 + 通用闸门；`verified=false`，不参与判定 |
| `assets/i18n/zh-CN.toml` | 默认中文语言包 | CLI/报告/状态/GUI 标签/资格说明 key |
| `assets/i18n/en-US.toml` | 英文语言包 | 与 zh-CN 同 key；法律文本待用户确认（stub） |

### 4.2 外部覆盖层防提权规则

运行期可通过 `--targets-dir`、`--profiles-dir`、`--config` 叠加外部本地配置，但 core 在加载期（任何网络 I/O 之前）强制执行以下防线：
- 外部 `targets` 层**严禁包含 `[admission]` 许可**，不得放宽预算或取消系统 `protected = true`；
- 外部 `profiles` 层只能在代码硬上限（`HARD_LIMITS`：全局并发 6、单主机 2、逻辑 GET 1、额外尝试 1、自动跟跳 0、单响应 16 MiB、v6 回落门槛 8000 ms）内收紧；
- `--config` 拒绝 `example_only = true` 的示例文件；
- 任一校验失败直接返回退出码 `1`，零发包。

### 4.3 稳定 JSON Schema 族

| Schema 标识 | 对应输出 |
|---|---|
| `parallaxe.report/v1` | `check --json` 及终态事件中的完整报告 |
| `parallaxe.event/v1` | `check --jsonl` 与 Agent 异步事件记录 |
| `parallaxe.agent/v1` | `agent --stdio` 请求/响应协议 |
| `parallaxe.guard-audit/v1` | Guard 只读预览附带的审计对象 |
| `parallaxe.guard-rollback/v1` | `guard rollback --json` 只读回滚状态 |
| `parallaxe.link/v1` | `link explain --json` 四列链路体检报告 |
| `parallaxe.explain/v1` | `explain --fixture --json` 离线案例解释 |
| `parallaxe.isolate/v1` | `isolate plan/explain --json` 路径隔离解释 |
| `parallaxe.freshness/v1` | 显式 `--state-dir` 下的规则新鲜度计数文件 |
| `parallaxe.fixture/v1` | `tests/fixtures/<target_id>/<case>/meta.json` 离线契约元数据 |

---

## 5. 安全与合规架构防线

1. **账号与凭据物理隔离**：`reqwest` 禁用默认特性且未启用 `cookies`；每次请求新建无状态匿名客户端、设置 `Policy::none()`（禁止自动跟跳）、`retry::never()`、`pool_max_idle_per_host(0)`，并显式发送空 `Cookie` 头（`crates/core/src/probe.rs`）。
2. **自定义导入隔离（F1–F5）**：用户导入的网址强制进入 `user.*` 命名空间，默认 `allow_online = false`、`protected = true`（禁止进入 Guard 候选），运行前必须经一次性会话许可（`SessionGrant`）原子消费，且不污染内置 `assets/`。
3. **Guard 只读闭包与写操作门控**：闭包在内存按有穷队列（`MAX_CLOSURE_NODES = 10_000`、`MAX_EDGES_SCANNED = 100_000`）反查入边与关联边；依赖超过 365 天或落在未来即标 `closure_complete = false` 并否决；三否决原因固定按 `GUARD_DEFERRED_SHARED_CDN` > `BLOCK_DEFERRED_DEPENDENCY_CONFLICT` > `NO_SAFE_BLOCK` 排序。hosts / 防火墙 / loopback API 在当前版本全部未开放。
4. **验证基准**：当前工作区已固化 **138 项自动化测试**（含 Guard、链路体检、流式取消、自定义导入、状态目录与三端 parity）、**28/0 离线自检**、**10/10 GUI 回放一致性**以及 `google-all = 107/0/3` 零发包基线。

---

## 6. 关键架构决策记录（ADR）

| ADR | 主题 | 核心裁决摘要 |
|---|---|---|
| [ADR-001](docs/decisions.md) | 域名对照表计数口径 | 404 源不计为可复核来源；社区表仅作交叉核对，不证明域名独占或可封 |
| [ADR-002](docs/decisions.md) | 历史样例与新依据冲突 | 政策看官方、响应看带上下文直接观测；ccTLD 归并后不投票；修正 `crl.pki.goog` 拼写；保留 `google.notebook` 旧 ID |
| [ADR-003](docs/decisions.md) | 报告、9-State 与退出码 | 顶层固定 9 态；退出码采用原规格 `0/1/2/3`（2 优先于 3）；单目标耗时统一为 `duration_ms` |
| [ADR-004](docs/decisions.md) | 仓库根、规则拆分与 `SIGNAL_STALE` | 仓库根固定工作区根；信号规则独立为 `signal-rules.toml`；`SIGNAL_STALE` 仅显式 `--state-dir` 开启，默认零写盘 |
| [ADR-005](docs/decisions.md) | 阶段 2 工程边界确认 | 确认 23 项实现边界（含 `targets list` 默认 107/`catalog_size=135`、`guard plan` 拒绝预览退出码 3、`--interface` 不静默直连等） |
