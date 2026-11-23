# 03 四份 TOML 的读取与校验契约

阶段 1 · 2026-11-21。四份文件均使用 [TOML 1.0 语法](https://toml.io/en/v1.0.0)，UTF-8、`schema_version=1`。这是配置数据，不是模板语言：不执行表达式、不展开环境变量、不远程 include。文件解析成功只证明语法；还要通过引用、预算和准入检查。权威字段名以随附 TOML 为准。

## targets：注册与许可分开

[targets.toml](../../assets/targets.toml) 内 `[[targets]]` 是 107 条 Google 注册；`[[controls]]` 是 28 条独立对照。它们使用同一行结构。`[defaults]` 是所有行继承的、不由目标放宽的公共值；`signals` 外键指向独立的 [signal-rules.toml](../../assets/signal-rules.toml)；目标文件不内联第二套定义。

| 字段 | 类型、含义、验证规则 |
|---|---|
| `id / family / group / matrix_group` | 非空字符串。ID 在两个数组全局唯一，不从 URL 临时生成；`group` 如 `google.ai`，选组按全串相等；`matrix_group` 为 A–E 或 CONTROL，保留任务矩阵而非替代用户选组 |
| `url / surface / signals` | 固定公开 HTTPS URL（无 userinfo/fragment；仅接受登记的必要常量查询）/表面说明/非空 reader ID 数组。ID 与 URL 一对一；初始地址由注册行给出，命令不接受替换 URL。`observed_redirect_host?:string` 只读历史提示，不是目标地址 |
| `confidence / confidence_basis / failure` | 针对上次样本**所主张的地区量纲**的 HIGH/MEDIUM/LOW/NONE、源表限定语、失效与退化说明。状态行存在可以高可信而地区为 NONE；不是从请求前预设结论。基于版次的理想上限不覆盖当次未命中 |
| `verification` | HTTP_OBSERVED＝该表面获首跳；FIELD_UNVERIFIED＝有响应但相关候选字段未获可用样本；NO_HTTP＝已尝试未获 HTTP。都不表示地区已证或已获许可 |
| `protected / allow_online / safe_default` | 三个显式布尔值，不在缺失时猜测。第一项是保护下限，后两项分别限制可否联机、可否在隐式默认选择下联机；两者为 true 仍不是许可。`safe_default=true` 但 `allow_online=false` 为非法组合 |
| `defaults` | `method=GET, precondition=ANONYMOUS_ONLY, request_budget=1, max_body_bytes≤16777216, max_redirects=0, stale_misses=3（仅显式 --state-dir 时生效）`。`source_type` 是登记状态的来源，`verified_at` 是 TOML 本地日期（不是授权到期日） |
| 外键 `signals[]` | 每项为 `rule.*` 稳定键；规则有 `version,applies_to,extract,notes,layer,channel,credibility` 等，详见 [09](09-signals-and-fixtures.md)。`layer` 见 02；`credibility` 是规则对其特定主张的上限。作用域为显式引用它的目标/表面，不仅凭相似正文全局匹配。目标 ID + reader ID/版本组成信号 ID；实际缺失/失败只记原因。跨文件引用必须双向一致，禁用的回归负例不能进入目标列表。`ip_present` 为字段存在布尔值，不输出原值 |

v1 提取器仅 `status/header/redirect_host/redirect_param/regex/text_label/body_length/text_kv/json`；`extract.pattern` 由有界语法和字段白名单识别，不能任意读响应头、访问文件或执行正则脚本。旧 `name="region"` 仅登记为禁用的负例，无目标可引用。DNS/TLS/QUIC 为独立链路诊断，不注册成正文 reader。

**许可闸门**：`admission.grants` 是受信任发行快照中的表数组，本版是空数组。新增记录需维护人逐址复核，具备 `target_id, method, exact_url, surface, purpose, policy_ref, reviewed_by`（字符串）、`reviewed_at, valid_until`（日期，含首尾）、`rule_versions`（reader ID→版本）。许可绑定准确路径及查询、用途、版本与目标；过期、未经审查、时钟不可信、外部覆盖改变 URL/表面或规则就拒绝。不得以同域、301 目的地、`--full`、用户签名为 USER 或添加普通 TOML 来替代。**实际联网 = 命中选择 ∧ 精确发行许可有效 ∧ allow_online ∧（隐式默认时还须 safe_default）∧ 请求/路径/预算安全。** 策略拒绝记录在未覆盖，不尝试 DNS 或 HTTP。

## profiles、依赖与优先级

| 文件/结构 | v1 语义 |
|---|---|
| [profiles.toml](../../assets/profiles.toml) `[limits]` | 硬上限只有一个真值：全局 6、每主机 2、每目标 1 次逻辑 GET、最多 1 个额外尝试（v4 回落与应用重试共享）、无自动跟跳、16 MiB、v6 首包回落门槛 8000 ms。文件只能在代码硬限制内生效，不是用户可抬高的开关 |
| `[[profiles]]` | `id:string`，`ids:string[]`、`groups:string[]`、`namespace:google或all` 三者恰一，引用必须存在；`run_timeout_ms,request_timeout_ms,max_concurrency,per_host` 正整数。运行预算≤120000 ms、单请求≤12000 ms，单主机≤实际全局并发；内嵌配置可再收紧。simple 为 15 个候选；google-all 仅 Google；full 为全部；business/study/media 是主题选择，不代用户设重要性 |
| [dependencies.toml](../../assets/dependencies.toml) | `default_scope=UNKNOWN`，`seeds.{source,exact[],suffixes[],protected_families[]}`；`edges[]:{src,dst,kind,source_type,source,verified_at}`。有效节点、派生主机边、保护谓词、方向及冲突顺序见 [06](06-dependencies.md)。`classifications.cdn_exact/cdn_suffixes` 为只读的 CDN 归因子集。`source` 的 `文件#目标ID` 是溯源定位符，不是 HTML 锚点 |
| [priorities.example.toml](../../assets/priorities.example.toml) | `example_only=true`，不加载为用户状态。真实用户主动提供的优先文件须 `example_only=false`。`services[]:{id,criticality,protect_from_guard,route_policy,selected_by}`；等级 CRITICAL/IMPORTANT/OPTIONAL/DIAGNOSTIC，路由 PROTECTED/CURRENT_PATH/USER_SELECTED/UNPROVEN，`selected_by=USER` 记录操作来源，不是权限令牌。未选择保持未指定，不擅自把服务降为 OPTIONAL |

## 嵌入、覆盖与失败行为

1. 未来单文件用 `include_str!` 嵌入四件套及只读的 `signal-rules.toml`；独立运行不需要配置目录。保留嵌入的许可及保护下限为不可变基线。启动时一次加载成不可变快照，运行中不热切换。
2. `--targets-dir`/`--profiles-dir` 只加载显式本地目录内的对应类型 `.toml`，稳定排序；`--config` 仅显式引用上述层与真实优先文件，不扫描 home、浏览器、代理或环境变量。同层重复 ID/键拒绝；已存在目标的覆盖须保持 ID、URL、规则/表面契约，只能禁用、收紧预算或增加保护；改 URL 请用新 ID，**只登记、无许可**。外部层禁止包含 `admission` 或重定义内嵌 reader；跨类型、过宽路径、指向目录外的链接/重解析点拒绝。
3. profile 可以换选集，不能提高内嵌预算；从硬上限、profile、用户收紧值取最小。优先级、域保护以更严者为准，`protect_from_guard=false` 不能取消系统保护，不能把未知依赖提升为专用。数据层不含任意请求头、凭据、订阅或指令字段。
4. ID/URL 重复、reader/组/目标悬空、未知版本/字段、类型错误、非法预算、未经授权的提权字段：整层配置无效，在任何网络 I/O 前返回 1 并显示字段位置（不回显原值）；有效配置只是没有许可，不是配置错误，在线 check 按覆盖/退出码 3 报告。离线重放不需要在线许可，但也不能取得联网能力。
