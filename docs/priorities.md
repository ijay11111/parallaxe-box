# 服务优先级配置与用户决策权（Priorities）

> 本篇说明 Parallaxe box 如何尊重用户对不同 Google 服务重要性的自主判断，为什么内置示例绝不冒充用户状态，以及优先级与硬保护下限的关系。  
> 权威格式：`assets/priorities.example.toml`；加载校验：`crates/core/src/targets.rs`；CLI 入口：`parallaxe priorities list`、`parallaxe --config <PATH>`。

---

## 1. 核心原则：检测结果不替用户做决定

不同用户使用 Google 服务的侧重点完全不同：
- 开发者可能把 `google.gemini`、`google.studio`、`google.cloud` 视为最核心服务；
- 跨境办公用户可能最在意 `google.gmail`、`google.drive`、`google.docs`、`google.meet`；
- 普通用户可能最关心 `google.play` 应用下载更新与 `google.youtube`。

因此 Parallaxe box 坚持三条纪律：
1. **绝不预设业务重要性**：工具不会因为自己算出的某种评分，就擅自把某个服务降级为「不重要（OPTIONAL）」或自动关闭。
2. **默认仅报告与给方案**：检测完成后只陈述观测事实、共享路径与潜在影响，始终等待用户显式选择（`selected_by = "USER"`）。
3. **示例不是真实配置**：内嵌的 `assets/priorities.example.toml` 显式标记了 `example_only = true`。默认启动时，程序**不会**把这份示例当成用户的真实优先级，因此 `parallaxe priorities list` 默认输出为空（详见 [ADR-005 D-18](../docs/decisions.md)）。

---

## 2. 配置文件格式与字段语义

如需提供真实的用户优先级配置，可复制 `assets/priorities.example.toml` 为本地文件，并将 **`example_only` 改为 `false`** 后通过 `--config` 传入：

```toml
schema_version = 1
example_only = false

[[services]]
id = "google.gemini"
criticality = "CRITICAL"
protect_from_guard = true
route_policy = "PROTECTED"
selected_by = "USER"

[[services]]
id = "google.books"
criticality = "OPTIONAL"
protect_from_guard = false
route_policy = "CURRENT_PATH"
selected_by = "USER"
```

### 字段字典

| 字段 | 允许取值 | 含义与约束 |
|---|---|---|
| `schema_version` | `1` | 固定 schema 版本 |
| `example_only` | `bool` | 内嵌示例为 `true`；用户通过 `--config` 加载时**必须为 `false`**，否则在任何网络 I/O 之前直接拒绝并返回退出码 `1` |
| `services[].id` | 目录内合法目标 ID | 必须在目标目录中存在 |
| `services[].criticality` | `CRITICAL` / `IMPORTANT` / `OPTIONAL` / `DIAGNOSTIC` | 用户声明的业务重要级别；未列出的服务保持「未指定」，不自动视为 `OPTIONAL` |
| `services[].protect_from_guard` | `bool` | 用户是否要求额外纳入 Guard 保护 |
| `services[].route_policy` | `PROTECTED` / `CURRENT_PATH` / `USER_SELECTED` / `UNPROVEN` | 用户声明的路由策略标签（仅元数据记录，**不会自动切换路由或修改代理**） |
| `services[].selected_by` | `"USER"` | 标识该优先级由用户人工指定（属于操作来源审计字段，不是提权令牌） |

---

## 3. 优先级与系统硬保护下限的关系（「从严原则」）

用户优先级可以**增加保护**，但**永远不能削弱系统硬保护**：

| 场景 | 最终生效规则 |
|---|---|
| 系统硬保护域（如登录链 `accounts.google.com`、付款链、Play 下载域 `dl.google.com`、共享 CDN `gstatic.com`），用户在配置里写 `criticality = "OPTIONAL"` 且 `protect_from_guard = false` | **系统硬保护仍然生效**，Guard 仍拒绝生成阻断规则（`safe_to_apply = false`） |
| 非硬保护目标，用户标为 `criticality = "CRITICAL"` 或 `protect_from_guard = true` | **保护级别提升**，Guard 计算影响闭包时若触及该目标或其依赖，将触发 `BLOCK_DEFERRED_DEPENDENCY_CONFLICT` 否决 |
| 用户通过 `--config` 传入包含 `admission`、`grants`、`allow_online` 或未定义字段的文件 | **整份配置被拒绝**，返回退出码 `1`，零发包 |

---

## 4. 命令行验证

```bash
# 查看当前加载的用户优先级（默认未提供 --config 时为空）
parallaxe priorities list
parallaxe priorities list --json

# 尝试把内置示例文件直接当作 --config 传入（将被安全拦截，退出码 1）
parallaxe --config assets/priorities.example.toml priorities list

# 加载用户自己维护的真实优先级文件（文件内 example_only = false）
parallaxe --config ./my-priorities.toml priorities list
```
