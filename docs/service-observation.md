# 服务观测记录与四列只读链路体检（Service Observation）

> 本篇说明 Parallaxe box 如何把「DNS 解析、连接边缘、HTTP 出口、IPv4/IPv6 地址族」拆成四列独立观测，避免把本地解析或 CDN 边缘误当成出口地区，同时诚实披露已实现、未测与禁止能力。  
> 源码实现：`crates/core/src/linkcheck.rs`、`crates/core/src/network.rs`；CLI 命令：`parallaxe link explain`。

---

## 1. 为什么必须拆成四列观测？

网络排障中最大的误区之一，就是把不同网络层的观测混为一谈：
- 本机 DNS 解析器返回的 IP，可能受系统 DNS、代理远端 DNS 或 Anycast 调度影响，**不一定是实际建连的对端**；
- 实际连接到的边缘节点地址（如 Google/Cloudflare Anycast 节点），**不等于目标服务业务层认定的用户出口国家**；
- IPv6 慢或半断导致的 8~11 秒首包等待，**是双栈传输故障，不是地区封禁**。

因此 Parallaxe box 在 `parallaxe.link/v1`（`LinkReport`）与 GUI「处置中心」中，强制把每个目标的链路拆为**四列独立呈现**，未知字段一律留 `null` 或 `NOT_OBSERVED`，绝不互相补填：

| 列 | 结构体字段 | 回答的问题 | 防混淆铁律 |
|---|---|---|---|
| **1. `resolver`** | `status`, `v4`, `v6`, `addresses[]`, `reason` | 本机系统解析器对目标主机返回了多少 v4/v6 记录？ | 默认零发包（`NOT_OBSERVED`）；仅显式 `--resolve` 时对内置目录主机查询一次；默认 `addresses = []` 脱敏，仅 `--show-ip` 才展示本地解析结果 |
| **2. `connection_edge`** | `status`, `observed_family`, `attempts[]`, `peer_address`, `reason` | 实际传输尝试的阶段、首包耗时、HTTP 状态与观察地址族是什么？ | 当前 `reqwest` 不暴露原始对端地址，故 `peer_address` 保持 `null`（宁缺勿伪造）；区分「尝试」与「实际已发 HTTP 请求（`request_sent`）」 |
| **3. `http_egress`** | `status`, `observed_country`, `source`, `layer`, `reason` | 出口网络层（`EGRESS_NETWORK`）是否直接观察到了出口国家？ | **严格量纲隔离**：Gemini 页面内联三字码属于 `SERVICE_POLICY` 层，绝不填入 `http_egress` 列（此时该列保持 `NOT_OBSERVED / country = null`） |
| **4. `address_family`** | `v4`, `v6`, `fallback`, `diagnostic` | IPv4 与 IPv6 各自的尝试状态、是否发生回落及双栈诊断结论是什么？ | 与服务地区结论完全解耦 |

---

## 2. 双栈路径六种状态与区分规则

结合 `crates/core/src/network.rs` 与 `linkcheck.rs`，双栈诊断区分以下情形（绝不把仅 v6 出口下 v4 失败判成节点不可用）：

| 诊断形态 | 含义与判定边界 |
|---|---|
| `v4_only` | 仅观察到 IPv4 可用路径（或用户通过 `--ipv4` 显式禁用 v6） |
| `v6_only`（`IPV6_ONLY_EGRESS`） | 仅 IPv6 出口可达、IPv4 不可用；不因此把节点标为「坏节点」 |
| `dual_stack_consistent` | IPv4 与 IPv6 均可达且已观测出口国家一致 |
| `dual_stack_country_conflict` | IPv4 与 IPv6 出口观测国家不一致，标为高风险差异 |
| `v6_reachable_but_slow`（`IPv6_SLOW`） | IPv6 最终成功但首包显著偏慢（触发或接近 8000 ms 受控回落门槛） |
| `v6_unreachable`（`IPv6_BROKEN`） | IPv6 失败而同条件受控回落 IPv4 成功 |

> 补充说明：若用户通过 `--proxy` 连接且代理未暴露远端地址族，系统记录 `bind_status = UNKNOWN` 与 `observed_family = null`，绝不伪标 v4 或 v6，也绝不为回落而绕过代理直连；若 `--interface` 绑定不可验证，直接输出 `UNSUPPORTED`，绝不静默直连（ADR-005 D-5/D-6）。

---

## 3. 能力声明清单（`LinkCapabilities`）

每一份 `parallaxe link explain --json` 与 GUI 链路面板都会显式附带 7 项能力声明，诚实告知哪些已实现、哪些未测、哪些永久禁止：

| 能力字段 | 当前固定取值 | 说明 |
|---|---|---|
| `route_get` | `LOCAL_INTERFACE_OBSERVATION_ONLY` | 仅通过 `if-addrs` 只读观察本机接口（如是否存在 TUN 类接口 `via_tun`）；**不证明某次请求一定经过该接口**，且运行前后不修改系统路由表 |
| `dns_compare` | `UNSUPPORTED_CAPABILITY` | 当前构建未引入第二套独立解析器，也不调用第三方 DoH，不做伪对比 |
| `dns_leak` | `UNTESTED` | 无受控权威解析器或抓包证据，标为未测/不可定论，不瞎报「无泄露」或「泄露」 |
| `quic` | `UNTESTED` | 未发起专门的 QUIC/UDP 探测；HTTP/1.1 或 HTTP/2 结果绝不冒充 QUIC 结论 |
| `http_egress` | 投影既有报告 | 仅读取 `EGRESS_NETWORK` 层已脱敏观测 |
| `connection_edge` | 投影既有尝试账本 | 沿用 `Attempt` 既有语义，不伪造未暴露的 `connect_ms` 或 `peer_address` |
| `port_scan` | **`PROHIBITED`** | **永久禁止**任何形式的本机或远端端口扫描 |

---

## 4. 默认零发包与显式 `--resolve` 规则

```bash
# 默认零发包：仅列出所选目标的未观察快照（dns_query_count=0, http_request_count=0）
parallaxe link explain --profile simple --json

# 基于离线脱敏案例投影四列视图（严格零发包）
parallaxe link explain --fixture gemini-positive --json

# 显式执行系统 DNS 查询（仅限内置目录主机，按主机去重，失败亦只计 1 次）
parallaxe link explain --target google.home --resolve --json

# 如需在本地终端查看解析得到的地址（示例输出中文档只用 RFC 5737 保留段如 203.0.113.x 示意）
parallaxe link explain --target google.home --resolve --show-ip
```

### 安全限制（已由集成测试固化）
1. `--fixture` 与 `--resolve` 严格互斥（同时传入直接返回退出码 `1`）；GUI 处于离线案例模式时自动禁用 DNS 查询按钮。
2. `probe_resolver` 仅允许解析内置目录中已注册的主机，按主机名去重计数；**拒绝解析用户导入目标**（返回 `USER_TARGET_REQUIRES_SESSION_GRANT`）。
3. 脱敏默认开启：未显式传 `--show-ip` 时，`resolver.addresses` 恒为空数组 `[]`，不泄露任何解析 IP。
