# 服务依赖图与路径隔离边界（Isolation）

> 本篇说明 Parallaxe box 如何维护有证据支撑的服务依赖图、如何解释不同服务间的关联影响，以及在没有独立出口路径时，工具**能保证什么**与**不能保证什么**。  
> 依赖数据源：`assets/dependencies.toml`；源码实现：`crates/core/src/guard.rs`、`crates/core/src/runtime.rs`、`crates/cli/src/main.rs`。

---

## 1. 依赖图模型（`assets/dependencies.toml`）

依赖图采用**严格命名空间分离**的节点模型，防止把「同主机不同路径」误判成「独立主机」：

| 节点前缀 | 含义 | 示例 |
|---|---|---|
| `svc:<target_id>` | 注册目录中的具体服务表面 | `svc:google.drive`、`svc:google.flights`、`svc:google.notebook` |
| `host:<规范FQDN>` | 规范化小写 ASCII 域名（去尾点） | `host:accounts.google.com`、`host:dl.google.com`、`host:www.google.com` |
| `capability:<名称>` | 平台级关键功能能力 | `capability:play-download`、`capability:android-update` |

### 1.1 两类边的来源与语义

1. **隐式 ENTRY 边（自动派生）**：目录中 135 个注册条目均隐式生成 `svc:<target_id> → host:<fqdn>`。因此 `google.flights`、`google.travel`、`google.finance`、`google.search`、`google.home` 自动汇聚于 `host:www.google.com`，不会彼此孤立。
2. **显式证据边（`[[edges]]`，逐条附来源与核验日期）**：当前 `assets/dependencies.toml` 收录 4 条已核验的显式边（每条均带 `source_type`、`source` 与 `verified_at`）：
   - `svc:google.drive → host:accounts.google.com`（`LOGIN`，直接匿名观测）
   - `capability:play-download → host:dl.google.com`（`DOWNLOAD`，Google Android Enterprise 官方文档）
   - `capability:android-update → host:dl.google.com`（`UPDATE`，Google Android Enterprise 官方文档）
   - `svc:google.notebook → host:notebook.google.com`（`REDIRECT`，直接匿名观测，仅表示交接，不断言服务归属或授权）

### 1.2 「无显式边 ≠ 域名独占」原则

`assets/dependencies.toml` 顶层强制声明 **`default_scope = "UNKNOWN"`**：
- 若某服务在 `dependencies explain` 中返回空显式边，仅代表当前图库尚未登记可复核的显式依赖，**绝不等于证明该域名为单一服务独占**；
- 过期边（超过 `DEPENDENCY_MAX_AGE_DAYS = 365` 天或日期落在未来）会使闭包完整性降为 `closure_complete = false`，直接否决安全阻断；
- 绝不通过在线扫描或盲猜来补全依赖图。

---

## 2. 路径隔离的诚实边界

当用户希望「某个低优先级 Google 服务受限时，不牵连 Gemini、Gmail 或 Play 下载等核心服务」时，必须区分两种截然不同的网络前提：

### 情况 A：单一共享出口（无已验证的独立服务路由）

软件**只能**做到：
- 展示各服务的主机复用、反向依赖与共享 CDN 影响闭包；
- 保护登录、OAuth、付款、Play 下载与共享 CDN 不被误伤；
- 在未来受控规则体系下，最多减少本机对单一非共享低优先级前端继续发包。

软件**绝对不能**保证：
- 远端 Google 账号关联国家不发生变化；
- 远端订阅资格（如 Gemini / Google One / YouTube Premium）不受影响；
- 共享出口 IP 在目标平台已记录的历史行为画像被撤销。

因此报告中固定输出三条不可伪造的声明：
```text
effect_scope            = LOCAL_TRAFFIC_ONLY
remote_account_impact   = NOT_PROVABLE
shared_egress_guarantee = UNPROVEN
```

### 情况 B：用户已自行配置并可证明独立服务路径

只有当用户已经拥有独立的出口链路、代理分流组或应用级独立路由，并能对两条路径提供可复核证据时，在概念上才属于 `ROUTE_ISOLATED`。**Parallaxe box 绝不替用户创建、切换、测速或验证新代理节点。**

---

## 3. CLI 与 Agent 查询命令

```bash
# 查看单个目标的显式依赖边与入口主机
parallaxe dependencies explain --target google.drive
parallaxe dependencies explain --target google.gemini --json

# 解释隔离路径（当前无独立路径，返回 NO_EXISTING_PATH 与明确限制说明）
parallaxe isolate plan --protect google.gemini --defer google.books --json
parallaxe isolate explain --target google.books --json
```

对应 `parallaxe agent --stdio` 中的 `isolate` 操作返回完全一致的语义结构：
```json
{
  "status": "NO_EXISTING_PATH",
  "paths": [],
  "limitations": [
    "只解释已有路径，不创建、切换或验证新代理",
    "独立路径必须由用户自行提供并证明"
  ],
  "action": "explain",
  "selected_by": "USER"
}
```
