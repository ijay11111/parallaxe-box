# 03 Google 全服务地址矩阵：逐地址首跳，而非一个 Google 国家

研究日 `2026-11-21`（上海）；实测类型 `DIRECT_ANONYMOUS_OBSERVATION`。这里登记的是原规格 §3.4 A–G 的服务表面，按任务卡重新归入 A–E 五组；`service_family` 保留十二类。**107 个唯一 Google URL 均逐址尝试，不含 Cloudflare，不把一跳的多个参数当多个目标。** 目标目录、拿到 HTTP、拿到地区字段、准入默认是四个不同的计数。

### 路径、复核方法与限制

本轮从研究环境已有、无嵌入凭据的 HTTPS 代理路径访问公开 URL；Python 标准库发 GET，启用证书校验，每个地址一个新客户端，显式空 Cookie、不带认证、不存 Cookie、不执行 JS。只看首跳；Location 仅保留主机与白名单版本参数，不保存其余查询、用户标识、IP、响应体或整套头。收到指向登录/挑战的响应即停。没有绑定网卡或确认代理远端的地址族、DNS、缓存与 TUN，**不是用户节点的实测**。本机 curl/Schannel 预检曾在代理握手后报本地凭据错误，未将 CONNECT 200 算站点响应。预检另有两个成功 GET（首页与 Books），不混入下表采样计数。

表内每址预算 1 次逻辑 GET，未自动重试或跟跳；套接字超时 9 秒、正文读取上限 16 MiB（多读一个字节检测截断）；并发最多 4、同主机最多 2。研究时仅在内存筛选严格限定的字段。复核者在获准路径用同一 URL、方法及 `Parallaxe-Research/0 (signed-out; public page only)` 标识，照表检查指定字段；不要带拒绝/同意页 Cookie，不为复现修改系统网络，服务条款不允许自动访问时用本表留存的脱敏片段做离线检查。裸域 4xx 仍是有 HTTP 响应，不等于产品被封。

### P0 原始响应摘录（只摘当条响应存在的内容）

下列反引号为状态行前部、正文子串或 Location 中的独立子串，并非完整报文；大小为读取字节。没有从标题、主机名或单次缺失反推国家。

| 目标 | 响应片段 | 此样本支持什么，不支持什么 |
|---|---|---|
| 首页 `/` | `<title>Google</title>`、`,2,1,200,"USA"` | 未命中旧式 `name="region" value="XX"`。后者是已失效反例；内联三字母的**意义**也没有官方保证。 |
| Books `/` | `<title>Google Books: Find, Preview, and Read Books Online.</title>` | 200、375941 字节，未命中旧三字母形态；不是 CHN、也不是 USA 的证据。 |
| Gemini `/` | `,2,1,200,"USA"` | 200、986209 字节；只证明这份 HTML 有此字段，不证明 AI 可用。 |
| YouTube `/` | `"GL":"US"`、`<title>YouTube</title>` | 当前页面配置；设置或默认版也能给 US。不能判会员、出口；条款审查见 08。 |
| News `/` | `hl=en-US`、`gl=US`、`ceid=US:en` | 同一 302 Location 中三个参数，不是三票。 |
| Play `/store` | `HTTP/1.1 302`；Location 主机 `play.google.com` | 首跳正文 0 字节，不能给它填应用目录的国旗。 |
| Play `/store/apps` | `regionflags/us.png` | 200、2243796 字节；本轮只确认资源名，未命中历史成对的英文页脚文案，不当付款资料。 |
| Drive `/` | `HTTP/1.1 302`；Location 主机 `accounts.google.com` | 登录交接，不跟随、不测文件内容。 |
| Gmail `/` | `HTTP/1.1 301`；Location 主机 `mail.google.com` | 首跳同域；未证明收件箱可用。 |
| Calendar `/` | `HTTP/1.1 302`；Location 主机 `calendar.google.com` | 首跳同域；11 秒历史案例是 IPv6 路径，不是地区。 |
| Accounts `/` | `HTTP/1.1 302`；Location 主机 `accounts.google.com` | 看得到路由入口，不能知道账号及关联国。 |
| Premium `/premium` | `HTTP/1.1 200` | 读到 1031781 字节；无可接受的资格证据，未登录、未播放、未试购。 |

### 读表约定（列的公共值是每行契约的一部分）

- 每行地址均为 `canonical_url`（本次实际请求入口，旧入口保留，不把首跳当最终地址），也是该行 `source_url`；时间为 UTC 时分秒。`probe_kind=HTTP GET / 首跳`；`request_budget=1`；观测前提均为“未登录/匿名/既有研究代理”。所有行 `non_observable` 至少含账号、付款、设备、组织、订阅和项目真实区域。字段列前半是表面/检查项，不表示命中；后半的本次摘录才是观测。
- 可信度列分“这份响应是否包含所见字段”和“字段的地区含义”；对 HTTP 事实为高，对地区只有表中所列，`无`=没有可打分的地区证据。`已实测` 指收到了响应，不代表地区可判；`待验证` 可与已测首跳并存。无地区一律 `region=null + NO_ANONYMOUS_REGION_SIGNAL`；普通公开入口且已确认可达仅 `REACHABLE_ONLY`；认证、挑战、未知错误取 `UNKNOWN` 附实际状态；未建立 TLS 另记链路错误，不算服务拒绝。
- `stale_after`（研究建议，不是 TTL 实测）：首跳/传输只对**当次**有效；无文档的内容字段下次运行必须重采，样本超过 7 天只供历史展示，不再引用为现况；缺失计数按 [05](05-signals.md)。**每行 `safe_default=false`**：单一路径初测不是合规批准或跨路径复现。保护列“是”表示保护，“否”也绝不等于可阻断；所有行本次均无 Guard 准入证据。保护以主机为单位：例如同在 `www.google.com` 的 Flights、Travel、Finance、Preferences 全继承主机保护；跳到该受保护入口的 Shopping、英国域，以及与 Premium 共用主机的 YouTube 首页也不作阻断候选。单有 Location 主机而无明确版本参数，仅是传输证据，地区可信度为无。

逐址账目：107 次逻辑 GET 尝试；104 个有 HTTP 首跳、3 个握手/超时未获 HTTP；不含前述预检、官方来源检索和非 Google 对照。

### A 身份与登录链（14）

| target_id · family | 请求地址 / 一手来源 | 表面及读取字段 | 本次响应（UTC） | 来源层；地区可信度 | 失效条件 / 降级 | 保护 | 验证状态 |
|---|---|---|---|---|---|---|---|
| `google.accounts` · identity | <https://accounts.google.com/> | 登录入口：首跳主机/状态 | 07:25:16 · 302 text/html → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.signin-id` · identity | <https://accounts.google.com/signin/v2/identifier> | 登录入口：首跳主机/状态 | 07:25:16 · 302 text/html → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.signin-v3` · identity | <https://accounts.google.com/v3/signin/> | 登录入口：首跳主机/状态 | 07:25:16 · 404 text/html | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.yt-accounts` · identity | <https://accounts.youtube.com/> | 登录入口：首跳主机/状态 | 07:25:16 · 404 text/html | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.myaccount` · identity | <https://myaccount.google.com/> | 登录入口：首跳主机/状态 | 07:25:17 · 302 application/binary → `myaccount.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.signinoptions` · identity | <https://myaccount.google.com/signinoptions> | 登录入口：首跳主机/状态 | 07:25:17 · 404 text/html | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.security` · identity | <https://myaccount.google.com/security> | 登录入口：首跳主机/状态 | 07:25:17 · 302 application/binary → `myaccount.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.oauth` · api | <https://oauth2.googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:18 · 404 text/html | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.openid` · api | <https://openidconnect.googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:18 · 404 text/html | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.apis` · api | <https://apis.google.com/> | API 根或列表：状态/响应类型 | 07:25:18 · 301 text/html → `developers.google.com` | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.ssl-gstatic` · static | <https://ssl.gstatic.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:18 · 404 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.recaptcha` · identity | <https://www.gstatic.com/recaptcha/> | 静态/共享根：TLS、HTTP 状态 | 07:25:19 · 404 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.play-account` · store | <https://play.google.com/store/account> | 登录入口：首跳主机/状态 | 07:25:19 · 302 application/binary → `play.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.gsi` · identity | <https://accounts.google.com/gsi/client> | 静态/共享根：TLS、HTTP 状态 | 07:25:19 · 403 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |

### B 内容、AI、教育与生产力（49）

| target_id · family | 请求地址 / 一手来源 | 表面及读取字段 | 本次响应（UTC） | 来源层；地区可信度 | 失效条件 / 降级 | 保护 | 验证状态 |
|---|---|---|---|---|---|---|---|
| `google.home` · search | <https://www.google.com/> | 首页 HTML：旧 region 与内联三字码 | 07:25:19 · 200 text/html；`,2,1,200,"USA"`；`<title>Google</title>` | 未公开字段；低 | 改版、A/B 或字段重名，弃地区值 | 是 | 已实测·首跳 |
| `google.search` · search | <https://www.google.com/search> | 搜索入口：是否明确标来源 | 07:25:19 · 302 text/html → `www.google.com` | 服务归因标签；本轮无 | 无查询/无页脚/混入设备标签即不用 | 是 | 已实测·首跳 |
| `google.images` · search | <https://images.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:19 · 302 text/html → `images.google.com.hk`；`hl=zh-CN` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 否 | 已实测·首跳 |
| `google.maps` · content | <https://maps.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:20 · 302 text/html → `maps.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 否 | 已实测·首跳 |
| `google.news` · content | <https://news.google.com/> | 版次跳转：Location 中 hl/gl/ceid | 07:25:20 · 302 application/binary → `news.google.com`；`hl=en-US`；`gl=US`；`ceid=US:en` | 用户控制或版次；中（仅版本） | 手动参数、缓存或无跳转；不投出口票 | 否 | 已实测·首跳 |
| `google.books` · content | <https://books.google.com/> | 页面内联：,2,1,200 后三字码 | 07:25:20 · 200 text/html；`<title>Google Books: Find, Preview, and Read Books Online.</title>` | 此响应无该地区字段 | 版式、内容协商、壳页；不跨站借值 | 否 | 已实测·首跳；地区字段待验证 |
| `google.scholar` · search | <https://scholar.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:20 · 200 text/html | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 否 | 已实测·首跳 |
| `google.trends` · search | <https://trends.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:20 · 301 text/html → `trends.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 否 | 已实测·首跳 |
| `google.shopping` · store | <https://shopping.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:21 · 301 text/html → `www.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 是 | 已实测·首跳 |
| `google.translate` · content | <https://translate.google.com/> | 展示入口：Location 主机/公开版次参数 | 07:25:21 · 200 text/html | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 否 | 已实测·首跳 |
| `google.alerts` · search | <https://www.google.com/alerts> | 登录入口：首跳主机/状态 | 07:25:21 · 200 text/html | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.arts` · content | <https://artsandculture.google.com/> | 公开说明/目录：状态或新入口 | 07:25:22 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.flights` · search | <https://www.google.com/travel/flights> | 展示入口：Location 主机/公开版次参数 | 07:25:22 · 302 application/binary → `www.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 是 | 已实测·首跳 |
| `google.travel` · content | <https://www.google.com/travel/> | 展示入口：Location 主机/公开版次参数 | 07:25:22 · 302 application/binary → `www.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 是 | 已实测·首跳 |
| `google.finance` · content | <https://www.google.com/finance/> | 展示入口：Location 主机/公开版次参数 | 07:25:23 · 302 application/binary → `www.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 是 | 已实测·首跳 |
| `google.patents` · search | <https://patents.google.com/> | 公开说明/目录：状态或新入口 | 07:25:23 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.docs` · workspace | <https://docs.google.com/document/> | 登录入口：首跳主机/状态 | 07:25:23 · 302 application/binary → `www.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.sheets` · workspace | <https://docs.google.com/spreadsheets/> | 登录入口：首跳主机/状态 | 07:25:23 · 302 application/binary → `www.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.slides` · workspace | <https://docs.google.com/presentation/> | 登录入口：首跳主机/状态 | 07:25:24 · 302 application/binary → `www.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.forms` · workspace | <https://docs.google.com/forms/> | 登录入口：首跳主机/状态 | 07:25:24 · 302 text/html → `docs.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.drive` · workspace | <https://drive.google.com/> | 登录入口：首跳主机/状态 | 07:25:24 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.gmail` · workspace | <https://mail.google.com/> | 登录入口：首跳主机/状态 | 07:25:24 · 301 text/html → `mail.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.calendar` · workspace | <https://calendar.google.com/> | 登录入口：首跳主机/状态 | 07:25:25 · 302 application/binary → `calendar.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.meet` · workspace | <https://meet.google.com/> | 登录入口：首跳主机/状态 | 07:25:25 · 302 application/binary → `meet.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.chat` · workspace | <https://chat.google.com/> | 登录入口：首跳主机/状态 | 07:25:25 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.keep` · workspace | <https://keep.google.com/> | 登录入口：首跳主机/状态 | 07:25:25 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.tasks` · workspace | <https://tasks.google.com/> | 登录入口：首跳主机/状态 | 07:25:26 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.sites` · workspace | <https://sites.google.com/> | 登录入口：首跳主机/状态 | 07:25:26 · 302 text/html → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.photos` · content | <https://photos.google.com/> | 登录入口：首跳主机/状态 | 07:25:26 · 302 application/binary → `www.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.contacts` · workspace | <https://contacts.google.com/> | 登录入口：首跳主机/状态 | 07:25:26 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.groups` · workspace | <https://groups.google.com/> | 登录入口：首跳主机/状态 | 07:25:27 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.voice` · workspace | <https://voice.google.com/> | 登录入口：首跳主机/状态 | 07:25:27 · 302 text/html → `workspace.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.one` · store | <https://one.google.com/> | 公开说明/目录：状态或新入口 | 07:25:27 · 307 application/binary → `one.google.com` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.takeout` · identity | <https://takeout.google.com/> | 登录入口：首跳主机/状态 | 07:25:27 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.gemini` · ai | <https://gemini.google.com/> | 页面内联：,2,1,200 后三字码 | 07:25:28 · 200 text/html；`,2,1,200,"USA"` | 未公开字段；低（仅命中时） | 版式、内容协商、壳页；不跨站借值 | 否 | 已实测·首跳 |
| `google.gemini-mobile` · ai | <https://support.google.com/gemini/answer/14579026> | 公开说明/目录：状态或新入口 | 07:25:28 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.studio` · ai | <https://aistudio.google.com/> | 公开资格入口：HTTP/明确门槛说明 | 07:25:28 · 302 application/binary → `aistudio.google.com` | 服务策略；无地区码 | 普通 200/403、营销文案不证明资格 | 是 | 已实测·首跳 |
| `google.gemini-doc` · ai | <https://ai.google.dev/gemini-api/docs> | 公开说明/目录：状态或新入口 | 07:25:28 · 301 text/html → `aistudio.google.com` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.models` · ai | <https://generativelanguage.googleapis.com/v1beta/models> | API 根或列表：状态/响应类型 | 07:25:29 · 403 application/json | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.notebook` · ai | <https://notebooklm.google.com/> | 公开资格入口：HTTP/明确门槛说明 | 07:25:29 · 301 application/binary → `notebook.google.com` | 服务策略；无地区码 | 普通 200/403、营销文案不证明资格 | 否 | 已实测·首跳 |
| `google.ai-plans` · ai | <https://one.google.com/about/google-ai-plans/> | 公开说明/目录：状态或新入口 | 07:25:29 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.colab` · education | <https://colab.research.google.com/> | 登录入口：首跳主机/状态 | 07:25:29 · 200 text/html | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.classroom` · education | <https://classroom.google.com/> | 登录入口：首跳主机/状态 | 07:25:30 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.edu-meet` · education | <https://edu.google.com/workspace-for-education/products/meet/> | 公开说明/目录：状态或新入口 | 07:25:30 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.labs` · ai | <https://labs.google/> | 公开说明/目录：状态或新入口 | 07:25:30 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.flow` · ai | <https://labs.google/fx/tools/flow> | 公开说明/目录：状态或新入口 | 07:25:31 · 308 text/html → `flow.google.com` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.ccuk` · search | <https://www.google.co.uk/> | 展示入口：Location 主机/公开版次参数 | 07:25:45 · 301 text/html → `www.google.com` | 展示版次；低（仅返回值） | 可选语言、地区、缓存；不反推出口 | 是 | 已实测·首跳 |
| `google.flow-current` · ai | <https://flow.google.com/> | 公开说明/目录：状态或新入口 | 07:27:19 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.preferences` · search | <https://www.google.com/preferences> | 页面内联：,2,1,200 后三字码 | 07:27:19 · 200 application/xhtml+xml | 此响应无该地区字段 | 版式、内容协商、壳页；不跨站借值 | 是 | 已实测·首跳；地区字段待验证 |

### C 媒体与商店（13）

| target_id · family | 请求地址 / 一手来源 | 表面及读取字段 | 本次响应（UTC） | 来源层；地区可信度 | 失效条件 / 降级 | 保护 | 验证状态 |
|---|---|---|---|---|---|---|---|
| `google.youtube` · media | <https://www.youtube.com/> | 媒体配置：HTML 中 GL | 07:25:31 · 200 text/html；`"GL":"US"`；`<title>YouTube</title>` | 用户控制或版次；中（仅版本且命中） | 默认 US、同意页、配置缺失；不测播放 | 是 | 已实测·首跳 |
| `google.yt-music` · media | <https://music.youtube.com/> | 媒体配置：HTML 中 GL | 07:25:31 · 200 text/html | 此响应无该地区字段 | 默认 US、同意页、配置缺失；不测播放 | 否 | 已实测·首跳；地区字段待验证 |
| `google.yt-studio` · media | <https://studio.youtube.com/> | 登录入口：首跳主机/状态 | 07:25:31 · 302 application/binary → `accounts.google.com`；`hl=en` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.premium` · media | <https://www.youtube.com/premium> | 公开资格入口：HTTP/明确门槛说明 | 07:25:31 · 200 text/html | 服务策略；无地区码 | 普通 200/403、营销文案不证明资格 | 是 | 已实测·首跳 |
| `google.tv` · media | <https://tv.google/> | 公开说明/目录：状态或新入口 | 07:25:32 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.play` · store | <https://play.google.com/store> | 商店 HTML：国旗资源及配对文案 | 07:25:32 · 302 application/binary → `play.google.com` | 此响应无该地区字段 | 选择列表、文案缺位或重定向不算选中值 | 是 | 已实测·首跳；地区字段待验证 |
| `google.play-detail` · store | <https://play.google.com/store/apps/details?id=com.google.android.youtube> | 商品/分类页：状态、公开展示面 | 07:25:32 · 200 text/html | 目录版次；无可靠国家 | 可见不等于可买；币种、目录因版本变化 | 是 | 已实测·首跳 |
| `google.play-books` · store | <https://play.google.com/store/books> | 商品/分类页：状态、公开展示面 | 07:25:32 · 200 text/html | 目录版次；无可靠国家 | 可见不等于可买；币种、目录因版本变化 | 是 | 已实测·首跳 |
| `google.play-games` · store | <https://play.google.com/store/games> | 商品/分类页：状态、公开展示面 | 07:25:33 · 200 text/html | 目录版次；无可靠国家 | 可见不等于可买；币种、目录因版本变化 | 是 | 已实测·首跳 |
| `google.play-movies` · store | <https://play.google.com/store/movies> | 商品/分类页：状态、公开展示面 | 07:25:33 · 200 text/html | 目录版次；无可靠国家 | 可见不等于可买；币种、目录因版本变化 | 是 | 已实测·首跳 |
| `google.play-console` · developer | <https://play.google.com/console/> | 登录入口：首跳主机/状态 | 07:25:33 · 301 application/binary → `play.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.play-country` · store | <https://support.google.com/googleplay/answer/7431675> | 公开说明/目录：状态或新入口 | 07:25:34 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.play-apps` · store | <https://play.google.com/store/apps> | 商店 HTML：国旗资源及配对文案 | 07:27:19 · 200 text/html；`regionflags/us.png` | 商店版次；低（仅素材） | 选择列表、文案缺位或重定向不算选中值 | 是 | 已实测·首跳 |

### D 云与开发者（18）

| target_id · family | 请求地址 / 一手来源 | 表面及读取字段 | 本次响应（UTC） | 来源层；地区可信度 | 失效条件 / 降级 | 保护 | 验证状态 |
|---|---|---|---|---|---|---|---|
| `google.console` · cloud | <https://console.cloud.google.com/> | 云控制台或状态页：首跳/服务入口 | 07:25:34 · 302 application/binary → `accounts.google.com` | 资源或登录；无 | 状态页区域不指人；租户配置不可见 | 是 | 已实测·首跳 |
| `google.cloud` · cloud | <https://cloud.google.com/> | 公开说明/目录：状态或新入口 | 07:25:35 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.vertex` · cloud | <https://cloud.google.com/vertex-ai> | 公开说明/目录：状态或新入口 | 07:25:35 · 301 application/binary → `cloud.google.com` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 是 | 已实测·首跳 |
| `google.health` · cloud | <https://status.cloud.google.com/> | 云控制台或状态页：首跳/服务入口 | 07:25:35 · 200 text/html | 资源或登录；无 | 状态页区域不指人；租户配置不可见 | 是 | 已实测·首跳 |
| `google.firebase` · developer | <https://firebase.google.com/> | 公开说明/目录：状态或新入口 | 07:25:36 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.dev` · developer | <https://developers.google.com/> | 公开说明/目录：状态或新入口 | 07:25:36 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.android` · developer | <https://developer.android.com/> | 公开说明/目录：状态或新入口 | 07:25:36 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.api-explorer` · developer | <https://developers.google.com/apis-explorer> | 公开说明/目录：状态或新入口 | 07:25:36 · 200 text/html；`<title>Google</title>` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.shell` · cloud | <https://shell.cloud.google.com/> | 登录入口：首跳主机/状态 | 07:25:37 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.skills` · education | <https://www.cloudskillsboost.google/> | 公开说明/目录：状态或新入口 | 07:25:37 · 308 text/html → `www.skills.google` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.admin` · workspace | <https://admin.google.com/> | 登录入口：首跳主机/状态 | 07:25:38 · 302 text/html → `www.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.marketplace` · workspace | <https://workspace.google.com/marketplace> | 公开说明/目录：状态或新入口 | 07:25:38 · 200 text/html | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.appsheet` · developer | <https://www.appsheet.com/> | 公开说明/目录：状态或新入口 | 07:25:38 · 302 text/plain → `about.appsheet.com` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |
| `google.analytics` · developer | <https://analytics.google.com/> | 登录入口：首跳主机/状态 | 07:25:39 · 301 text/html → `analytics.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.tagmanager` · developer | <https://tagmanager.google.com/> | 登录入口：首跳主机/状态 | 07:25:39 · 302 application/binary → `accounts.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.search-console` · developer | <https://search.google.com/search-console/> | 登录入口：首跳主机/状态 | 07:25:39 · 302 application/binary → `search.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.merchant` · store | <https://merchants.google.com/> | 登录入口：首跳主机/状态 | 07:25:40 · 302 text/html → `merchants.google.com` | 账号依赖；无 | 入口换版或登录墙，停在首跳 | 是 | 已实测·首跳 |
| `google.play-api` · developer | <https://developers.google.com/android-publisher> | 公开说明/目录：状态或新入口 | 07:25:40 · 200 text/html；`<title>Google</title>` | 公开资料；无 | 文档、营销页或重命名均不能证明资格 | 否 | 已实测·首跳 |

### E 静态与 API 域（13）

| target_id · family | 请求地址 / 一手来源 | 表面及读取字段 | 本次响应（UTC） | 来源层；地区可信度 | 失效条件 / 降级 | 保护 | 验证状态 |
|---|---|---|---|---|---|---|---|
| `google.gstatic` · static | <https://gstatic.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:40 · 301 text/html → `www.gstatic.com` | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.usercontent` · static | <https://googleusercontent.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:41 · 404 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.ggpht` · static | <https://ggpht.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:41 · TimeoutError | 未获 HTTP；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已尝试·服务待验证 |
| `google.ytimg` · static | <https://ytimg.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:41 · TimeoutError | 未获 HTTP；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已尝试·服务待验证 |
| `google.video` · static | <https://googlevideo.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:41 · SSLCertVerificationError | 未获 HTTP；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已尝试·服务待验证 |
| `google.gen-api` · api | <https://generativelanguage.googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:42 · 404 text/html | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.vertex-api` · api | <https://aiplatform.googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:42 · 404 text/html | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.googleapis` · api | <https://googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:42 · 404 text/html | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.fonts-api` · api | <https://fonts.googleapis.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:43 · 404 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.fonts` · static | <https://fonts.gstatic.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:43 · 404 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.lh3` · static | <https://lh3.googleusercontent.com/> | 静态/共享根：TLS、HTTP 状态 | 07:25:44 · 400 text/html | 传输；无 | 根非资源；4xx/证书失败不代表实际下载失败 | 是 | 已实测·首跳 |
| `google.storage` · api | <https://storage.googleapis.com/> | API 根或列表：状态/响应类型 | 07:25:44 · 400 application/xml | 认证或资源；无 | 裸根无方法、项目/key 缺失；不补认证 | 是 | 已实测·首跳 |
| `google.http204` · api | <https://www.google.com/generate_204> | 204：响应体长度及疑似地区头 | 07:25:45 · 204 text/plain；正文 0 B，匹配头名 0 | 链路；无 | 门户拦截/新头须另验；不以 204 判国 | 是 | 已实测·首跳 |

### A2 域名核对（不据此发包、不用于阻断）

77 个不同首跳主机；以域名标签边界做本地比对，[snapei 的 google.txt](https://raw.githubusercontent.com/snapei/clash-pro-rules/release/google.txt) 显式后缀命中 68 个；[v2fly 的 google 表](https://raw.githubusercontent.com/v2fly/domain-list-community/master/data/google) 仅取本文件素域名、不展开 include，命中 68 个。前表未命中的有：`accounts.youtube.com`、`ai.google.dev`、`developer.android.com`、`googlevideo.com`、`music.youtube.com`、`studio.youtube.com`、`www.appsheet.com`、`www.youtube.com`、`ytimg.com`。漏收不等于非 Google，命中也不证明安全阻断。广告、下载主机与历史别名不为凑数扩探；共享依赖另见 [06](06-blocking.md)。旧第二来源在匿名路径 404，见 [ADR-001](../../docs/decisions.md)。
