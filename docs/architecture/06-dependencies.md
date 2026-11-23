# 06 从候选域反查会被连带影响的功能

阶段 1 · 2026-11-21。输入是 [目录](../../assets/targets.toml)、[依赖事实与保护种子](../../assets/dependencies.toml)、用户明确的[优先级格式](../../assets/priorities.example.toml)和既有结果。输出是内存 `GuardPlan`，不是路由表。默认行为是不阻断。

## 图的构造与保护下限

- 节点用 `svc:<target_id>`、`host:<规范FQDN>`、`capability:<命名功能>` 分隔命名空间。同主机不同 URL 仍是不同服务节点；主机去尾点、统一小写和 ASCII 国际化标签。每个注册条目隐式生成 `svc → host` 的 ENTRY 边，来源是目录 ID，因此 Flights、Travel、Finance 和 Search 不会被当成四台独享主机。
- 显式边始终 `src 依赖 dst`；当前 `LOGIN/DOWNLOAD/UPDATE/REDIRECT`，可登记 `PAYMENT/OAUTH/SHARED_BACKEND/REQUIRES`。每条保留 `source_type,source,verified_at`。REDIRECT 只表示已观察交接，不断言服务归属或许可；派生同主机/反向索引无需复制边。无来源、来源过期、方向不明或图不全都不足以证明独占。`required_by` 由反向索引生成，不让用户手写第二套事实。
- `exact` 只比规范主机；`suffixes` 比根域或以 `.` 为界的子域。编入的 `protected_families` 仅用于**保护**：`google-account-country` 为 `accounts.google.` 后跟有效 DNS 后缀标签的保守匹配（含复合国家后缀）；`mtalk-label` 为 `google.com` 下首标签以 mtalk 起始；`mtalk-alt-1-8` 为 alt1–alt8-mtalk；`clients-1-6` 为 clients1–clients6。非标准或未识别别名按 UNKNOWN 拒绝，不借宽松字符串匹配生成阻断。登录/付款/证书（**`crl.pki.goog`**）/推送/Play/更新链和矩阵中 `protected=true` 的主机形成硬保护并集。
- `classifications.cdn_exact/cdn_suffixes` 只给种子加“已知共享分发”的解释，不构成允许清单。gvt2/3 的诊断用途依旧受保护，不因为名字像 gvt1 就声称正在下载。默认 `domain_scope=UNKNOWN`；EXCLUSIVE 必须有新证据证明边界完整，不能由“反向索引只查到自己”推出。用户可加保护，不能移除硬保护。优先级只记录真实用户的选择，无选择时不假设哪项不重要。

## 闭包：有穷队列与否决顺序

对已明确选择的候选精确主机集合 B，计算 `B ∪ shared_domains(B) ∪ reverse_dependencies(B) ∪ protected_login_chain(B)`。最后一项只指**与 B 有路径的链**，不是把所有无关种子塞进每个报告。

1. 先检查意图、运行出处和候选资格：须是同路径未过期的直接、高可信 `RESTRICTED/BLOCKED`，`guard_eligible=true`，明确 `selected_by=USER`。现场没有这类证据则只提示；即使有保护冲突，也可以**解释**拒绝理由，不产生可应用片段。
2. 用 B 初始化队列。每个出队节点扫描入边（反向找依赖它的服务或上游功能）；将受影响节点入队。对已到达服务另查其已知的共享域、显式关键链/后端边，纳入保守的关联检查；用 `kind` 区分“会被阻断”与“有关联因而不能保证不误伤”，同期使用的所有网站不能无依据连成一图。多条共享域、别名或重定向可继续扩展，**绝不在线枚举新主机**。
3. 按 `(节点,遍历角色)` 去重（角色为受影响/关联检查）；保存来源边、前驱和关键风险标记。扩展到队列空；有限图至多处理两次每节点，各邻边常数次。若超出预设资源预算、中断、缺关键边或过期，标 `closure_complete=false`（完整性缺证亦为 false），否决，不能把部分闭包当安全。结果按节点稳定排序，避免先扫到哪个域就改变原因。
4. 收集所有适用原因。主原因固定顺序：**① `GUARD_DEFERRED_SHARED_CDN`**：命中已知共享分发种子；**② `BLOCK_DEFERRED_DEPENDENCY_CONFLICT`**：非 CDN 共享主机、CRITICAL/用户保护项、硬保护或登录/OAuth/付款/下载/更新的关键依赖；**③ `NO_SAFE_BLOCK`**：其他不满足，包括 UNKNOWN、专用性/图/路径不明、证据不足、Global 模式不能证明规则生效。并列时显示全部，`primary_reason` 取最前者；若其他门槛全过且仅因①/②否决，不凭空加③。任何一条成立即 `safe_to_apply=false`。
5. 只有所有前提满足、闭包完整、每个候选都有专用性证据且无否决时，才可 `safe_to_apply=true` 表示**理论上可审阅的精确主机方案**。本版没有此类目录证据；P0 即便得到该值也始终 `applied=false`、无写入器。禁止按 ASN、IP、共享 CDN 或 HTTPS 路径生成封锁；严格 Global、未证独立路径都不承诺隔离。`ROUTE_ISOLATED` 仅用于用户已有且能证明的独立路径，非本工具切换的结果。

## 可复算的三个例子

| 选择与事实 | 反查得到的证据链 | 预览结果 |
|---|---|---|
| 选择停止 Play 下载，B 含 `dl.google.com` | 反向边找到 play-download 与 android-update；主机也是已知 CDN 分发种子 | `safe_to_apply=false`，①优先，保留②关键下载/更新原因；用户标 OPTIONAL 也不能覆写 |
| 选择 Flights，B= `www.google.com` | ENTRY 入边返回 `google.flights`、`google.search`、`google.home` 等；Search 与 Flights 路径不同，但域名级动作无法分开 | `safe_to_apply=false`，②；不是 CDN 分类。显示相关路径，不导出阻断片段 |
| 某候选没有硬保护标，但没有独占性证明 | 空/不全入边不能证明不会影响其他服务；同出口也不证明同主机 | `safe_to_apply=false`，③；不开展扫描补图 |

报告固定 `LOCAL_TRAFFIC_ONLY`、`remote_account_impact=NOT_PROVABLE`、`shared_egress_guarantee=UNPROVEN`；说明受影响服务/域/共享路径、限制和回滚（预览未写入，无需回滚）。用户以后显式执行的写入生命周期仍由后续门控负责。
