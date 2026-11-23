# Parallaxe box 快速开始（QUICKSTART）

> 本指南帮助你用最直白的方式在 5 分钟内跑通 Parallaxe box 的离线自检、离线演练、CLI 诊断、原生 GUI 与 Agent 控制面。所有命令均与当前源码实现完全一致。

---

## 1. 环境准备与构建

- **Rust 工具链**：`rustc` / `cargo` **1.78+**（Rust 2021 edition）
- **无需 OpenSSL**：TLS 底层完全使用 `rustls`（`reqwest` 禁用默认特性，仅启用 `rustls-tls`、`http2`、`charset`）
- **单文件资产内嵌**：默认目标目录、信号规则、Profile 与依赖图均通过 `include_str!` 编入二进制，运行时不依赖外部配置目录。

```bash
# 方式 A：构建完整单文件（默认包含原生 GUI + CLI + Agent）
cargo build --release -p parallaxe-cli

# 方式 B：构建纯无头单文件（不编译 egui/eframe 与图形栈，适合服务器 / CI）
cargo build --release -p parallaxe-cli --no-default-features
```

开发期也可直接用 `cargo run -p parallaxe-cli -- <子命令>` 运行；下文示例均以已编译的 `parallaxe` 命令书写。

---

## 2. 第一步：运行离线自检（验证契约完好）

无需联网、不会发出任何网络包，也不会写入任何本地文件：

```bash
parallaxe selftest --offline
```

或使用等价快捷参数：

```bash
parallaxe --selftest
```

如需机器可读 JSON：

```bash
parallaxe selftest --offline --json
```

**预期结果**：全部 28 项离线契约用例通过（`28 passed / 0 failed`），退出码 `0`。

---

## 3. 第二步：理解「默认空许可」与离线 Fixture 演练

### 为什么直接跑 `google-all` 会发包 0 且退出码是 3？

尝试运行：

```bash
parallaxe check --headless --profile google-all --json
```

你会看到报告中：
- `coverage.catalog_size`: `135`（107 个 Google 目标 + 28 个对照目标）
- `coverage.selected_count`: `107`
- `coverage.covered`: `[]`（0 项）
- `network_request_count`: `0`
- 进程退出码：`3`（存在未覆盖/不确定项）

**这不是程序坏了，而是刻意设计的安全闸门。**  
按照合规与目标方逐址准入原则（见 [docs/compliance.md](docs/compliance.md)），内置 `assets/targets.toml` 中的 `admission.grants` 默认为空，且所有目标的 `allow_online = false`。软件绝不会在未经逐址核准的情况下对 107 个第三方站点悄悄发起自动探测。

### 用 8 个内置脱敏 Fixture 体验完整判定引擎

无需联网即可通过 `--fixture <CASE>` 在合成脱敏样本上走完「信号提取 → 分桶投票 → 9-State 判定 → 退出码归约」全流程：

| Fixture ID | 对应目标 | 演练场景 | 预期状态 | 退出码 |
|---|---|---|---|---:|
| `gemini-positive` | `google.gemini` | Gemini 内联三字码正例（仅证明页面存在此值，低可信度） | `OK` | `0` |
| `books-missing` | `google.books` | Books 有公开标题但无内联字段（不跨服务借用 Gemini 的值） | `REACHABLE_ONLY` | `0` |
| `legacy-region-negative` | `google.home` | 废弃的 `name="region"` 属性出现但零产出 | `UNKNOWN` | `3` |
| `drive-login` | `google.drive` | 302 跳转登录域：停在首跳、地区 `null` | `UNKNOWN` | `3` |
| `news-edition` | `google.news` | 跳转含 `hl/gl/ceid`：只计 1 条 `PREFERENCE` 版次证据，不当出口 | `REACHABLE_ONLY` | `0` |
| `empty-204` | `google.http204` | 204 空体仅证明连通性，不计 HTML 字段缺失 | `REACHABLE_ONLY` | `0` |
| `rate-429` | `google.models` | 429 限流：记限流迹象、不重试、不判封国 | `UNKNOWN` | `3` |
| `truncated-page` | `google.books` | 页面截断：不提取正文、不递增 stale 计数 | `UNKNOWN` | `3` |
| `conflict-tie` | `google.gemini` | 同层出现 `USA` 与 `CHN` 冲突：平票无赢家 | `CONFLICT` | `3` |

> 注：内置回归集共包含上述 9 项案例（见 `crates/core/src/fixtures.rs`）。

演练示例：

```bash
# 人读表格输出
parallaxe check --headless --fixture gemini-positive

# 单个 JSON 报告输出（无 ANSI 装饰，带 schema: parallaxe.report/v1）
parallaxe check --headless --fixture conflict-tie --json

# 逐事件 NDJSON 流输出（带 schema: parallaxe.event/v1）
parallaxe check --headless --fixture drive-login --jsonl

# 导出单文件离线 HTML 报告（内联样式，无外部 CDN）
parallaxe check --headless --fixture gemini-positive --html > report.html

# 导出防公式注入 CSV 报告
parallaxe check --headless --fixture news-edition --csv > report.csv
```

---

## 4. 第三步：目录查询、目标解释、Guard 与链路体检

```bash
# 列出全部 107 个 Google 目标及其保护/准入状态
parallaxe targets list

# 按分组筛选（如 google.ai、google.identity、control.domestic 等）
parallaxe targets list --group google.ai --json
parallaxe targets list --group control.domestic

# 查看单个目标的完整登记信息与绑定规则
parallaxe explain --target google.gemini
parallaxe explain --target google.play --json

# 解释离线案例的判定原因
parallaxe explain --target google.drive --fixture drive-login --json

# 查看某目标在依赖图中的显式依赖边
parallaxe dependencies explain --target google.drive --json

# 查看用户优先级配置（默认为空，示例文件不自动加载）
parallaxe priorities list

# Guard 只读影响闭包预览（支持 --source-report 读取已有 check --json 报告，绝不落盘）
parallaxe guard plan --dry-run --target google.notebook --json
parallaxe guard rollback --json

# 路径隔离解释（不创建或切换代理）
parallaxe isolate explain --target google.books --json

# 四列只读链路体检（默认零发包；可基于离线案例投影）
parallaxe link explain --fixture gemini-positive
parallaxe link explain --profile simple --json

# 如需显式执行一次受控系统 DNS 解析（仅允许内置目录主机，禁止与 --fixture 同用）
# parallaxe link explain --target google.home --resolve --json
```

---

## 5. 第四步：启动原生 GUI

```bash
parallaxe gui
```

GUI 采用原生 `egui/eframe` 窗口，**仅在本机运行，不开启任何网络监听端口**。在 GUI 中你可以：
- 浏览按服务家族分组的 Google 107 项 + 对照 28 项完整目录树；
- 一键演练离线脱敏案例，逐项展开 `transport / service_observed / risk_signal / local_policy` 四层证据与冲突原因；
- 在顶部切换明/暗主题与正常/极简密度（切换视图绝不改变判定结果，也不隐藏冲突与 `UNKNOWN`）；
- 在「处置中心」查看 Guard 影响闭包审计与链路体检四列视图；
- 在「自定义导入」页面导入自有或获授权的自定义检查网址与 9 类白名单检查方式（须阅读并主动勾选两段责任声明，运行前经一次性模态弹窗确认后方可对该次会话发包；详见 [docs/user-import.md](docs/user-import.md)）。

---

## 6. 第五步：接入 Agent（`--stdio` NDJSON）

无需网络端口，适合自动化脚本或编码 Agent 通过标准输入输出调用：

```bash
parallaxe agent --stdio
```

输入一行 UTF-8 JSON 请求（schema 固定为 `parallaxe.agent/v1`），即可在 stdout 收到响应与异步事件流：

```json
{"schema":"parallaxe.agent/v1","id":"req-1","op":"list_targets","args":{"group":"google.ai"}}
{"schema":"parallaxe.agent/v1","id":"req-2","op":"check","args":{"fixture":"gemini-positive"}}
{"schema":"parallaxe.agent/v1","id":"req-3","op":"selftest","args":{}}
```

完整 9 种 `op` 说明与流式取消示例请参阅 [docs/agent-interface.md](docs/agent-interface.md)。

---

## 7. 退出码速查（ADR-003）

| 退出码 | 含义 | 典型场景 |
|---:|---|---|
| **0** | 全部可接受 | 所有已测目标为 `OK` 或符合预期的 `REACHABLE_ONLY`；只读查询/离线自检全部通过 |
| **1** | 参数、配置或内部运行错误 | 未知选项、互斥参数冲突、非法代理 URI、外部 TOML 试图提权、内部 I/O 错误 |
| **2** | 存在明确异常 | 任一目标为 `RESTRICTED`、`BLOCKED` 或 `LOCAL_PATH_ERROR`（与 3 类共存时优先返回 2） |
| **3** | 存在不确定或未完成项 | 存在 `UNKNOWN`、`CONFLICT`、`SIGNAL_STALE`、`UNSUPPORTED`，或选中目标未获准/被取消/超时 |

---

## 8. 使用外部覆盖层（只收紧、不提权）

单文件二进制无需外部目录即可运行；如你需要扩展只读目录或收紧 Profile 预算，可显式传入全局参数：

```bash
# 加载外部目录覆盖层（外部层禁止包含 admission 许可，不得放宽硬预算或移除系统保护）
parallaxe --targets-dir ./my-targets --profiles-dir ./my-profiles targets list

# 加载用户真实优先级配置文件（文件内必须显式声明 example_only = false）
parallaxe --config ./my-priorities.toml priorities list
```

详细打包与覆盖层安全规则请参阅 [docs/packaging.md](docs/packaging.md) 与 [docs/priorities.md](docs/priorities.md)。
