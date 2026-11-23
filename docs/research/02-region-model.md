# 02 七种地区含义：先问“是谁的地区”

研究日 / `verified_at=2026-11-21`（适用于每行）。**同一个请求可以有几个地区，且都没有错。** 条款页面的国家版本、新闻版次、接入机房不是三个测量出口的温度计。运行事实见 [03](03-google-matrix.md)、[04](04-nongoogle-matrix.md)；下列官方链接证明字段语义或政策，不是对当前请求的测量。

| 归因层 | 定义与可观测量 | 匿名可得性、可信度及失效条件 | 最容易犯的错 |
|---|---|---|---|
| 1 出口网络 | 接收方看见的网络地址及其地理库映射；另记地址族、resolver 和边缘位置 | CF trace 字段本路径已实测；高仅针对该次接收方观测。走代理、缓存或多出口即不能代表另一条路径 | 用 CDN 机房、RIR 注册国或 DNS 出口当物理位置 |
| 2 服务地区策略 | 未登录条款版本 `signed_out_location_observation`；每个产品自己的公开版次、目录或明确地区提示 | 条款按使用位置；页面不一定写位置。已测 Gemini 内联值，字段存在可见、策略语义低；缺上下文、页面换版、挑战即停止归因 | 把一款产品的地区和公开可用国家清单推广到全 Google 或账号资格 |
| 3 账号关联 | Google 为账号关联地区以确定条款和提供方；新号与创建地有关，长期账号可按通常使用地 | 官方语义高；个人值未观测且禁止验证。`account_country=NOT_OBSERVABLE_ANONYMOUSLY` | 将匿名条款版本或登录跳转冒充账号国家 |
| 4 付款/Play 档案 | 付款资料与账号 Play 国家影响购买/订阅；另记商店公开版本 | 官方语义高；`payments_profile_country=NOT_OBSERVABLE_ANONYMOUSLY`。Play 图标已测，只对页面素材有意义；多国选择器、缓存或展示语更换会使推断失效 | 用币种或免费应用推测账单地址。`play_country_observed` 只描述公开商店，不写入 `current_network_country` |
| 5 用户偏好/请求版本 | `hl/gl/uule/ceid`、YouTube location 等可影响展示 | 个人保存的偏好不可观测、工具不碰；**当前请求参数和跳转参数可见**。News 返回键值已实测，版本语义中，出口语义无；即使未传参也不能排除默认版或缓存 | 以语言推国，或把服务回显当独立的第二票 |
| 6 设备位置 | 用户设备授权的位置，不是 IP 地理库 | 无头不调用定位服务：`device_location=NOT_AVAILABLE_IN_HEADLESS_MODE`。官方区分高；当前设备值不采，不补猜 | 把 IP 定位等同于 GPS 或“精确定位” |
| 7 云资源与服务端点 | 项目资源、模型端点、Workspace 数据驻留的配置区域；可另记提供服务的边缘 | 区域目录可匿名查，语义高；项目实际配置/租户策略不可观测。global/多区域、模型差异和端点更名都会破坏一对一映射 | 将 API 主机、Colab 实例地区或服务健康页区域当作访问者出口 |

各行实测状态：第 1、2、4、5 层的字段命中仅指 [03](03-google-matrix.md)/[04](04-nongoogle-matrix.md) 对应匿名首跳；条款版本、搜索页脚来源标签尚无本路径直接样本。第 3、4、5 层的私人属性、第 6 层设备值、第 7 层项目配置均未测且禁止补采，不属于“再登录验证”的待办。所有政策结论为 OFFICIAL_DOC，与字段存在的实测状态分开。

### 归因顺序和污染处理

1. 承认路径边界：裸 GET、空 Cookie、不保存回包 Cookie、无身份/偏好输入。不指明地址族的代理只记代理路径；不证明用户系统出口。若有认证输入则不发请求。
2. 按 `host + path + 表面` 取证。先判传输是否完成，再看登录、挑战、裸 API 错误或同意页；它们只能说明该层，不穿过验证。未见地区时，地区字段为 `null + reason`。
3. [Google Search Help](https://support.google.com/websearch/answer/179386?hl=en) 把页脚来源分成：`From your internet address` → 1；`From your device` → 6；`Based on your places (Home/Work)` → 3 中的保存地址；`Based on your past activity` → 3/历史个性化，不是国籍或付款国。这些是来源标签，不等于国家码。匿名路径中只有第一类可接受为 IP 来源；**不要求每页必出现**。被动看到其余标签，记“存在非匿名信号，结果不可用于出口判定”，不读取地点、活动或身份，不断言某人已登录；本次无查询词的 `/search` 首跳未测到标签。
4. 服务生成但可由版本选择影响的量仍标 `USER_CONTROLLED_OR_SERVICE_EDITION`。只有明确归属同一主体和量纲的证据才比较；相同请求的页脚、图标与语言不是三份独立证据。
5. S1 历史报告在同一出口观察到 Books 为 CHN，其他表面有 USA；本轮 Books 有公开标题却没命中旧内联模式，Gemini 则命中。两次不是同路径的同步重演。记录两个子域的证据与差异，不替 Books 填 USA，也不把一次缺字段断成永久改版。

### 各产品的资格边界（官方文档 OFFICIAL_DOC；账号资格一律未测）

- [条款](https://policies.google.com/terms)区分未登录按使用地点与登录后的关联；[FAQ](https://policies.google.com/faq)解释账号关联。不是每个服务都暴露条款版本。[Play 帮助](https://support.google.com/googleplay/answer/7431675)将位置、当地付款方式和账号档案分开；[YouTube 设置](https://support.google.com/youtube/answer/87604)允许选内容地区，无法判定时还可能回退美国；[Premium 政策](https://support.google.com/youtube/answer/6307365)另有会员可用地与旅行要求。
- [Gemini Web](https://support.google.com/gemini/answer/13575153) 与[移动版](https://support.google.com/gemini/answer/14579026)分别列条件；应用安装地区、设备和账号条件不能用 Web 首页证明。[AI Studio/API](https://ai.google.dev/gemini-api/docs/available-regions)列服务覆盖并另引年龄要求，Colab 还取决于实例地区。无 key 不调用推理；模型入口的 403 不证明项目权限、额度或地区。NotebookLM 的[帮助页](https://support.google.com/notebooklm/answer/16164461)现用 Gemini Notebook 名称，年龄/账号要求与 [Google AI 方案](https://one.google.com/about/google-ai-plans/)的订阅权益分别展示。
- 个人 Gmail 的登录首页不同于组织的 Workspace 许可、管理员开关和[教育版](https://edu.google.com/workspace-for-education/editions/overview/)的学校配置；同一个登录跳转不能证明任何一种权限。[Workspace 数据区域](https://support.google.com/a/answer/9223653)、[云端点与部署位置](https://cloud.google.com/vertex-ai/generative-ai/docs/learn/locations)指存储/处理地点，不指人。
- [Custom Search 文档](https://developers.google.com/custom-search/v1/reference/rest/v1/cse/list)证明该 API 的 `gl/hl` 是输入，不是 Google News 私有协议说明。News 的 `ceid=US:en` 是本轮一条跳转的 DIRECT_ANONYMOUS_OBSERVATION，不是通用格式承诺。登录/OAuth/验证码/共享静态资源没有匿名身份或购买证明。

**归因层 ≠ 呈现层。** 例如 Drive 首跳 302：`transport`=收到响应；`service_observed`=转向登录域、地区未知；`risk_signal`=无可归因证据（不是“无风险”）；`local_policy`=保护、不阻断。四个视角均不能反推出七层中缺失的值。历史证据为 DESIGN-OPT §1/5（内部设计记录，不随仓库公开） 转述 S1；单例归纳标 INFERENCE，不作为默认配置。
