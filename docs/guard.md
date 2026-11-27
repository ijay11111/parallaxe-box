# Guard 安全防护子系统与共享 CDN 边界（Guard）

> 本篇说明 Parallaxe box 的 Guard 只读影响闭包引擎、共享 CDN 不可分离难题、审计与回滚契约，以及 hosts/防火墙/loopback API 写操作评估结论。  
> 源码实现：`crates/core/src/guard.rs`；依赖数据源：`assets/dependencies.toml`；设计归档：[docs/architecture/06-dependencies.md](architecture/06-dependencies.md)、写入生命周期评估（本地内部资料，不随仓库公开）。

---

## 1. Guard 的定位与诚实边界

Guard 的唯一目的是：**在明确观测到某个低优先级目标受限或被阻断时，帮助用户评估「是否可以安全地减少本机继续向该目标发包」，并预先揭示可能误伤哪些核心服务。**

Guard **绝不能**做到以下事情，也绝不作出虚假承诺：
- 不能修改远端平台对共享出口 IP 的历史行为画像；
- 不能恢复账号资格、订阅资格或更改付款国家；
- 不能绕过风控、验证码或地区门槛；
- **测试版不含系统级阻断**。`guard plan` 只读，`applied=false`、`rules_written=0`。规则文件通过 `guard export --out-dir <目录>` 手动导出。闭包完整且只有 `NO_SAFE_BLOCK` 警示时，导出结果保留 `EXPORT_WARNING_NO_SAFE_BLOCK`，用户须自行审阅。

固定对外边界字段：
```text
effect_scope            = LOCAL_TRAFFIC_ONLY
remote_account_impact   = NOT_PROVABLE
shared_egress_guarantee = UNPROVEN
```

---

## 2. 共享 CDN 与「阻断会误伤下载」的核心难题

对 Google 而言，大量域名、Anycast IP 与后端基础设施被多产品复用（参考 Google Android Enterprise 官方网络文档）。因此以下做法被**永久禁止**：
- 按 Google ASN、IP 段、CDN IP 或 Anycast 地址生成阻断规则；
- 对 `gstatic.com`、`googleusercontent.com`、`googleapis.com`、`ggpht.com`、`gvt1/2/3.com`、`dl.google.com`、`dl-ssl.google.com`、`clients1–6.google.com`、`xn--ngstr-lra8j.com` 等共享基础设施生成后缀 REJECT；
- 为拆分同一主机下的 HTTPS 路径而静默安装根证书或实施 TLS 中间人解密；
- 把共享 CDN 域写入全局 hosts 黑名单。

### 内置硬保护种子（`assets/dependencies.toml`）
- **35 个精确保护域**：覆盖登录链（`accounts.google.com`、`accounts.youtube.com`、`myaccount.google.com`、`oauth2.googleapis.com`、`openidconnect.googleapis.com`、`apis.google.com`）、付款链（`pay.google.com`、`payments.google.com`）、证书校验（`crl.pki.goog`、`ocsp.pki.goog`、`pki.google.com`，详见 ADR-002 修正）、下载与更新（`dl.google.com`、`dl-ssl.google.com`、`android.clients.google.com`）、推送与通用前端（`play.google.com`、`www.google.com`、`www.gstatic.com`、`www.recaptcha.net`、`storage.googleapis.com` 等）。
- **10 个完整 DNS 标签后缀**：`ggpht.com`、`googleapis.com`、`googleusercontent.com`、`gstatic.com`、`gvt1.com`、`gvt2.com`、`gvt3.com`、`xn--ngstr-lra8j.com`、`android.com`、`google-analytics.com`。
- **4 组保守保护家族谓词**：`google-account-country`、`mtalk-label`、`mtalk-alt-1-8`、`clients-1-6`。
- **用户导入目标硬保护**：所有通过 GUI 自定义导入的 `user.*` 目标强制 `protected = true`，**绝不允许进入 Guard 候选集**。

---

## 3. 候选准入与影响闭包算法（`guard::plan_preview`）

### 3.1 候选准入（四重门槛缺一不可）

目标进入候选集必须同时满足：
1. 来源运行中该目标终态为 `RESTRICTED` 或 `BLOCKED`（`UNKNOWN`、`CONFLICT`、`REACHABLE_ONLY`、`SIGNAL_STALE`、`LOCAL_PATH_ERROR`、无结果项一律拒绝）；
2. `confidence >= MEDIUM`；
3. `local_policy.protected == false` 且 `local_policy.guard_eligible == true`（同时重新核对目录级保护位，防止伪造来源报告绕过）；
4. 关联证据不含 `FreshnessStatus::Stale`。

### 3.2 有穷队列影响闭包

对候选精确主机集合 `B`，引擎在本地依赖图上（**绝不在线枚举新主机**）计算：

```text
impact_closure(B) =
  B
  ∪ shared_domains(B)
  ∪ reverse_dependencies(B)
  ∪ protected_login_chain(B)
```

- **邻接索引**：由完整 135 项目录的隐式 `svc:<id> → host:<fqdn>` ENTRY 边 + `assets/dependencies.toml` 显式边构成（例如选择 `google.labs` 会自动发现同主机 `labs.google` 上的 `google.flow` 并继续沿关联边检查）。
- **有穷防环与预算**：按 `(node, role)`（`AFFECTED` / `ASSOCIATED`）去重，设硬预算 `MAX_CLOSURE_NODES = 10_000`、`MAX_EDGES_SCANNED = 100_000`，输出稳定排序确保结果确定性。
- **完整性校验（`closure_complete`）**：以下任一情况即置 `closure_complete = false` 并否决：
  - 缺少来源运行结果或候选为空；
  - 来源运行曾被取消、超时或未完整结束（`source_interrupted = true`）；
  - 依赖边 `verified_at` 超过 `DEPENDENCY_MAX_AGE_DAYS = 365` 天或落在基准日期之后（未来日期）；
  - 触发节点/边扫描预算上限。

### 3.3 三个否决原因的固定优先级

| 优先级 | 原因码 | 触发情形 |
|---:|---|---|
| **①** | `GUARD_DEFERRED_SHARED_CDN` | 命中已知共享分发 CDN 种子（如 `dl.google.com`、`gvt1.com`、`gstatic.com`、`googleusercontent.com`） |
| **②** | `BLOCK_DEFERRED_DEPENDENCY_CONFLICT` | 命中同主机多服务复用（如 `www.google.com` 同时承载 Search 与 Flights）、关键依赖（登录/OAuth/付款/Play 下载/更新）或硬保护/用户保护服务 |
| **③** | `NO_SAFE_BLOCK` | 其他安全前提不满足：包括闭包不完整、缺乏域名专属性证明（当前内置目录默认 `default_scope = "UNKNOWN"`）、证据不足等（仅当①/②均不成立时追加或作为主原因） |

只要任一否决原因成立，即设置 **`safe_to_apply = false`**。在当前内置资产下，由于目录不包含域名独占性证明，`safe_to_apply` 恒为 `false`——这是刻意保守的安全边界，而不是程序缺陷。

---

## 4. 只读审计（`GuardAudit`）与命令行使用

每次 `guard plan` 均在内存生成 `GuardAudit`（`schema = "parallaxe.guard-audit/v1"`），记录时间、计划 ID、来源运行 ID、`selected_by = "USER"`、请求/准入/被拒明细、前后范围摘要（`rules_written = 0`）、结果（`PREVIEW_REJECTED`）及回滚状态（`NOTHING_APPLIED`），**不写盘**。

```bash
# 直接预览指定目标（无来源报告时也会完整解释拒绝原因，退出码 3）
parallaxe guard plan --dry-run --target google.notebook --json

# 基于已导出的 check --json 报告计算闭包（只读 ≤8 MiB，不发起新请求）
parallaxe guard plan --dry-run --source-report ./check-report.json --target google.books --json

# 查看回滚状态（当前恒报无可回滚）
parallaxe guard rollback --json
```

---

## 5. hosts、防火墙与 loopback API 写操作评估结论

阶段 4（写入生命周期评估为本地内部资料）对三类高风险系统写入/监听能力进行了独立评估，结论如下：

| 评估对象 | 本版本状态 | 不开放原因与未来门槛 |
|---|---|---|
| **hosts 文件** | **不开放** | 需管理员/root 权限、影响整机全部端口与路径、易误伤共享 CDN、与外部编辑冲突 |
| **Windows 防火墙** | **不开放** | 需提权且规则受网络配置/组策略/EDR 影响，禁止按 IP/ASN 阻断 |
| **loopback API** | **不开放** | 现有 `--stdio` 与 GUI 进程内事件流已满足三端一致，无必要引入本机网络监听攻击面 |

若未来经正式 ADR 裁决考虑开放任一写入能力，必须全部通过 **G0–G9 共 10 条硬门控**（用户显式发起、仅精确 FQDN、四重候选门槛、闭包完整、独立路径证明、预览与写入字节级一致、写前备份与原子替换、精确回滚、完整审计、失败即停不提权）。
