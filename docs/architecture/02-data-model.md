# 02 核心值、事件与命令契约 v1

阶段 1 · 2026-11-21。[ADR-003 裁决](../../docs/decisions.md)优先。下文 `T?` 表示值可为 JSON null，`T[]` 表示数组（可空），`u64` 非负整数。时间为 UTC RFC 3339；持续时间取单调时钟的毫秒差。ID 是不含用户数据的不透明字符串。缺失值留空并给机器可读 `reason`；字段未观察不等于 false。以下为传输契约，不是运行日志。

## 公共对象

| 类型 | v1 字段与约束 |
|---|---|
| `Target` | `id, family, group, matrix_group, url, surface: string`；`signals: RuleId[]`，`confidence: Confidence`，`confidence_basis, failure, verification: string`；`protected, allow_online, safe_default: bool`。合并目录默认值得 `method, precondition, verified_at, request_budget`；配置词典见 [03](03-schemas.md)。运行时计算 `admission_status/reason`，不反写目录 |
| `Evidence` | `id, target_id, sample_id, rule_id: string; rule_version: u64; observed_at: time; layer: Layer或TRANSPORT; claim: string; value: string或u64或bool或null; source_type: Source; source_ref: string; confidence: Confidence; path_id, origin: string; freshness: Freshness; reason: string?; fixture: bool`。同一次响应共享 sample_id；source_ref 仅目录/文档/fixture 索引。value 只能是 reader 白名单字段，不含原始报文 |
| `Path`（G5） | `path_id, requested_family, proxy_mode: string; interface_ref, proxy_ref, observed_interface, observed_family, resolver_path, connected_peer_ref, http_egress_ref: string?; bind_status: NOT_REQUESTED或VERIFIED或UNSUPPORTED或UNKNOWN; via_tun: bool?; reason: string?`。请求族取 AUTO/V4/V6；实见族取 V4/V6 或 null。标识用本地非敏感别名；连接到代理不等于证明代理远端族/出口。不能验证绑定时绝不静默直连 |
| `Result` | `target: TargetId; state: State; reason: string?; confidence: Confidence; source: Source?; path: Path; duration_ms, request_count: u64; evidence: Evidence[]; region_layers: LayerValue[]; observed: EvidenceId[]; conflicts: Conflict[]; agreement: Agreement[]; expectation?: {key,expected,actual:string?,status}; origin?: string; consent_version?: string; imported_at?: time; rule_digest?: string; eligibility_notes?: EligibilityNote[]; transport, service_observed, risk_signal, local_policy: object`。所有引用都在本目标内解析；`elapsed_ms` 如兼容旧输入仅同值，默认输出 `duration_ms`；用户导入的目标或规则产生结果时显式输出 `origin="USER_IMPORTED"`、`consent_version`、`imported_at`、`rule_digest` |
| `Coverage` | `catalog_size, selected_count: u64; covered: [{target,request_count,http_received:bool}]; uncovered: [{target,selected:bool,reason}]`。目录快照内每个 ID 恰出现一次。首个传输尝试即算覆盖（TLS 失败也算）；仅排队、策略拒绝、未选均未覆盖。fixture 用同一口径描述**模拟**尝试并标记模式；已启动后取消所见事实仍保留 |
| `GuardPlan` | `plan_id, run_id: string; selected_by: USER; operation: PREVIEW; candidates,affected_domains,affected_services,affected_shared_paths: string[]; impact_closure: [{node,via:node?,role,kind,source_ref}]; reasons: string[]; primary_reason: string?; safe_to_apply,closure_complete: bool; applied: false; rollback,limitations: string[]; effect_scope: LOCAL_TRAFFIC_ONLY; remote_account_impact: NOT_PROVABLE; shared_egress_guarantee: UNPROVEN`。纸面准入与 P0 可执行权限不同；见 [06](06-dependencies.md) |
| `Report` | `{schema:"parallaxe.report/v1",tool:"parallaxe-box",version:string,generated_at:time,run_id:string,mode:ONLINE或FIXTURE,results:Result[],coverage:Coverage,plans:GuardPlan[],wall_duration_ms:u64,network_request_count:u64,simulated_request_count:u64,termination:string,exit_code:0..3}`。目标按目录顺序稳定输出，事件则按提交顺序。未覆盖项没发请求，不伪造 Result；致命错误的部分报告须显式标失败 |

`Confidence = HIGH / MEDIUM / LOW / NONE`，针对某一 claim；`Source = OFFICIAL_DOC / DIRECT_ANONYMOUS_OBSERVATION / COMMUNITY_REPORT / LEGACY_CODE / INFERENCE`，是来源而非等级换算。合成 fixture 保留样本模拟的来源类别，但必须标 `fixture=true` 和 `mode=FIXTURE`，不能作为真实观测引用。`LayerValue={layer,value:string?,reason:string?}`；七层固定列出 `EGRESS_NETWORK / SERVICE_POLICY / ACCOUNT / PAYMENTS / PREFERENCE / DEVICE / CLOUD`。私人账号、付款、保存偏好与实际项目配置不采，值 null、原因 `NOT_OBSERVABLE_ANONYMOUSLY`；设备为 `NOT_COLLECTED`。公开版次在 PREFERENCE 标 `PUBLIC_EDITION`，不是该用户的偏好。

| 四层结果 | Result 中的最小信息 |
|---|---|
| `transport` | `attempts:[{family,phase,connect_ms:u64?,first_byte_ms:u64?,http_status:u64?,fallback:bool,request_sent:bool}]`、`path_diagnostic:string?`；v6 慢/失败、仅 v6 出口、解析器和独立 QUIC 诊断不折算成地区 |
| `service_observed` | `surface:string, observed_country:string?, evidence_ids:string[], reason:string?`；服务字段缺失不能拿 CF、语言或 CDN 机房补值 |
| `risk_signal` | `markers:string[], evidence_ids:string[], reason:string?`；429/挑战/未说明的 403 只记迹象，不能当作封国或账号状态 |
| `local_policy` | `protected,guard_eligible:bool; plan_id:string?; reason:string?`；表达本机保护/预览，不代表远端判定变化 |

`State` 仅 `OK / RESTRICTED / BLOCKED / REACHABLE_ONLY / LOCAL_PATH_ERROR / CONFLICT / UNKNOWN / SIGNAL_STALE / UNSUPPORTED`。OK 需要证据满足所测主张；普通页可达取 REACHABLE_ONLY；明确服务地区限制/策略拒绝分别用 RESTRICTED/BLOCKED；能归因的本机路径错误才取 LOCAL_PATH_ERROR；不支持该能力用 UNSUPPORTED。挑战、`INCONCLUSIVE`、无字段是 reason，不扩枚举。

`Freshness={status:CURRENT或MISSING或STALE或HISTORICAL或NOT_TRACKED,last_hit_at:time?,comparable_misses:u64}` 由 core 产出；超过 7 天只作历史，不等于连续三次缺失。字段缺失时可产生 value=null 的诊断 Evidence。

`Conflict={claim,evidence_ids[]}`；`Agreement={bucket,votes:[{value,sample_ids[]}],winner:string?,tie:bool}`。按服务/表面/量纲/路径/地址族分桶；只比较可归因的同类字段，同一响应、复制的同源值不重复投票。不同地区可并列，不强算全 Google 国家；同桶冲突保留全部证据、状态 CONFLICT，平票无赢家；不可观测项不入分母。

**连续缺失 N=3**：`verdict` 以目录 ID、规范 URL、surface、规则 ID/版本、匿名形态、有效路径与地址族作键；先有同键合格命中，才对三个不同运行的可比完整页面计数。重试、挑战、截断、缓存重放、未知路径不递增；新键不继承。前两次 UNKNOWN，第三次 SIGNAL_STALE；合格命中清零，隔离后的规则须人工复核才可再准入。[ADR-004](../../docs/decisions.md) 裁定：只有启动方**显式指定 `--state-dir`** 才启用跨运行 N=3。默认关闭时每次独立计（N=1），即使是同一常驻 agent 也**只报 UNKNOWN + FIELD_MISSING，不报 SIGNAL_STALE**。显式目录的格式、七天过期与隔离见 [05](05-security-boundary.md)；fixture 仅用隔离的虚拟状态目录，不更新线上历史。

## 事件：一个运行只有一个终结点

事件信封 `{schema:"parallaxe.event/v1",run_id:string,seq:u64,ts:time,is_fixture:bool,event:EventName,payload:object}`。每个 run 从 1 起无间隙递增；协调器提交后再发布，不能按时间或目标 ID 重新编号。同运行可交错多个目标，不同运行序号互不比较。订阅方按 `(run_id,seq)` 去重，缺口先请求重放/快照，不自行推算遗漏的终态。

| 事件 | 必需 payload 与时序 |
|---|---|
| RunStarted | `mode,profile,catalog_revision,selected_ids[]`；运行首事件 |
| PreflightReady | `admitted_ids[],skipped:[{target,reason}],budget,path:Path`；无准入也必须报预检 |
| TargetStarted | `target,path_id,logical_attempt:u64`；仅在实际/模拟尝试时产生 |
| EvidenceObserved | `target,evidence:Evidence`；属已启动未终结目标，可零至多条 |
| TargetFinished | `result:Result`；每个已启动目标恰一次；中途取消无结论则 UNKNOWN + CANCELLED |
| CoverageUpdated | `coverage:Coverage`；是快照而非增量；最后快照与报告完全一致 |
| GuardPlanReady | `plan:GuardPlan`；只因显式预览请求产生，非落地通知 |
| Error | `scope:global或target或request,code,message_safe,recoverable:bool,target:string?`；运行内的错误。解析命令失败发生在 run 外，用命令错误回包，不制造无 run_id 的事件 |
| RunFinished | `report:Report`；含成功、全局预算耗尽或失败的 termination；内部致命错误先发 Error 再终结 |
| RunCancelled | `report:Report`；用户取消，保留覆盖和已获证据；与 RunFinished 互斥 |

终止前停止派发、结清已启动目标和最后覆盖快照；不再接纳迟到证据。已完成运行不可追加事件。事后 `guard_plan` 创建新预览运行，`GuardPlan.source_run_id`（可选 string）指向已有报告，`run_id` 总指产生计划的运行。预览/离线自检等非联网操作按操作成功与否归约，不因没有页面探测本身报错。

## agent：九种请求，不是远程 shell

一行一个 UTF-8 JSON 对象。请求 `{schema:"parallaxe.agent/v1",id:string,op:string,args:object}`；直接响应 `{id,ok:true,data:object}` 或 `{id,ok:false,error:{code,message_safe}}`；异步通知 `{id,record:<完整 event 信封>}`，id 是**启动该运行**的请求 ID，并非最近收到的命令 ID。先确认运行 ID，再输出其事件；stdout 不掺日志。重复/缺失 id、未知键/动作或含密输入拒绝；无法解析 id 的坏行用 `id:null`，会话可继续。

| op | args → data（字段类型） |
|---|---|
| list_targets | `group?:string` → `targets:[{id,group,protected,admission_status,reason}]` |
| load_profile | `profile:string` → `profile,selected_ids[],budget`；只读命名快照，不触网 |
| check | `profile?:string,group?:string,target?:string,fixture?:string,options?:CheckOptions` → `run_id`；options 是 [07](07-cli.md) 的有界参数，不收额外头或任意 URL |
| explain | `run_id,target:string` → `result:Result?` 及未覆盖原因；不补探 |
| selftest | `suite?:string` → `run_id`，只允许内置离线套件 |
| guard_plan | `source_run_id:string,targets:string[]` → `run_id`；仅 dry-run，显式请求代表选择，不解锁系统写入 |
| export_report | `run_id,format:json或html或csv` → `format,content:string`；返回已脱敏内容，不接受任意磁盘路径 |
| cancel | `run_id:string` → `status:REQUESTED或ALREADY_TERMINAL`；未知 ID 返回错误 |
| isolate | `action:plan或explain,source_run_id:string,protect:string[],defer:string[]` → `status,paths[],limitations[]`；只解释已存在的路径，不创建、切换或验证新代理 |

归约只在 core：**1** 用法/配置/内部运行错误；合法结果先判 **2** 明确异常，再判 **3** 不确定，否则 **0** 可接受。取消、空准入和无覆盖不会冒充通过。详细边界/混合结果用例见 [07](07-cli.md)。GUI、CLI、agent 只能投影同一核心报告和事件序列。

**agent 会话边界**：每行请求必须声明 v1；初次 `list_targets` 的 data 另带 `protocol_version:2, operations:string[], max_active_runs:1` 作为无副作用能力握手（v2 在 v1 的 9 个只读操作上追加 4 个 `ai_*` 授权桥操作）。长驻会话一次最多一个活动运行，第二个启动请求返回 `BUSY`，但 `cancel`、目录和已完成运行的只读请求仍可服务；不同进程按独立会话处理。错误码包括 `INVALID_ARGUMENT / UNSUPPORTED_SCHEMA / BUSY / UNKNOWN_RUN / POLICY_DENIED / BRIDGE_UNAVAILABLE / AUTONOMOUS_DISABLED / INTERNAL`；断开 stdin 取消活动运行，不启动后台服务。请求日志仅记合成本地 attempt_id、target、路径别名、去重键摘要、phase、status、duration_ms、fallback/retry、request_sent，不写完整头/URL 查询。


## 可提取的报告 JSON Schema（v1）

为保持阶段 1 只交付 Markdown/TOML，下方唯一 JSON 代码块即机器可读 schema，不另建 JSON 文件。只覆盖规范化**输出**（旧 `elapsed_ms` 在输入边界处理）；ID 引用、七层逐项唯一、覆盖集合互斥、事件顺序与退出码优先级是上面的语义约束，不由结构验证替代。终结事件的 `payload.report.exit_code` 即 GUI 所需的终态严重度；不另存第二个 severity 字段。

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {"schema":{"const":"parallaxe.report/v1"},"tool":{"const":"parallaxe-box"},"version":{"type":"string"},"generated_at":{"$ref":"#/$defs/time"},"run_id":{"type":"string"},"mode":{"enum":["ONLINE","FIXTURE"]},"results":{"type":"array","items":{"$ref":"#/$defs/result"}},"coverage":{"$ref":"#/$defs/coverage"},"plans":{"type":"array","items":{"$ref":"#/$defs/plan"}},"wall_duration_ms":{"$ref":"#/$defs/u64"},"network_request_count":{"$ref":"#/$defs/u64"},"simulated_request_count":{"$ref":"#/$defs/u64"},"termination":{"enum":["COMPLETE","CANCELLED","GLOBAL_TIMEOUT","FAILED","PARTIAL"]},"exit_code":{"enum":[0,1,2,3]}},
  "required": ["schema","tool","version","generated_at","run_id","mode","results","coverage","plans","wall_duration_ms","network_request_count","simulated_request_count","termination","exit_code"],
  "additionalProperties": false,
  "$defs": {
    "u64": {"type":"integer","minimum":0,"maximum":18446744073709551615},
    "time": {"type":"string","format":"date-time"},
    "confidence": {"enum":["HIGH","MEDIUM","LOW","NONE"]},
    "source": {"enum":["OFFICIAL_DOC","DIRECT_ANONYMOUS_OBSERVATION","COMMUNITY_REPORT","LEGACY_CODE","INFERENCE"]},
    "state": {"enum":["OK","RESTRICTED","BLOCKED","REACHABLE_ONLY","LOCAL_PATH_ERROR","CONFLICT","UNKNOWN","SIGNAL_STALE","UNSUPPORTED"]},
    "layer": {"enum":["EGRESS_NETWORK","SERVICE_POLICY","ACCOUNT","PAYMENTS","PREFERENCE","DEVICE","CLOUD"]},
    "path": {"type":"object","properties":{"path_id":{"type":"string"},"requested_family":{"enum":["AUTO","V4","V6"]},"proxy_mode":{"type":"string"},"interface_ref":{"anyOf":[{"type":"string"},{"type":"null"}]},"proxy_ref":{"anyOf":[{"type":"string"},{"type":"null"}]},"observed_interface":{"anyOf":[{"type":"string"},{"type":"null"}]},"observed_family":{"anyOf":[{"enum":["V4","V6"]},{"type":"null"}]},"resolver_path":{"anyOf":[{"type":"string"},{"type":"null"}]},"connected_peer_ref":{"anyOf":[{"type":"string"},{"type":"null"}]},"http_egress_ref":{"anyOf":[{"type":"string"},{"type":"null"}]},"bind_status":{"enum":["NOT_REQUESTED","VERIFIED","UNSUPPORTED","UNKNOWN"]},"via_tun":{"anyOf":[{"type":"boolean"},{"type":"null"}]},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["path_id","requested_family","proxy_mode","interface_ref","proxy_ref","observed_interface","observed_family","resolver_path","connected_peer_ref","http_egress_ref","bind_status","via_tun","reason"],"additionalProperties":false},
    "evidence": {"type":"object","properties":{"id":{"type":"string"},"target_id":{"type":"string"},"sample_id":{"type":"string"},"rule_id":{"type":"string"},"rule_version":{"$ref":"#/$defs/u64"},"observed_at":{"$ref":"#/$defs/time"},"layer":{"anyOf":[{"$ref":"#/$defs/layer"},{"const":"TRANSPORT"}]},"claim":{"type":"string"},"value":{"anyOf":[{"type":"string"},{"$ref":"#/$defs/u64"},{"type":"boolean"},{"type":"null"}]},"source_type":{"$ref":"#/$defs/source"},"source_ref":{"type":"string"},"confidence":{"$ref":"#/$defs/confidence"},"path_id":{"type":"string"},"origin":{"type":"string"},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]},"fixture":{"type":"boolean"},"freshness":{"$ref":"#/$defs/freshness"}},"required":["id","target_id","sample_id","rule_id","rule_version","observed_at","layer","claim","value","source_type","source_ref","confidence","path_id","origin","reason","fixture","freshness"],"additionalProperties":false},
    "attempt": {"type":"object","properties":{"family":{"enum":["AUTO","V4","V6","UNKNOWN"]},"phase":{"type":"string"},"connect_ms":{"anyOf":[{"$ref":"#/$defs/u64"},{"type":"null"}]},"first_byte_ms":{"anyOf":[{"$ref":"#/$defs/u64"},{"type":"null"}]},"http_status":{"anyOf":[{"type":"integer","minimum":100,"maximum":599},{"type":"null"}]},"fallback":{"type":"boolean"},"request_sent":{"type":"boolean"}},"required":["family","phase","connect_ms","first_byte_ms","http_status","fallback","request_sent"],"additionalProperties":false},
    "destination": {"type":"object","properties":{"peer_role":{"enum":["DIRECT_SOCKET","PROXY_NODE","NOT_OBSERVED"]},"resolved_count":{"type":"integer","minimum":0},"resolved_v6":{"type":"integer","minimum":0},"peer_observed":{"type":"boolean"},"resolved_addresses":{"type":"array","items":{"type":"string"}},"peer_address":{"type":["string","null"]},"address_consistency":{"enum":["MATCH","MISMATCH","PROXY_NOT_COMPARABLE","NOT_OBSERVED"]},"resolved_countries":{"type":"array","items":{"type":"string"}},"peer_country":{"type":["string","null"]},"peer_asn":{"type":["integer","null"]},"expected_country":{"type":["string","null"]},"region_consistency":{"enum":["MATCH","MISMATCH","PROXY_NOT_COMPARABLE","NOT_OBSERVED"]}},"required":["peer_role","resolved_count","resolved_v6","peer_observed","resolved_addresses","peer_address","address_consistency","resolved_countries","peer_country","peer_asn","expected_country","region_consistency"],"additionalProperties":false},
    "result": {"type":"object","properties":{"destination":{"$ref":"#/$defs/destination"},"target":{"type":"string"},"state":{"$ref":"#/$defs/state"},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]},"confidence":{"$ref":"#/$defs/confidence"},"source":{"anyOf":[{"$ref":"#/$defs/source"},{"type":"null"}]},"path":{"$ref":"#/$defs/path"},"duration_ms":{"$ref":"#/$defs/u64"},"request_count":{"$ref":"#/$defs/u64"},"evidence":{"type":"array","items":{"$ref":"#/$defs/evidence"}},"region_layers":{"type":"array","items":{"type":"object","properties":{"layer":{"$ref":"#/$defs/layer"},"value":{"anyOf":[{"type":"string"},{"type":"null"}]},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["layer","value","reason"],"additionalProperties":false},"minItems":7,"maxItems":7},"observed":{"type":"array","items":{"type":"string"}},"conflicts":{"type":"array","items":{"type":"object","properties":{"claim":{"type":"string"},"evidence_ids":{"type":"array","items":{"type":"string"}}},"required":["claim","evidence_ids"],"additionalProperties":false}},"agreement":{"type":"array","items":{"type":"object","properties":{"bucket":{"type":"string"},"votes":{"type":"array","items":{"type":"object","properties":{"value":{"type":"string"},"sample_ids":{"type":"array","items":{"type":"string"}}},"required":["value","sample_ids"],"additionalProperties":false}},"winner":{"anyOf":[{"type":"string"},{"type":"null"}]},"tie":{"type":"boolean"}},"required":["bucket","votes","winner","tie"],"additionalProperties":false}},"transport":{"type":"object","properties":{"attempts":{"type":"array","items":{"$ref":"#/$defs/attempt"}},"path_diagnostic":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["attempts","path_diagnostic"],"additionalProperties":false},"service_observed":{"type":"object","properties":{"surface":{"type":"string"},"observed_country":{"anyOf":[{"type":"string"},{"type":"null"}]},"evidence_ids":{"type":"array","items":{"type":"string"}},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["surface","observed_country","evidence_ids","reason"],"additionalProperties":false},"risk_signal":{"type":"object","properties":{"markers":{"type":"array","items":{"type":"string"}},"evidence_ids":{"type":"array","items":{"type":"string"}},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["markers","evidence_ids","reason"],"additionalProperties":false},"local_policy":{"type":"object","properties":{"protected":{"type":"boolean"},"guard_eligible":{"type":"boolean"},"plan_id":{"anyOf":[{"type":"string"},{"type":"null"}]},"reason":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["protected","guard_eligible","plan_id","reason"],"additionalProperties":false},"expectation":{"type":"object","properties":{"key":{"type":"string"},"expected":{"type":"string"},"actual":{"anyOf":[{"type":"string"},{"type":"null"}]},"status":{"enum":["MATCH","MISMATCH","NOT_OBSERVABLE"]}},"required":["key","expected","actual","status"],"additionalProperties":false},"origin":{"type":"string"},"consent_version":{"type":"string"},"imported_at":{"$ref":"#/$defs/time"},"rule_digest":{"type":"string"},"eligibility_notes":{"type":"array","items":{"type":"object","properties":{"category":{"const":"ENTITLEMENT_GATE"},"summary_key":{"type":"string"},"source_url":{"type":"string"},"verified":{"const":false}},"required":["category","summary_key","source_url","verified"],"additionalProperties":false}}},"required":["target","state","reason","confidence","source","path","duration_ms","request_count","evidence","region_layers","observed","conflicts","agreement","transport","service_observed","risk_signal","local_policy"],"additionalProperties":false},
    "coverage": {"type":"object","properties":{"catalog_size":{"$ref":"#/$defs/u64"},"selected_count":{"$ref":"#/$defs/u64"},"covered":{"type":"array","items":{"type":"object","properties":{"target":{"type":"string"},"request_count":{"$ref":"#/$defs/u64"},"http_received":{"type":"boolean"}},"required":["target","request_count","http_received"],"additionalProperties":false}},"uncovered":{"type":"array","items":{"type":"object","properties":{"target":{"type":"string"},"selected":{"type":"boolean"},"reason":{"type":"string"}},"required":["target","selected","reason"],"additionalProperties":false}}},"required":["catalog_size","selected_count","covered","uncovered"],"additionalProperties":false},
    "plan": {"type":"object","properties":{"plan_id":{"type":"string"},"run_id":{"type":"string"},"selected_by":{"const":"USER"},"operation":{"const":"PREVIEW"},"candidates":{"type":"array","items":{"type":"string"}},"affected_domains":{"type":"array","items":{"type":"string"}},"affected_services":{"type":"array","items":{"type":"string"}},"affected_shared_paths":{"type":"array","items":{"type":"string"}},"impact_closure":{"type":"array","items":{"type":"object","properties":{"node":{"type":"string"},"via":{"anyOf":[{"type":"string"},{"type":"null"}]},"kind":{"type":"string"},"source_ref":{"type":"string"},"role":{"enum":["AFFECTED","ASSOCIATED"]}},"required":["node","via","kind","source_ref","role"],"additionalProperties":false}},"reasons":{"type":"array","items":{"type":"string"}},"primary_reason":{"anyOf":[{"type":"string"},{"type":"null"}]},"safe_to_apply":{"type":"boolean"},"closure_complete":{"type":"boolean"},"applied":{"const":false},"rollback":{"type":"array","items":{"type":"string"}},"limitations":{"type":"array","items":{"type":"string"}},"effect_scope":{"const":"LOCAL_TRAFFIC_ONLY"},"remote_account_impact":{"const":"NOT_PROVABLE"},"shared_egress_guarantee":{"const":"UNPROVEN"},"source_run_id":{"type":"string"}},"required":["plan_id","run_id","selected_by","operation","candidates","affected_domains","affected_services","affected_shared_paths","impact_closure","reasons","primary_reason","safe_to_apply","closure_complete","applied","rollback","limitations","effect_scope","remote_account_impact","shared_egress_guarantee"],"additionalProperties":false},
    "freshness": {"type":"object","properties":{"status":{"enum":["CURRENT","MISSING","STALE","HISTORICAL","NOT_TRACKED"]},"last_hit_at":{"anyOf":[{"$ref":"#/$defs/time"},{"type":"null"}]},"comparable_misses":{"$ref":"#/$defs/u64"}},"required":["status","last_hit_at","comparable_misses"],"additionalProperties":false}
  }
}
```
