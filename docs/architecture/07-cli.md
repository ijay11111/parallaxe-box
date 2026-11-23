# 07 CLI 命令树、参数与退出码

阶段 1 · 2026-11-21。`parallaxe` 为单一入口；`node-clean` 仅历史别名。下列是设计接口，不是已安装命令。CLI 解析输入后调用共享 core，不自行拼接 URL、推国家或归约退出码。

```text
parallaxe
  check [--headless] [选择器] [路径] [预算] [输出]
  explain --target ID [--fixture CASE]             目录解释，或解释离线样本
  targets list [--group GROUP] [--json]           含保护、验证与准入状态
  selftest --offline [--json]                     --selftest 是离线快捷方式
  guard plan --dry-run [--target ID] [--fixture CASE]
  isolate plan --protect ID --defer ID            仅预览已有路径
  isolate explain --target ID
  priorities list                                 仅列用户显式选择
  dependencies explain --target ID                列证据边/反向影响
  agent --stdio                                   NDJSON，不能网络监听
  gui                                             非无头构建；只投影事件
```

`guard plan` 即使省略 `--dry-run` 也只预览；没有供引用的真实结果时只解释资格不足，不自动启动 check。`isolate` 不接代理控制器、不部署新线路。运行结束的 CLI 不假设有隐藏历史文件；`explain` 无 fixture 时只解释静态目录。agent 内可引用同会话的 run_id。全局 `--config`、`--targets-dir`、`--profiles-dir` 依 [03](03-schemas.md) 单次读取，不提权。`--apply`、`--force` 非本阶段有效参数。

| check 参数 | 语义、冲突和上限 |
|---|---|
| `--profile NAME / --group GROUP / --target ID / --full` | 四类互斥，不写为隐式 simple。同类 group/target 可重复，取并集、稳定去重；组名和目标 ID 位于不同命名空间。`--full` 等价于 `--profile full`。未知键返回 1，不回落到全量。显式选择仍需逐址许可 |
| `--headless / --fast` | 前者不初始化图形系统；后者收紧运行时限到至多 20000 ms、全局并发至多 4，并跳过可选独立链路探测，标 `FAST_SKIPPED`；不把 minimal 视图当 fast 或删未知项 |
| `--ipv4 / --interface REF / --proxy URI` | `--ipv4` 禁 v6 及回落，不保证代理的远端族。interface 为已存在接口的本地引用；proxy 仅用户给定、无凭据的既有代理。二者同时用只能表明绑定到代理的本地连接，必须验证是否支持；不支持时报告 UNSUPPORTED，不静默直连。拒绝订阅、路径查询片段、认证输入；G5 默认不成对联网 |
| `--expect KEY=VALUE` | v1 可比较 `state=<枚举>`、`service_country=<两字母国家码>`、`family=V4或V6`；语义分别取实际 state、明确服务观察国家、实际路径族。只写比较结果 MATCH/MISMATCH/NOT_OBSERVABLE，不向站点提交值，不把版次当国家。已知不同只作提示，不单独升为 RESTRICTED/退出 2；缺失所要求量纲属于不确定（3）。脚本要筛出已知偏差，应读比较字段，不把退出码当“国家相等” |
| `--timeout MS / --max-concurrency N` | 正整数，分别收紧**整次运行**截止/全局槽位；有效值不超过 profile 和硬限，超出用户可设范围报 1。请求截止再取剩余时间与 profile 的 request_timeout_ms 的较小值；每主机≤2、额外尝试≤1、体积≤16 MiB、回落门槛不可从入口放宽 |
| `--json / --jsonl / --html / --csv` | 选择一种 stdout 格式，不可混用；无格式时人读表。JSON 单报告、JSONL 逐事件、HTML 离线不引用 CDN、CSV 防公式注入；HTML/CSV 将非标量证据转义且保留理由，不自行按绿/红筛选。结构化 stdout 无 ANSI 或进度输出 |
| `--trace / --request-log / --redact / --show-ip` | 前两者只输出已批准的阶段名、脱敏路径、计数/时长到 stderr；默认已脱敏。`--redact` 锁定遮蔽；与显式 `--show-ip` 互斥。后者仅本机当次人/机器报告展示**已有且允许展示**的地址，不扩大采集、不显示认证头/账户/整行请求，不进入导出 fixture 或 agent 默认通道 |
| `--state-dir DIR` | 仅显式本地目录同意写入去标识缺失计数；省略时完全不读写此状态、N=1 不报 stale。不是报告目录/Guard 授权；`--fixture/--selftest` 拒绝真实状态目录，只由离线套件注入虚拟状态目录。见 [05](05-security-boundary.md) |
| `--fixture CASE` | 只允许已登记的离线案例 ID，无任意脚本/远程 URL/原始 HAR；与 `--proxy`、`--interface` 互斥。在合成路径上演练核心逻辑，不计真实网络流量 |

**聚合顺序（ADR-003 的裁决，不按数值取最大值）**：①无效用法/配置或内部运行错误 → **1**，保留已存在的部分报告。②有 RESTRICTED、BLOCKED 或 LOCAL_PATH_ERROR → **2**。③有 UNKNOWN、CONFLICT、SIGNAL_STALE、诊断 INCONCLUSIVE、所求能力 UNSUPPORTED，或选中但未能完成/准入、用户取消、全局超时等 → **3**。④否则全部可接受 → **0**。`REACHABLE_ONLY` 对仅要求连通的表面可接受；若要求地区而无证据则 3。未选目录项显示在未覆盖栏但本身不触发 3。预期不涉及某项能力时不因“未请求”判失败。`selftest` 所有预期（包括故意产生的 2/3）断言满足为 0，断言失败为 1；agent 会话不因某次 check 返回 2/3 而结束。

| 后续必须固化的离线用例 | 进程/运行码 | 报告不能丢掉的事实 |
|---|---:|---|
| 连通性预期：OK + REACHABLE_ONLY；其它均未选 | 0 | 未选不是已通过；地域仍可为空 |
| 未知 profile / 非法 proxy；或运行内部故障 | 1 | 第一种零网络；内部错误区分于站点异常 |
| **BLOCKED + UNKNOWN + CONFLICT**，或 LOCAL_PATH_ERROR 后用户取消 | **2** | 同时保留不确定行、取消/未启动原因；绝不能按 max(2,3) 得 3 |
| 都没发请求：空许可、取消、全局截止，或只有 SIGNAL_STALE/UNSUPPORTED | 3 | 终止原因、覆盖及实际请求计数；不是全绿/内部错误 |
| `--expect service_country=US`，只得到新闻版次；另一目标明确受限 | 2 | 比较为 NOT_OBSERVABLE，只有明确受限项触发 2 |

例如 `parallaxe check --headless --profile google-all --json` 在本版资产上应报告选中 107、发包 0、退出 3；`parallaxe targets list --group google.ai --json` 是只读目录操作，成功应为 0。这是阶段 2 验收预期，不是本轮运行输出。
