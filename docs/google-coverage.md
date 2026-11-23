# Google 全服务目标覆盖与目录清点（Google Coverage）

> 本篇说明 Parallaxe box 内置的 **135 项全量目标目录**（107 个 Google 服务表面 + 28 个非 Google 对照目标）、六大预设 Profile、覆盖/未覆盖两栏语义以及「默认空许可」准入机制。  
> 权威数据源：`assets/targets.toml`、`assets/profiles.toml`；校验逻辑：`crates/core/src/targets.rs`。

---

## 1. 为什么坚持「Google 全服务目录」？

许多诊断脚本只探测 `google.com` 或 `gemini.google.com` 就宣称「Google 正常」。但真实排障中，登录链、搜索、Workspace、Play 商店、AI 平台、云控制台和静态资源分别托管在不同的子域与基础设施上，地区表现与连通性高度独立。

Parallaxe box 的硬规则是：
1. **目录完整保留**：不会因为某个服务缺乏匿名地区信号就把它从目录删除；没有地区信号的目标照常登记，观测后诚实输出 `REACHABLE_ONLY` 或 `UNKNOWN`。
2. **逐表面独立登记**：同一主机名下不同路径（如 `www.google.com/`、`/search`、`/travel/flights`、`/generate_204`）分别作为独立目标注册，不混用规则或结果。
3. **不丢弃无 HTTP 历史项**：例如 `google.ggpht`、`google.ytimg`、`google.video` 在阶段 0 研究中未取得首跳 HTTP（`verification = "NO_HTTP"`），依然保留在目录中并标明实测状态。

---

## 2. 目录结构统计（共 135 项）

### 2.1 Google 全服务目录：107 项（12 个 Family · 5 个矩阵组 A–E · 77 个不同入口主机）

#### 按矩阵组（`matrix_group`）分布

| 矩阵组 | 数量 | 覆盖范围与典型表面 |
|---|---:|---|
| **A · 登录与身份链** | 14 | `accounts.google.com`、`/signin/v2/identifier`、`/v3/signin/`、`accounts.youtube.com`、`myaccount.google.com`、`oauth2`/`openidconnect`、`apis.google.com`、`ssl.gstatic.com`、`recaptcha`、`play.google.com/store/account`、`gsi/client`（全员默认保护） |
| **B · 搜索、内容、生产力、AI 与教育** | 49 | Google 首页、Search、Images、Maps、News、Books、Scholar、Trends、Shopping、Translate、Alerts、Arts、Flights、Travel、Finance、Patents、Docs/Sheets/Slides/Forms、Drive、Gmail、Calendar、Meet、Chat、Keep、Tasks、Sites、Photos、Contacts、Groups、Voice、One、Takeout、Gemini Web/Mobile、AI Studio、Gemini Docs/Models、NotebookLM、AI Plans、Colab、Classroom、Edu Meet、Labs、Flow、UK ccTLD、Preferences |
| **C · 媒体、视频与 Play 商店** | 13 | YouTube、YouTube Music、YouTube Studio、YouTube Premium 公开页、Google TV、Play Store 首页/详情/Books/Games/Movies/Console/Country 帮助/Apps |
| **D · 云、开发者与企业平台** | 18 | Cloud Console、Google Cloud、Vertex AI、Cloud Service Health、Firebase、Google Developers、Android Developers、APIs Explorer、Cloud Shell、Cloud Skills Boost、Workspace Admin/Marketplace、AppSheet、Analytics、Tag Manager、Search Console、Merchant Center、Play Developer API 文档 |
| **E · 静态资源与 API 域** | 13 | `gstatic.com`、`googleusercontent.com`、`ggpht.com`、`ytimg.com`、`googlevideo.com`、`generativelanguage.googleapis.com`、`aiplatform.googleapis.com`、`googleapis.com`、`fonts.googleapis.com`、`fonts.gstatic.com`、`lh3.googleusercontent.com`、`storage.googleapis.com`、`www.google.com/generate_204` |
| **Google 合计** | **107** | 其中 `protected = true` 共 **82 项**；当前发行版 `allow_online = true` 为 **0 项** |

#### 按功能分组（`--group` / `family`）精确计数

| `--group` 参数值 | `family` | 目标数 | 代表目标 ID |
|---|---|---:|---|
| `google.identity` | `identity` | 10 | `google.accounts`, `google.myaccount`, `google.signin-id`, `google.gsi` |
| `google.search` | `search` | 10 | `google.home`, `google.search`, `google.maps`, `google.ccuk`, `google.preferences` |
| `google.content` | `content` | 8 | `google.news`, `google.books`, `google.scholar`, `google.flights`, `google.finance` |
| `google.workspace` | `workspace` | 17 | `google.gmail`, `google.drive`, `google.docs`, `google.calendar`, `google.meet`, `google.one` |
| `google.media` | `media` | 5 | `google.youtube`, `google.yt-music`, `google.yt-studio`, `google.premium`, `google.tv` |
| `google.store` | `store` | 11 | `google.play`, `google.play-apps`, `google.play-detail`, `google.play-account` |
| `google.ai` | `ai` | 10 | `google.gemini`, `google.studio`, `google.models`, `google.notebook`, `google.labs`, `google.flow` |
| `google.education` | `education` | 4 | `google.colab`, `google.classroom`, `google.edu-meet`, `google.play-country`（按目录归组） |
| `google.cloud` | `cloud` | 5 | `google.console`, `google.cloud`, `google.vertex`, `google.health`, `google.shell` |
| `google.developer` | `developer` | 10 | `google.firebase`, `google.dev`, `google.android`, `google.admin`, `google.analytics` |
| `google.static` | `static` | 8 | `google.gstatic`, `google.googleusercontent`（`google.usercontent`）, `google.ggpht`, `google.ytimg`, `google.video`, `google.fonts` |
| `google.api` | `api` | 9 | `google.oauth`, `google.openid`, `google.apis`, `google.gen-api`, `google.vertex-api`, `google.storage`, `google.http204` |

> 提示：随时可通过 `parallaxe targets list --group <组名>` 查看精确目标清单与准入状态。

### 2.2 非 Google 独立对照目录：28 项（`[[controls]]`）

对照目标独立存放在 `assets/targets.toml` 的 `[[controls]]` 表中，全部为公开状态页、帮助页、条款页或国内/CDN 对照入口（默认全部 `protected = true`、`allow_online = false`），绝不混入 Google 统计：

| `--group` 参数值 | 目标数 | 包含目标 | 用途边界 |
|---|---:|---|---|
| `control.ai` | 7 | ChatGPT 状态页、OpenAI Docs、Claude 状态页、Claude Docs、Perplexity Docs、Copilot、Grok/xAI Docs | 公开状态/文档面对照，**不代表应用或模型资格** |
| `control.media` | 6 | Netflix 条款页、Disney+ 帮助、Prime Video 帮助、Spotify 支持、TikTok 支持、HBO Max 帮助 | 仅条款/帮助页，**绝不抓片库、不播放、不测 DRM** |
| `control.social` | 3 | X 帮助、Reddit 状态页、Telegram FAQ | 公开状态/帮助面对照 |
| `control.developer` | 3 | GitHub 状态页、npm 状态页、PyPI 状态页 | 开发者平台状态页对照 |
| `control.search` | 1 | Bing 首页 | 非 Google 搜索对照 |
| `control.domestic` | 6 | 12306、淘宝、京东、B 站、微信网页、工行 | 辅助发现国内流量疑似错分流，**不用于证明海外或 Google 地区** |
| `control.cdn` | 2 | Cloudflare `one.one.one.one/cdn-cgi/trace`、`speed.cloudflare.com/meta` | 接收方出口对照（`speed` 曾观测 403，未验证字段不补值） |

---

## 3. 六大内置 Profile 与运行模式

`assets/profiles.toml` 定义了 6 个内置预设（所有预算均受代码硬上限 `HARD_LIMITS` 约束，不可抬高）：

| Profile ID | 命令示例 | 选择范围 | 选中数 | 运行时限 | 单请求时限 | 并发 / 单主机 | 适用场景 |
|---|---|---|---:|---:|---:|---:|---|
| `simple` | `parallaxe check --headless`（默认） | 9 个核心 Google + 6 个对照目标 | 15 | 20 s | 10 s | 4 / 2 | 日常快速排障 |
| `google-all` | `parallaxe check --headless --profile google-all` | 全部 `google.*` 命名空间 | 107 | 120 s | 12 s | 6 / 2 | Google 全服务专项诊断 |
| `full` | `parallaxe check --headless --full` | 全部内置目标（不含用户导入 `user.*`） | 135 | 120 s | 12 s | 6 / 2 | 换线后的全量覆盖体检 |
| `business` | `parallaxe check --headless --profile business` | `google.workspace` + `identity` + `cloud` + `control.developer` | 35 | 60 s | 10 s | 4 / 1 | 跨境办公与云开发场景 |
| `study` | `parallaxe check --headless --profile study` | Books、Scholar、NotebookLM、Classroom、Colab、Edu Meet | 6 | 30 s | 10 s | 3 / 1 | 留学与学术研究场景 |
| `media` | `parallaxe check --headless --profile media` | `google.media` + `google.store` + `control.media` | 22 | 45 s | 10 s | 4 / 1 | 媒体与商店公开面观察 |

> 注意：`--profile`、`--group`、`--target`、`--full` 四种选择器严格互斥；未指定任何选择器时默认走隐式 `simple`（`implicit_default = true`，额外要求目标 `safe_default = true`）。

---

## 4. 覆盖与未覆盖两栏语义（`Coverage`）

每一份报告（CLI 表格、JSON、HTML、CSV、GUI、Agent）都必须并列展示 **`covered`（本次覆盖）** 与 **`uncovered`（本次未覆盖）**，并且满足数学守恒：

> 目录快照中的每一个目标 ID，在 `covered` ∪ `uncovered` 中**恰出现一次**（不漏记、不重复）。

- **何时计入 `covered`**：只要发起了首个传输尝试（即便 TLS 握手失败或没有地区信号）即计入 `covered`，同时记录 `request_count` 与 `http_received`（布尔值）。**收到 HTTP 但无地区信号归入已覆盖，绝不伪装成「地区已判定」。**
- **何时计入 `uncovered`**：未选中（`selected = false`, `reason = "NOT_SELECTED"`）、策略未准入（`selected = true`, 如 `ALLOW_ONLINE_FALSE` / `NO_ADMISSION`）、预算耗尽、或取消前尚未启动的目标均留在 `uncovered`，并附明确原因。

---

## 5. 默认空许可与在线准入状态机

`crates/core/src/targets.rs::Registry::admission` 强制执行五重合取门控：

```text
实际允许在线发包 =
  命中选集
  ∧ target.allow_online == true
  ∧ (若隐式默认选择，则 target.safe_default == true)
  ∧ 存在精确匹配的受信任内嵌 Grant（target_id、exact_url、surface、method 全等）
  ∧ 当前 UTC 日期 ∈ [grant.reviewed_at, grant.valid_until]
  ∧ 该目标全部启用规则的 version 与 grant.rule_versions 完全一致
```

**当前发行版基线**：`assets/targets.toml` 中 `admission.grants = []` 且全部 135 个内置目标 `allow_online = false`。
- 因此执行 `parallaxe check --headless --profile google-all --json` 时，必定输出：`selected_count = 107`、`covered = []`、`network_request_count = 0`、`exit_code = 3`。
- 外部覆盖层（`--targets-dir`）**禁止携带 `[admission]` 块**，也不能把已有内置目标的 `allow_online` 改为 `true` 绕过准入。
- 若需在不修改内置目录的情况下对自有/获授权网址执行受控检查，请使用 GUI 的「自定义导入」流程（强制两段责任声明与一次性弹窗授权，详见 [user-import.md](user-import.md)）。
