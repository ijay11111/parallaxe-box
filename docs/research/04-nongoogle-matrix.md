# 04 非 Google 对照矩阵：对照的是路径，不是各站共享的地区

研究日 `2026-11-21`。来源类型为 DIRECT_ANONYMOUS_OBSERVATION：各行 URL 即一手来源；研究代理、空 Cookie、未登录、无 API Key、一个 URL 一次 GET，只读首跳，未指定地址族，限制同 [03](03-google-matrix.md)。**28 个独立对照 URL 中 27 个取得 HTTP，1 个在 TLS 阶段失败。** 状态/帮助/开发文档与产品入口分别命名；即使同属一家公司，状态页的 200 也不是在线业务可用证据。自动访问受限或资格依赖登录的业务端不测，见 [08](08-compliance.md)。不登录、不播放、不请求模型、不提交搜索或交易。

公共观测前提适用于每行：`ANONYMOUS_ONLY`。下表“高”只指测到了某状态/主机；所有行对**产品国家**均“无证据”，除特别说明的 CF 出口映射外不投地区票。各行“已实测”仅限所列 URL/字段；业务权限一律未测。没有正文资格证据时，普通公开页最高 `REACHABLE_ONLY`；挑战、拒绝原因不明或业务层未查均 `UNKNOWN`。无一进入默认配置。HTTP/TLS 的“已实测”不继承到业务层：例如淘宝记录的事实是 TLS EOF，而不是确认服务器拒绝连接；CF speed 的 403 也不证明接口停用。

| 类别 / 服务 | 匿名观测地址 | 本次首跳或字段 | 可信度与失效条件；为什么不能扩大解释 |
|---|---|---|---|
| AI · ChatGPT | <https://status.openai.com/> | 已实测 200，状态站 | 高/状态站可独立托管或缓存；不是对话可用性 |
| AI · OpenAI API | <https://platform.openai.com/docs/overview> | 已实测 302 同域 | 高/文档迁移；无 key，不测模型、余额或项目 |
| AI · Claude | <https://status.claude.com/> | 已实测 200，状态站 | 高/即使状态正常也不能推出用户可进入 Claude |
| AI · Claude API | <https://platform.claude.com/docs/en/home> | 已实测 200，文档 | 高/文档静态分发；模型权限未知 |
| AI · Perplexity | <https://docs.perplexity.ai/> | 已实测 308 同域 | 高/文档跳转与搜索后端分离 |
| AI · Copilot | <https://www.microsoft.com/en-us/microsoft-copilot> | 已实测 301 同域 | 高/URL 已指定页面语言和市场，非服务定位 |
| AI · Grok/xAI | <https://docs.x.ai/> | 已实测 308 同域 | 高/不能将文档视作 Grok Web 会话 |
| 媒体 · Netflix | <https://help.netflix.com/legal/termsofuse> | 已实测 200，条款页 | 高/法律页与播放服务不同；不取片单、DRM 或播放权 |
| 媒体 · Disney+ | <https://help.disneyplus.com/> | 已实测 200，帮助页 | 高/帮助中心有独立路由；不代表订阅可用 |
| 媒体 · Prime Video | <https://www.primevideo.com/help> | 已实测 200，帮助页 | 高/分区版权与账号不在此表面 |
| 媒体 · Spotify | <https://support.spotify.com/> | 已实测 302 同域 | 高/帮助页版次可重定向，不能证明账号市场 |
| 媒体 · TikTok | <https://support.tiktok.com/> | 已实测 302 → www.tiktok.com | 高/仅记录交接，不能判视频可播放 |
| 媒体 · HBO Max | <https://help.hbomax.com/> | 已实测 302 同域 | 高/帮助中心不提供授权证据 |
| 社交 · X | <https://help.x.com/> | 已实测 403 | 高/拒绝原因未知；不可推断封国或封 IP |
| 社交 · Reddit | <https://www.redditstatus.com/> | 已实测 200 | 高/状态站不是用户流量面的同路对照 |
| 社交 · Telegram | <https://telegram.org/faq> | 已实测 200 | 高/FAQ 不能验证消息协议通道 |
| 开发 · GitHub | <https://www.githubstatus.com/> | 已实测 200 | 高/不能证明登录、Git 或私库能力 |
| 开发 · npm | <https://status.npmjs.org/> | 已实测 200 | 高/未请求包，不保证下载链 |
| 开发 · PyPI | <https://status.python.org/> | 已实测 200 | 高/基础设施状态页，不证明 wheel 交付 |
| 搜索 · Bing | <https://www.bing.com/> | 已实测 200 | 高/仅首页，无查询、无地区来源证明 |
| 国内 · 12306 | <https://www.12306.cn/> | 已实测 200 | 高/首页不是购票服务；历史 SSL 失败非本次结果 |
| 国内 · 淘宝 | <https://www.taobao.com/> | TLS EOF，已尝试；服务待验证 | 高/该连接未成功，低/归因；无 HTTP 不能定为地区或平台封禁 |
| 国内 · 京东 | <https://www.jd.com/> | 已实测 301 → corporate.jd.com | 高/企业站交接非商品服务；本次未分段测时延 |
| 国内 · B 站 | <https://www.bilibili.com/> | 已实测 302 同域 | 高/不验证媒体播放及用户地区 |
| 国内 · 微信网页 | <https://wx.qq.com/> | 已实测 200 | 高/不扫二维码、不验证账号或会话 |
| 国内 · 工行 | <https://www.icbc.com.cn/> | 已实测 200 | 高/只触达公开入口，不访问网银或交易 |
| CDN · Cloudflare trace | <https://one.one.one.one/cdn-cgi/trace> | 已实测 200；`loc=US`、`colo=SEA`、`http=http/1.1`、`tls=TLSv1.3`；`ip` 字段存在，值不保存 | 高/只描述这一请求：`loc` 是 CF 对出口 IP 的国家映射；`colo` 是处理请求的接入机房 IATA 标识。代理换路或缓存时不可外推；都不是 Google 国家 |
| CDN · Cloudflare speed meta | <https://speed.cloudflare.com/meta> | 已实测 403，应用 JSON 类型；地区字段待验证 | 高/有拒绝响应；`clientIp/country/colo/asn` 此次无可用样本，不能按 TECH-REF 的示例填值。也不索取城市、坐标或邮编 |

**跨表索引，不重复计数：** Gemini API（`google.models`，403）与 YouTube Premium（`google.premium`，200）均属 Google；分别用来对比“无凭据请求收到 403、原因未证”和“营销页成功但资格未知”。不把它们算入非 Google AI/媒体的数量。CF 两端同属一个提供方，不能算两家独立出口共识；[CF 官方文档](https://developers.cloudflare.com/fundamentals/reference/cdn-cgi-endpoint/)明确 trace 的一个用途是识别处理请求的数据中心。

### 国内组怎样区分错分流与偶发不通

历史 S1 中严格 Global 时出现 12306/银行 SSL 失败、淘宝握手失败、京东极慢。**在该次排障中**结合已知全局模式和路径对照，指向国内流量被送往境外；失败本身不是路由证据。本轮 12306/工行 200 与淘宝 EOF 同时发生，更不能复用那次归因。建议在用户已有、无需改设置的两条路径上，同一时段、同一地址族、同一目标分别记录 DNS、连接/TLS、首包、路由/代理观察与 HTTP；仅当多目标稳定差异且路由证据相符时报告“疑似错分流”。仅一条路径、代理远端解析不明、只见一家慢或遇挑战 → `INCONCLUSIVE`。不得主动切全局、切节点、探银行内网或用国内组给 Google 判国。
