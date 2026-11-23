# 08 发布判断：有条件发布

查阅日期：2026-11-21。**结论：有条件发布**，指自行编写、离线也能工作的匿名诊断工具可以准备 GitHub 发行；不是 GitHub 预批准，不是对 107 个目标的自动访问许可，更不是对任一司法辖区的法律意见。当前只做研究，没有取得站点书面许可，也没有实际发布。

适用场景：正当的跨境商务与远程办公、留学与学术交流、开发者访问文档与 API、跨地区协作、自有或明确受托管理的网络排障。README/首屏建议写：“诊断出口地区判定，辅助选择线路；本工具逐服务展示匿名证据，不选择或切换线路。”不宣传“解锁/解除限制”、伪装地区、洗白 IP、恢复账号或保证订阅可用。

## 平台、目标服务、当地法律是三道不同的门

下表全部为官方政策（OFFICIAL_DOC），判断是对**本项目范围**的解释，不是条款原文。[GitHub ToS](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service) 所读版本标注生效于 2026-04-27；[GitHub AUP](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) 为查阅日页面。发行前再查，分发地、服务版本或用途改变即重审。

| 条款定位 | 对本项目的约束 / 发布证据 |
|---|---|
| [ToS C；AUP §1](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#c-acceptable-use) | 使用 GitHub 须遵守适用法、版权/商标、制裁与出口管制；不设计规避功能。另查[GitHub 与贸易管制](https://docs.github.com/en/site-policy/other-site-policies/github-and-trade-controls)。可开源不等于可在任何地区以任何方式经营 |
| [ToS D.1–3、F](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#d-user-generated-content)；[AUP §3](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies#3-intellectual-property-authenticity-and-private-information) | 上传者对内容及权利负责；平台可以移除内容。逐文件确认代码/素材/数据权利。不附借来的脚本、完整响应、账户信息或私有配置，不冒充厂商 |
| [AUP §2、§4–5](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies#4-spam-and-inauthentic-activity-on-github) | 禁欺诈、骚扰、虚假账号、滥用自动化、未授权访问或干扰。低频匿名观察和本机规则预览本身不是这些行为；不能用 GitHub Actions 作出口池或用工具压测、代测第三方账户 |
| [ToS H](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#h-api-terms)；[AUP §7–9](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies#7-information-usage-restrictions) | 仓库元数据研究限公开非个人资料，研究成果按该政策开放访问；遵守 API、隐私与带宽限制。不抓用户列表、不收私人日志、不滥用托管流量。GitHub 对其数据的规则不能外推到被测站点 |

**目标方单独准入**：匿名 GET 仍是自动访问；200、公开网页、robots 未显式禁止或用户勾选均不等于授权。记录“主机/具体路径、用途、适用条款章节、机器可读限制、查阅日、许可范围及复核人”；无依据则仅保留目录/授权的替代资料/离线样本，`full` 也不得越过。

- [Google 条款「不要滥用我们的服务」](https://policies.google.com/terms?hl=en-US)涉及保护措施和机器可读限制；[Search 的 machine-generated traffic 政策](https://developers.google.com/search/docs/essentials/spam-policies#machine-generated-traffic)明确针对未经明确许可的自动查询。故不自动提交搜索词、抓排名或用结果数复测 `num`。Google 旗下各服务需另查专属条款。
- [YouTube 条款，Permissions and Restrictions 第 2–3 项](https://www.youtube.com/static?gl=US&template=terms)：自动访问有特定例外；本工具不是公共搜索引擎。未取得适用事前书面许可，首页/GL、Music、Premium 只留历史摘录或合成 fixture，不上线抓页、不测试会员。
- [Netflix 英文使用条款 §1.8(iii)、(iv)、(vii)](https://help.netflix.com/legal/termsofuse?locale=en) 限制规避保护、自动访问和数据提取；不抓片库、不播放、不试 DRM。04 的条款页只作法律资料。AI、商店、银行等未逐址核准的一律不发产品探测；状态/帮助页若另有允许，也只证明那个页面，不能代替业务探测。
- 本机 REJECT 预览、hosts 或防火墙管理应只针对用户有管理权的设备，只停止本机流量；生成不等于应用，不影响他人网络、不操纵远端服务、登录/付款/CDN 受保护。P0 不执行写操作。管理本机不取得访问远端的权利。

同类仓库佐证仅限“有人公开托管过这类软件”：[RegionRestrictionCheck](https://github.com/lmc999/RegionRestrictionCheck)、[IPQuality](https://github.com/xykt/IPQuality) 的提交历史，以及 [clash-ip-checker 的发布记录](https://github.com/tombcato/clash-ip-checker/releases) 均可查。它们的 Cookie、外部评分/报告或切换代理组等行为不被本项目继承；仓库长期存在既不是平台批准，也不证明其历史行为、再分发或本地使用合法。

## 发布文案与许可证

**可直接使用的免责声明：** 本软件仅用于自有或获授权网络的只读诊断。输出描述特定时间、服务与路径上的匿名观测，不代表实际所在地、账号国家、购买资格、IP 信誉或未来可用性。本机规则预览不会改变远端服务的判定。使用者须遵守适用法律、组织政策及目标服务条款；不获准的目标请勿探测。作者与所列服务商无隶属或背书关系。本声明不排除依法不得排除的责任。

**产品 EULA / 托管服务使用政策的禁止用途清单：** 不得侵权、绕过付费/订阅/DRM/年龄或地区资格、欺诈冒充、批量注册/刷量/骚扰、规避制裁或出口管制、未授权访问或过量抓取/压测；不得违反适用法律或目标条款。落实在产品边界：不接收或查找凭据、订阅、浏览器数据；不伪造地区/TLS 指纹，不破验证码，不重放登录，不切节点；仅对准入公开表面做有预算的 GET，挑战即停，默认无遥测、无第三方风险查询。

许可证建议：对权属清楚的**原创代码**优先考虑 [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0)（明确专利授权、注意修改与 NOTICE 义务）；[MIT](https://opensource.org/license/mit) 更简短，保留版权/许可声明；[AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.html) 的强互惠和特定网络交互下的对应源代码要求，不能按 MIT 条件混入。仓库可见不代表可复制；逐文件查授权，无明确许可不引入。EULA/README 的禁用说明**不能缩减已按标准开源许可证授出的权利**（参见[开源定义第 6 条](https://opensource.org/osd)）；若要强制限制代码可用于哪些行业/用途，须另行评估分发模式，不可同时宣称未经修改的 OSI 开源授权。

## 发行前检查（本轮不代签）

- [ ] 维护人复核 GitHub 条款、目标逐址许可、robots 和地区适用版本；不明项目默认及自选在线模式均关闭。
- [ ] 对源码、依赖、规则表、字体/图标、UI 图、摘录和第三方 NOTICE 建权利清单；无权再分发的一律不进包。
- [ ] 仅从发行白名单打包；逐项扫描文件、历史、压缩包、截图、fixture、报告和发行附件中的 Cookie、令牌、订阅、地址、标识符。原始 `sessions/`、`refs/archives/`、私有协作记忆不直接发布；若曾泄露先停止发布并按所有者流程处置。
- [ ] 离线验证默认零写、无浏览器/账号访问、无遥测，挑战停、超预算停；GUI/CLI/agent 不能越权；只看设计不算测试通过。
- [ ] 核查自定义导入功能（`docs/user-import.md`）：发行包与内置 `assets/` 不得随包携带任何用户导入的网址、规则或状态目录数据；两段强制责任声明原文（`liability-v1`）须在导入界面显式展示并经用户主动勾选；文档与演示示例仅使用 RFC 2606 / RFC 5737 保留域名与合成地址，不得包含任何真实公网 IP、个人域名或凭据。
- [ ] 审 README、首屏、商标和演示文案，提供误报、权利投诉及下架渠道；分发地、加密/出口管制、隐私义务由发布者核查。**商业化分发建议找专业人士复核**，尤其是跨司法辖区部署、强制 EULA 或第三方数据使用。
