# 09 信号规则与离线 fixture 契约

阶段 1 · 2026-11-21。按 [ADR-004](../../docs/decisions.md) 将规则从目录拆出：[signal-rules.toml](../../assets/signal-rules.toml) 是唯一机器可读定义，[targets.toml](../../assets/targets.toml) 只引用 `signals=["rule.…"]`。**启用提取器不等于启用目标联网**。本阶段没有创建、录制或运行任何真实 fixture。

## 绑定、提取与失效反例

每个 `[[rules]]` 有唯一 `id`、正整数 `version`、布尔 `enabled`、`applies_to[]:{target_id,host,path,surface}`、`extract:{kind,pattern,capture}`、七层之一的 `layer`、`channel`（SERVICE/TRANSPORT）、`credibility`、`source_grade`、`notes`、`negative_case`。`applies_to` 行内字段是**合取**，多行是可选范围；目录引用与作用域取交集，精确 URL/查询再由注册和准入表约束。变更语义须升版本，令旧许可和计数失效；禁止外部配置添加任意提取器。`channel=TRANSPORT` 时 `layer=EGRESS_NETWORK` 仅标归属，不将 HTTP 状态、MIME、跳转主机写入国家层。CF 的 `loc`、`colo` 也必须产生不同 claim。

`capture="1"` 表示有界正则第一捕获组，`"value"` 表示所列白名单字段的值/存在标识或专用标量；不保留整页匹配。`json` 在 v1 只接受列出的顶层键、`header` 只取允许头；`redirect_param` 的多个版本键仍由同一 sample_id 出证。规则 `credibility` 是合适样本上该主张的上限，而 `source_grade` 说明定义的依据，不保证每个绑定地址都出现过字段。

- 正例：合成 Gemini 完整 HTML 中的 `,2,1,200,"USA"` 由 `rule.inline3` 提取三字母；低可信度，仅说明页面有此值。Books 同一规则但没有字段，不能从 Gemini 借值。YouTube GL 只作版次，不作出口。
- 强制负例：`rule.legacy-region` 登记旧 `name="region"`，`enabled=false`，只供回归索引，不被任何目标引用。即使旧页面包含 `name="region" value="XX"`，正常提取零证据；配置若尝试引用禁用规则直接报错，不靠恰好没匹配实现禁用。

## 看哪些响应可以解析

`[detection]` 保存登录/同意主机、上下文主机、登录/验证码路径前缀、提示语、状态码、允许 MIME、最小长度和 16 MiB 上限。它是**安全停步策略**，不是识别真实用户、站点归属或判国的证据；语言关键词不完备，换语种时宁可 UNKNOWN。逐层处理：

1. 先记传输事实。无 HTTP 不解析；403/401 → `UNKNOWN + DENIED_OR_AUTH`，429 → `UNKNOWN + RATE_LIMITED`，不重试，不能直接标 IP 封禁。区域限制与策略拒绝只有具名、适用且直接的服务证据才能升级。
2. 首跳检查规范化 Location，登录/同意主机、验证路径直接停。`login_context_hosts` 不能仅按主机名命中，必须同时看已知登录路径/跳转；Gmail 同域 301 本身不等于要求登录。静态 recaptcha 资源仅按传输入口记录，不能证明某个用户遭风控。正文提示仅在类型、位置及核心页面特征相符时作停步理由；模糊命中给 UNKNOWN，不下国家结论。
3. 只有规则期望的表面、成功响应、完整、未截断、内容类型适配才提取内容。`min_text_bytes=1` 是排除空体的必要门槛，**不是**“正文≥1 字节即完整”；还须读到受限 EOF、已知长度一致（如有）、预期页面签名，不能是营销/错误壳或挑战。去除 MIME 参数后比较；未明确允许的类型不宽松嗅探。204 空体可用于传输规则，不能计为 HTML 字段缺失。截断、内容协商失败或同意页不递增 stale。
4. 默认关闭状态目录不做跨运行聚合；显式 `--state-dir` 才能按 [05](05-security-boundary.md) 在有前次命中的合格样本间累计 N=3。fixture 模式也不能将这类计数写入线上桶。

## fixture 文件对与导入门槛

约定后续目录 `tests/fixtures/<target_id>/<case>/meta.json` + `response.http`。目标 ID 必须存在；case 为固定的简短名称。只由维护者制作、审阅、收进发行白名单，正式客户端不接受浏览器 HAR/任意文件、没有在线录制按钮。

| 文件 | 必须记录 / 丢弃 |
|---|---|
| `meta.json` | `schema="parallaxe.fixture/v1",target_id,case,rule_versions,recorded_at`（UTC RFC 3339），`source_kind:SYNTHETIC或SANITIZED_EXCERPT,source_ref,mode=FIXTURE,requested_family,observed_family,synthetic_path_id,status?,mime?,body_bytes,complete,truncated`，及 `expected:{state,reason?,evidence_ids[],request_count,network_request_count}`。`body_bytes` 为净化后文件的字节数，原样本截断不得因净化体变短而改为完整 |
| `response.http` | 合成状态行；头白名单仅 Content-Type、经过重建的 Location（目录主机/允许路径和版次键），可选按净化体重算的 Content-Length；空行后最少必要的**合成或脱敏**正文。没有 HTTP 的案例则文件为空、meta.status=null，另列传输阶段。fixture 合成体建议≤64 KiB，模拟超过 16 MiB 用 `truncated=true,complete=false`，不存巨型负载 |

净化步骤是**重建白名单，不是抹黑名单**：从批准的摘录或合成规则重建片段；删除其余头（尤其 Set-Cookie/Cookie/Authorization）、完整查询/回跳参数、活跃脚本、用户标识、地址/坐标、所有真实 IP/令牌/订阅。Location 若指携密 URL，不留原字符串，只保留允许的作用域；无法净化成不歪曲语义的案例则不收录。输入中有未知头、超长正文、疑似凭据、规则版本或来源缺失一律拒绝。只读回放绝不执行 JS/跟跳/网络取证。`recorded_at` 是样本时间，不作为线上新鲜度。

**最小回归集**：Gemini 正例、旧 selector 负例（旧属性仍出现）、Books 同键连续缺失计数的显式开/关及过期、Drive 302 登录、News 版次同源、204 空体、429、截断页面、同层冲突和平票、v6 慢/回落/仅 v6、2+3 混合严重度。虚拟时钟和传输账本应使三端重放逐字段可比；账号/支付/设备值始终不可观测。用例是阶段 2 工作清单，不是本轮产品测试通过声明。
