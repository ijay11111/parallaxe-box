# 信号规则、可信度分级、失效风险与离线 Fixture（Signals & Fixtures）

> 本篇说明 Parallaxe box 如何从脱敏 HTTP 首跳样本中提取最小证据、何时触发安全停步、如何识别规则改版失效（`SIGNAL_STALE`），以及离线回归 Fixture 的净化契约。  
> 权威配置：`assets/signal-rules.toml`；源码实现：`crates/core/src/signals.rs`、`crates/core/src/fixtures.rs`、`crates/core/src/state.rs`。

---

## 1. 信号规则架构：白名单提取，不保存正文

Parallaxe box 将所有信号提取规则集中在独立的 `assets/signal-rules.toml` 中，并与 `assets/targets.toml` 双向校验：
- **精确作用域绑定**：每条规则的 `applies_to[]` 必须同时匹配 `target_id + host + path + surface`（合取），且被目标的 `signals[]` 显式引用。绝不凭相似 HTML 跨服务全局匹配。
- **9 种受限白名单 Reader**：系统只允许 9 种确定性提取器（`status`、`header`、`redirect_host`、`redirect_param`、`regex`、`text_label`、`body_length`、`text_kv`、`json`），不允许任意脚本、任意头抓取或文件访问；正文仅在内存中按有界正则/白名单键解析，**绝不持久化保存原始 HTML 或完整响应头**。
- **启用规则 ≠ 允许联网**：规则表只是「拿到样本后怎么读」，不授予任何在线探测权限。

### 内置 12 条信号规则清单（11 启用 + 1 禁用负例）

| 规则 ID | 版本 | 状态 | Reader `kind` | 归因层 / 通道 | 可信度上限 | 提取对象与边界说明 |
|---|---:|---|---|---|---|---|
| `rule.http` | 1 | 启用 | `status` | `EGRESS_NETWORK` / `TRANSPORT` | `HIGH` | 首跳 HTTP 状态码；**仅证明传输事实，绝不推导地区** |
| `rule.type` | 1 | 启用 | `header` | `EGRESS_NETWORK` / `TRANSPORT` | `HIGH` | `Content-Type`；JSON 403 不等于授权成功 |
| `rule.redirect` | 1 | 启用 | `redirect_host` | `EGRESS_NETWORK` / `TRANSPORT` | `HIGH` | 首跳 `Location` 主机名；不自动跟跳，跳转登录域不代表已登录 |
| `rule.edition` | 1 | 启用 | `redirect_param` | `PREFERENCE` / `SERVICE` | `MEDIUM` | `Location` 中的 `hl / gl / ceid`；同一跳三键同源只算 1 条版次证据，不投出口票 |
| `rule.inline3` | 1 | 启用 | `regex` | `SERVICE_POLICY` / `SERVICE` | `LOW` | 未公开内联三字码 `,2,1,200,"([A-Z]{3})"`（绑定首页/Books/Gemini/Preferences）；单条正则最多取 4 个不同值用于冲突检测；**不跨子域借值、不证明账号资格** |
| `rule.yt-gl` | 1 | 启用 | `regex` | `PREFERENCE` / `SERVICE` | `MEDIUM` | YouTube / Music HTML 内 `"GL":"([A-Z]{2})"`；仅反映公开内容版次，同意页不采样 |
| `rule.flag` | 1 | 启用 | `regex` | `PREFERENCE` / `SERVICE` | `LOW` | Play HTML 中 `regionflags/([a-z]{2})\.png`；仅记录页面素材，**不证明商店国家或付款国** |
| `rule.footer` | 1 | 启用 | `text_label` | `EGRESS_NETWORK` / `SERVICE` | `HIGH` | `From your internet address` 标签；仅识别来源为 IP，不读取地点文本 |
| `rule.empty` | 1 | 启用 | `body_length` | `EGRESS_NETWORK` / `TRANSPORT` | `HIGH` | `generate_204` 空体字节数；仅作链路连通证据 |
| `rule.trace` | 1 | 启用 | `text_kv` | `EGRESS_NETWORK` / `SERVICE` | `HIGH` | CF trace 的 `loc,colo,http,tls,ip_present`；`loc` 与 `colo` 分列，IP 仅记布尔存在不存原值 |
| `rule.meta` | 1 | 启用 | `json` | `EGRESS_NETWORK` / `SERVICE` | `NONE` | CF speed meta 的顶层 `country,colo,asn`；历史观测为 403，拒绝样本绝不填值 |
| `rule.legacy-region` | 1 | **禁用** | `regex` | `SERVICE_POLICY` / `SERVICE` | `NONE` | **强制回归负例**：废弃的 `name="region"`，禁止被任何目标引用；即使旧页面出现也零产出 |

---

## 2. 安全停步策略（`[detection]`）

解析任何正文前，引擎先执行安全停步检查（配置位于 `assets/signal-rules.toml` 的 `[detection]` 段）。**停步特征只用于决定「停止提取」，不是「封国」或「风控定罪」的证据**：

1. **状态码停步**：
   - `401 / 403` → `UNKNOWN + DENIED_OR_AUTH`（原因未明，不重试，不直接判封国）；
   - `429` → `UNKNOWN + RATE_LIMITED`（限流迹象，绝不重试）。
2. **首跳跳转停步**：
   - `Location` 指向登录主机（`accounts.google.com`、`accounts.youtube.com`、`myaccount.google.com`）或登录路径前缀（`/signin/`、`/v3/signin/`、`/ServiceLogin`）→ 停止并记 `REDIRECT_TO_LOGIN`；
   - `Location` 指向同意页主机（`consent.google.com`、`consent.youtube.com`）→ 停止并记同意页停步；
   - `Location` 命中验证码路径（`/sorry/`、`/recaptcha/`）→ 停止并记挑战停步。
3. **正文完整度与内容类型门控**：
   - 仅当 `complete = true`、`truncated = false`（响应未超过 16 MiB 上限）、字节数 ≥ `min_text_bytes` 且 MIME 命中白名单（HTML：`text/html`、`application/xhtml+xml`；JSON：`application/json`）时，才允许正文提取；
   - 正文命中同意页文案（如 `Before you continue to Google`）或挑战文案（`unusual traffic from your computer network`）时立即停步；
   - `204` 空体仅执行传输规则，不视作 HTML 字段缺失。

---

## 3. 四个历史失效案例与反例固化

依据阶段 0 研究（[docs/research/05-signals.md](research/05-signals.md)）与 [ADR-002](../docs/decisions.md)，以下四类常见错误推论已在引擎与测试中永久排除：

1. **旧首页 `name="region" value="XX"` 已失效**：保留为禁用负例 `rule.legacy-region`（测试覆盖 `legacy-region-negative`），空串绝不默认美国。
2. **国家顶级域名（ccTLD）不再投票**：Google 于 2025-04-15 公告逐步将国家域名归并至 `google.com`，实测 `www.google.co.uk` 首跳 301 至 `www.google.com`；绝不用最终域名判国。
3. **不再依赖 `num=100` 搜索分页**：不自动提交搜索查询词，不靠统计结果条数推导地区。
4. **IPv6 慢/超时回落不是地区拒绝**：11 秒首包等待属于双栈路径问题（`IPv6_SLOW` / `IPv6_BROKEN`），与服务地区限制完全分离。

---

<a id="signal-freshness"></a>

## 4. `SIGNAL_STALE` 规则新鲜度机制（N=3 与 `--state-dir`）

当网站改版导致原本能命中的解析规则持续失效时，需要区分「偶发缺失」与「规则失效」：

- **默认关闭（ADR-004 裁决）**：未显式传入 `--state-dir <目录>` 时，程序**绝不创建或读写任何本地状态文件**。单次合格样本缺字段恒记 `UNKNOWN + FIELD_MISSING`（N=1），不会误报 `SIGNAL_STALE`。
- **显式开启（`--state-dir DIR`）**：
  - 状态文件为 `<DIR>/freshness-v1.json`（`schema = "parallaxe.freshness/v1"`），使用文件锁（`fs2`）与临时文件原子替换；拒绝符号链接与目录逃逸。
  - **去标识化键**：以 `target_id + 规范 URL + surface + rule_id + rule_version + 请求形态 + 实际地址族 + 路径标识` 计算不可逆哈希摘要 `key_digest`——**不存原始查询、代理 URI、公网 IP 或页面内容**。
  - **递增规则**：必须先存在同键历史合格命中，随后在 **3 次不同运行**的合格完整页面上连续缺失（超时、截断、挑战、登录、重试、缓存重放均**不递增**），第 1/2 次报 `UNKNOWN + FIELD_MISSING`，第 3 次报 `SIGNAL_STALE` 并隔离该规则，直至人工复核。
  - **7 天窗口**：距最后一次合格命中超过 7 天的计数自动失效（物理条目按惰性策略清理，ADR-005 D-12）；时钟倒退不复用旧计数。

---

## 5. 离线 Fixture 回归集与净化契约

### 5.1 内置 9 个脱敏合成案例（`--fixture <CASE>`）

内置案例位于 `crates/core/src/fixtures.rs`，全为纯内存合成样本（`mode = "FIXTURE"`、`network_request_count = 0`），正式二进制无法借此获得在线发包权限：

`gemini-positive` · `books-missing` · `legacy-region-negative` · `drive-login` · `news-edition` · `empty-204` · `rate-429` · `truncated-page` · `conflict-tie`

### 5.2 仓库文件级回归夹具（`tests/fixtures/`）

仓库 `tests/fixtures/<target_id>/<case>/` 存放对应的 `meta.json`（`schema = "parallaxe.fixture/v1"`）与 `response.http`，由 `crates/core/tests/fixtures_files.rs` 自动化验证。所有夹具必须遵循**白名单重建净化原则**：
- 只保留 `Content-Type`、白名单重建的 `Location` 与可选 `Content-Length`；
- 严禁包含 `Set-Cookie`、`Cookie`、`Authorization`、真实公网 IP、订阅链接、账号标识符、活跃脚本或超大负载；
- 正式产品不提供「在线录制真实浏览器会话为 fixture」的按钮或命令。
