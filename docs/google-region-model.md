# Google 七层地区归因模型（Google Region Model）

> 本篇回答 Parallaxe box 的第一性问题：**「同一个出口请求，不同 Google 服务看到的究竟是谁的地区？」**  
> 对应源码：`crates/core/src/contracts.rs`（`Layer::ORDER`）；研究依据：[docs/research/02-region-model.md](research/02-region-model.md)。

---

## 1. 核心原则：先问「是谁的地区」，再看数值

同一个匿名请求可以同时产生多个彼此不同且各自合理的「地区值」：
- Cloudflare `trace` 看到的可能是接收端 IP 地理库映射（`loc`）与接入机房（`colo`）；
- Google News 跳转中的 `hl / gl / ceid` 是页面版次与语言参数；
- Gemini Web 公开页可能出现未公开的内联地区三字码；
- Google Cloud 控制台或 Vertex AI 文档里的 `region` 指的是云资源部署位置。

**绝不能把这些量纲混成一个「Google 出口国家」。** Parallaxe box 在每一条 `ResultRecord` 的 `region_layers` 数组中，严格按固定顺序输出 **7 个地区归因层**，缺值填 `null` 并写明机器可读 `reason`。

---

## 2. 七层地区归因模型（`Layer::ORDER`）

| 序号 | 枚举值（JSON） | 中文名称 | 定义与含义 | 匿名模式下可观测性 | 常见误用（本软件严格禁止） |
|---:|---|---|---|---|---|
| 1 | `EGRESS_NETWORK` | 出口网络位置 | 当前探测路径上接收方观察到的出口网络位置、地址族、DNS 解析路径与连接边缘 | **部分可观测**（仅限该接收方与该次路径，如 CF trace 的 `loc`） | 用 CDN 机房（`colo`）、RIR 注册国或本机 DNS 出口冒充物理位置或 Google 判定 |
| 2 | `SERVICE_POLICY` | 服务地区策略 | 未登录时的公开条款版本（`signed_out_location_observation`）、产品公开地区提示或页面内联地区字段 | **视表面而定**（许多公开页无信号；有内联字段时仅证明页面存在该值，不证明账号资格） | 把某一个服务的地区结果外推至全部 Google 服务 |
| 3 | `ACCOUNT` | Google 账号关联国家 | Google 为已登录账号关联的国家/地区（新账号关联创建地，长期账号可能参考常用地） | **严禁且不可观测**：恒为 `null`，原因固定 `NOT_OBSERVABLE_ANONYMOUSLY` | 把未登录页面、登录跳转或匿名出口国家写成账号国家 |
| 4 | `PAYMENTS` | Google Play 与付款档案国家 | 付款资料国家与账号 Play 国家，影响商店商品、应用购买、会员订阅与数字内容 | **严禁且不可观测**：恒为 `null`，原因固定 `NOT_OBSERVABLE_ANONYMOUSLY` | 用未登录商店页面的国旗图标、币种或免费应用列表反推付款国家 |
| 5 | `PREFERENCE` | 用户偏好与公开版次参数 | `hl`、`gl`、`ceid`、`uule`、YouTube `GL` 等可由请求参数、默认版次或用户设置决定的展示维度 | **仅公开跳转/页面版次可见**（标为 `PUBLIC_EDITION` / `USER_CONTROLLED_OR_SERVICE_EDITION`）；个人保存偏好不采集 | 把 `gl=US`、`ceid=US:en` 或语言参数当作独立出口 IP 投票 |
| 6 | `DEVICE` | 设备定位 | 浏览器或移动端操作系统授权的 GPS / Wi-Fi / 基站设备位置 | **无头 HTTP 不可得**：恒为 `null`，原因固定 `NOT_COLLECTED`（规格定义 `NOT_AVAILABLE_IN_HEADLESS_MODE`） | 用 IP 地理库替代设备定位 |
| 7 | `CLOUD` | 云资源与服务边缘区域 | Google Cloud、Vertex AI、Cloud Storage、Workspace 数据驻留及模型端点的配置区域 | **公开文档目录可见**；具体项目资源配置、IAM 与账单不可匿名观测 | 把 API 主机、Colab 实例区域或服务健康页地区当成用户出口国家 |

> 注：证据对象 `Evidence.layer` 还允许一个不参与地区投票的伪层 **`TRANSPORT`**，用于承载 HTTP 状态码、MIME 类型、首跳 `Location` 主机名、204 空体字节数等纯传输事实，确保传输信号绝不污染 7 层地区归因。

---

## 3. 归因顺序与非匿名信号污染处理

`parallaxe-core` 按严格顺序执行归因：

1. **匿名边界前置**：每次探测为独立无状态 HTTP GET，显式空 `Cookie` 头，不携带 `Authorization` 或用户偏好参数，不保存响应 `Set-Cookie`。
2. **先传输、后安全停步、再内容提取**：
   - 未收到 HTTP 响应则不执行任何内容归因；
   - 遇到登录跳转（`accounts.google.com` 等）、同意页（`consent.google.com` 等）、验证码（`/sorry/`、`/recaptcha/`）、401/403/429 时立即停步，地区字段保持 `null`；
   - 绝不穿透验证页或补抓页面。
3. **搜索页脚来源标签的严格区分**：
   依据 Google Search 官方帮助文档（`OFFICIAL_DOC`），页脚位置来源分为四类：
   - `From your internet address` → 可归入第 1 层（出口网络来源标签，仅说明来源，本身不是国家码）；
   - `From your device` → 第 6 层（设备）；
   - `Based on your places (Home/Work)` → 第 3 层（保存地址）；
   - `Based on your past activity` → 第 3 层（历史个性化）。  
   在匿名路径下只有第一类属于可接受的 IP 来源标签；若被动看到后三类，标记非匿名污染且绝不用于出口归因。
4. **跨服务差异保持并列，绝不强行归一化**：
   当同一出口在 Google 首页、Gemini Web、Books、News、YouTube、Play 上给出不同观测值或部分缺失时，报告如实记录服务间差异（例如 Books 缺字段绝不借用 Gemini 的值），不强行合并成「单一 Google 国家」。

---

## 4. Google 各服务表面的可观测与不可推出边界

| 服务表面 | 匿名请求主要可观察内容 | 绝对不能直接推出的内容 |
|---|---|---|
| Search、Images、Maps | 跳转主机、页面公开版次参数、是否出现 IP 来源标签 | 账号国家、付款国家、完整搜索排名行为 |
| Google News | 跳转中的 `hl / gl / ceid` 公开版次（同跳三键仅计 1 条版次证据） | 账号关联国家、出口 IP 国家 |
| YouTube / Music / Premium | 公开页 `"GL"` 版次、营销页 HTTP 可达性（受条款准入限制） | 订阅国家、家庭地址、Premium 购买资格、设备定位 |
| Google Play 商店 | 公开入口跳转、`/store/apps` 国旗素材（低可信度） | 未登录时的账号 Play 国家、付款档案国家、应用可购性 |
| Gemini Web / NotebookLM / Google One | 页面可达性、公开地区提示或内联未公开字段（低可信度） | 账号资格、年龄验证、订阅计划与实际模型额度 |
| AI Studio 与 Gemini API | 公开文档可达性、无 Key 调用模型列表的 403 状态与响应类型 | 项目权限、账单状态、API Key 额度与地区资格 |
| Gmail、Drive、Docs、Meet、Calendar | 首跳登录交接（如 302 指向 `accounts.google.com`）与匿名可达性 | 个人账号状态、Workspace 组织策略、数据驻留区域 |
| Cloud Console、Vertex AI、Firebase | 控制台/文档可达性与服务健康入口 | 项目真实资源 region、IAM 权限与计费信息 |
| `gstatic`、`googleusercontent`、`googlevideo` 等 | DNS、TLS 握手、HTTP 状态与响应体长度 | 服务地区、账号国家或产品资格 |

---

## 5. 归因层与呈现层的区别

在单目标结果 `ResultRecord` 中，除了 7 个逻辑归因层（`region_layers`），还提供 **4 个面向用户的呈现视角**（详见 [verdict.md](verdict.md)）：
- `transport`：尝试记录、地址族、首包耗时与路径诊断；
- `service_observed`：该服务表面直接观察到的国家（未观察则为 `null`）；
- `risk_signal`：限流、挑战或未说明拒绝的迹象标记；
- `local_policy`：本机保护状态与 Guard 候选资格。

**归因层 ≠ 呈现层**：例如 Google Drive 收到 `302 → accounts.google.com` 时，`transport` 记录收到响应，`service_observed.observed_country` 为 `null`（原因 `REDIRECT_TO_LOGIN`），`risk_signal` 无风控证据（不等于「无风险」），`local_policy.protected` 为 `true`；四者都不能反推出 `ACCOUNT` 或 `PAYMENTS` 层的缺失值。
