# GUI 与无头 CLI 使用指南及三端一致性（GUI & Headless）

> 本篇详细介绍 `parallaxe` 无头命令行工具、原生 `egui/eframe` 桌面 GUI，以及两者与 `agent --stdio` 共享同一核心事件流的三端一致性契约。  
> 源码实现：`crates/cli/src/main.rs`、`crates/gui/`；一致性测试：`crates/gui/tests/parity_replay.rs`（10/10）、`crates/cli/tests/gui_cli_agent_parity.rs`。

---

## 1. 无头 CLI 命令树（完整实装版）

单一入口二进制名为 `parallaxe`（历史别名 `node-clean` 仅用于文档追溯，不再用于新代码）。

### 1.1 全局参数（位于子命令前或后）

| 参数 | 作用与安全边界 |
|---|---|
| `--selftest` | 快捷参数，等价于 `parallaxe selftest --offline` |
| `--config <PATH>` | 加载用户显式优先级文件（要求 `example_only = false`，拒绝内置示例文件） |
| `--targets-dir <DIR>` | 叠加本地外部目标目录覆盖层（严禁含 `[admission]`，不得放宽预算或保护） |
| `--profiles-dir <DIR>` | 叠加本地外部 Profile 目录覆盖层（仅能在代码硬上限内收紧） |

### 1.2 全量子命令一览

| 子命令 | 完整语法与关键选项 | 说明与退出码 |
|---|---|---|
| `check` | `parallaxe check [--headless] [--profile NAME \| --group GROUP... \| --target ID... \| --full] [--expect KEY=VAL] [--fast] [--ipv4] [--interface REF] [--proxy URI] [--timeout MS] [--max-concurrency N] [--json \| --jsonl \| --html \| --csv] [--trace] [--request-log] [--redact \| --show-ip] [--state-dir DIR] [--fixture CASE]` | 执行只读探测或离线 fixture 演练；四种选择器互斥，四种输出格式互斥，`--redact` 与 `--show-ip` 互斥，`--fixture` 与 `--proxy`/`--interface` 互斥；退出码 `0/1/2/3` |
| `explain` | `parallaxe explain --target ID [--fixture CASE] [--json]` | 无 `--fixture` 时解释静态目录登记与绑定规则；指定 `--fixture` 时解释该离线案例的判定结果；退出码 `0/1` |
| `targets list` | `parallaxe targets list [--group GROUP] [--json]` | 默认列全部 107 项 Google 目标（JSON 同时输出 `catalog_size = 135`）；传 `--group` 可查看任意分组（如 `control.domestic`）；退出码 `0/1` |
| `selftest` | `parallaxe selftest --offline [--suite NAME] [--json]` | 运行 28 项断网契约自检（当前仅提供 `--offline`，省略 `--offline` 返回退出码 `1`）；全通过返回 `0`，失败返回 `1` |
| `guard plan` | `parallaxe guard plan [--dry-run] [--target ID...] [--source-report PATH] [--json]` | 基于本地依赖图计算影响闭包与只读审计（`GuardAudit`）；`--source-report` 只读 ≤8 MiB 的 `check --json` 报告，不发包；无论是否带 `--dry-run` 均只预览不写盘；预览被拒返回 `3` |
| `guard rollback` | `parallaxe guard rollback [--plan-id ID] [--json]` | 回滚预览；本版本无 apply 写入能力，恒输出 `NOTHING_APPLIED`（`writes_performed = 0`）；退出码 `0` |
| `link explain` | `parallaxe link explain [--profile NAME \| --group GROUP... \| --target ID...] [--fixture CASE] [--resolve] [--show-ip] [--json]` | 四列只读链路体检（默认零发包）；`--resolve`（与 `--fixture` 互斥）显式执行内置目录主机系统 DNS 去重查询；退出码 `0/1` |
| `isolate plan` / `explain` | `parallaxe isolate plan [--protect ID...] [--defer ID...] [--json]`<br>`parallaxe isolate explain --target ID [--json]` | 解释既有独立路径边界（不创建、切换或验证代理）；无独立路径时输出 `NO_EXISTING_PATH`；退出码 `0` |
| `priorities list` | `parallaxe priorities list [--json]` | 列出用户显式优先级（默认恒空，内置 `priorities.example.toml` 不作为用户状态加载）；退出码 `0` |
| `dependencies explain` | `parallaxe dependencies explain --target ID [--json]` | 列出目标主机在 `dependencies.toml` 中的显式依赖边（无边明确注明「无来源不等于专属性」）；退出码 `0/1` |
| `agent` | `parallaxe agent --stdio` | 启动无网络监听的 NDJSON 标准输入输出控制面（必须带 `--stdio`）；详见 [agent-interface.md](agent-interface.md) |
| `gui` | `parallaxe gui` | 启动原生桌面窗口（需启用默认 `gui` feature；无头构建运行此命令将清晰报错并返回 `1`） |

> 明确说明：**本版本没有 `--apply` 或 `--force` 参数**，也不提供 CLI 侧自定义 URL 导入命令。
>
> 全局 `--locale zh-CN|en-US` 只影响人类可读输出（帮助主标题、错误前缀、表格表头/行、explain 摘要、selftest 通过/失败标签）；`--json`/`--jsonl`/agent 的机器字段不随 locale 变化。

### 1.3 无头结构化输出保障

- `--json`：向 stdout 输出单个带 `schema: "parallaxe.report/v1"` 的完整报告 JSON，无 ANSI 转义码、无进度条噪音。
- `--jsonl`：向 stdout 逐行输出 NDJSON 事件流（每行 `schema: "parallaxe.event/v1"`）。
- `--html`：单文件离线 HTML，所有 CSS/脚本内联，不引用任何外部 CDN，对不可信文本完整 HTML 转义。
- `--csv`：标准表格导出，内置防公式注入转义（`= / + / - / @` 等前缀保护）。
- 结果对象新增**可选** `eligibility_notes`：只解释“访问可达 ≠ 订阅/完整功能可用”，不参与判定、不带账号状态；为空时不序列化，旧报告仍可反序列化。
- `--trace` / `--request-log`：仅向 **stderr** 输出脱敏后的阶段摘要与请求计数，绝不污染 stdout，也不输出完整请求头或凭据。

---

## 2. 原生桌面 GUI（`parallaxe gui`）

GUI 基于纯 Rust 原生框架 `egui/eframe` 构建：**不嵌入浏览器、不使用 Tauri、不读取浏览器状态、不开启任何本机/远程网络监听端口**。

### 2.1 页面组织与核心交互

1. **顶部栏（常驻）**：
   - 运行模式与导航切换（总览、服务矩阵、处置中心、自定义导入、时间线等）；
   - 常驻**明/暗主题切换**（全局仅替换 `Palette` 语义色板，布局与数据零变化）；
   - 常驻**密度切换（正常 normal / 极简 minimal）**（极简三卡视图折叠长细节，但**绝不隐藏冲突提示、UNKNOWN 原因或覆盖总数**）。
2. **左侧目录树与选集控制**：
   - 按 12 个 Google 服务家族与对照分组展示 135 项完整目录；
   - 独立标示「已登记」「在线准入」「受保护」状态；
   - 支持 `simple`、`google-all`、`full`、自定义选集与离线脱敏案例一键演练，支持运行中实时**取消**（走 OPT-1 `run_check_streaming`）。
3. **主视图与解释抽屉**：
   - **总览与服务矩阵**：并行展示「本次覆盖」与「本次未覆盖」两栏、实际网络请求计数、9 态穷举徽章与异常高亮；
   - **四层证据解释抽屉**：选中任一目标即可展开 `transport`、`service_observed`、`risk_signal`、`local_policy`、7 层地区归因、证据来源/可信度/新鲜度及冲突双方明细。
4. **处置中心（Guard + 链路体检）**：
   - 展示 Guard 只读影响闭包、`safe_to_apply=false` 原因、`GuardAudit`（`rules_written=0`、`applied=false`、`rollback=NOTHING_APPLIED`）；
   - 展示链路体检四列（`resolver` / 连接边缘 / HTTP 出口 / 地址族）与能力徽章（`port_scan=PROHIBITED`、`quic=UNTESTED`、`dns_leak=UNTESTED` 等；离线案例下自动禁用 DNS 按钮，保证 `dns_query_count=0`）。
5. **自定义导入（F1–F5）**：
   - 完整的两段责任声明勾选门槛、9 类人话白名单检查方式、默认零发包预览、以及运行前居中模态弹窗一次性会话级授权（详见 [user-import.md](user-import.md)）。
6. **响应式与无障碍**：
   - 宽屏多栏与窄屏（`< 840px`）单列布局自动切换；
   - 9 种状态全部配有明确文字与符号编码，不以红/绿颜色作为唯一区分维度。

### 2.2 永久剔除的 6 项 UI 反模式

在设计与验收中永久禁止以下 6 种误导做法：
1. 禁止伪精确「节点纯净度总分」（如 95/100）；
2. 禁止臆测代理客户端名称或节点列表；
3. 禁止大流量测速折线图；
4. 禁止默认暴露完整公网 IP 或原始响应头/Cookie；
5. 禁止把「HTTP 200」直接画成绿色「可用」；
6. 禁止缺少「本次覆盖 / 本次未覆盖」两栏对照。

---

## 3. 三端一致性契约（GUI = CLI = Agent）

`parallaxe-core` 是全项目唯一的事件生产者与判定引擎：

```text
core::runtime::run_check_streaming
  ├─→ CLI (--json / --jsonl / 表格 / HTML / CSV)
  ├─→ Agent (--stdio 异步 record 与 export_report)
  └─→ GUI (事件 reducer 消费同一 Event 枚举，零独立判定)
```

### 自动化验证基准
- **GUI 回放一致性**：`crates/gui/tests/parity_replay.rs`（10/10 全绿），验证 9 态映射、冲突共存、取消终态、窄屏/明暗/极简切换与核心输出一致。
- **三端跨进程/同样本一致性**：`crates/cli/tests/gui_cli_agent_parity.rs`，对同一离线 fixture 分别驱动 CLI、Agent 与 GUI reducer，逐字段校验 `target`、`state`、`reason`、`conflicts`、`evidence.id`、`duration_ms`、`request_count`、`coverage`、`exit_code` 与 Guard 计划原因 100% 一致。
- **纯无头构建门禁**：`cargo check -p parallaxe-cli --no-default-features --all-targets` 必须零警告通过，确保服务器与 CI 环境无需图形库即可独立部署。
