# 01 同类方案：可借的是证据组织，不是站点规则

研究日 2026-11-21。**已实测**：匿名读取下列公开仓库的 GitHub 元数据和默认分支 README。**待验证**：运行时行为；没有安装或运行任何竞品。作者说明是一手项目资料，但其服务判据只算 COMMUNITY_REPORT / LEGACY_CODE。末列为 API `pushed_at`（UTC），不是发布日期或维护承诺；许可为 API 识别值，不替代逐文件审计。

只读指不改网络、文件或持久会话；上传区分查询披露、测试流量和分享报告。未知不等于否。全表共用前提：未登录、匿名查公开资料；对仓库元数据/摘查到的源码事实可信度高，对 README 所述运行行为为中、未审为未知；默认分支、依赖或运行模式一变即需复查。任何一行都不构成在线地区信号。下表“未审”指不能依据 README 排除该行为；静态资料列仅评价读取行为。

| 项目（来源） | 范围、环境/接口 | Cookie | 切节点/订阅 | 只读？ | 上传/对外数据 | 双栈 | 借鉴、缺陷与排除 | 最近推送 · 归档 · 许可 |
|---|---|---|---|---|---|---|---|---|
| [wusuoweiju48/google-anti-cn-doctor](https://github.com/wusuoweiju48/google-anti-cn-doctor) | Python CLI；批查 Gemini | 主脚本未见 | 是，临时改全局逐节点 | 否 | 外查画像；报告未知 | 未证 | 借服务对照；抢占全局影响用户工作。 | 2026-10-16 · 未归档 · API 未识别许可 |
| [vernette/ipregion](https://github.com/vernette/ipregion) | Shell CLI/容器；地区、CDN | 是，含同意态 | 当前出口，不需订阅 | 基础检查为主；另有可选项 | 外查 IP 库；报告待核 | 是 | 保留每族结果；禁用访客 Cookie 和广告采样。 | 2026-11-19 · 未归档 · MIT |
| [Grim1313/ipregion-for-windows](https://github.com/Grim1313/ipregion-for-windows) | PowerShell CLI/JSON；地区一致性 | 部分探针有 | 不逐节点；可选代理 | 基础路径只读 | 外查；报告待核 | 是；代理单列 | 借平票不裁决；代理不能杜撰双栈。 | 2026-11-09 · 未归档 · MIT |
| [dogflydream/pick-node](https://github.com/dogflydream/pick-node) | Python CLI；批检 AI 与出口 | 未审 | 读代理 URI，逐节点独立内核 | 否，起进程写结果 | 查出口；报告未知 | 未证 | 隔离进程比切全局好，但仍不符合本项目边界。 | 2026-10-13 · 未归档 · MIT |
| [xykt/IPQuality](https://github.com/xykt/IPQuality) | Shell CLI；风险、黑名单、地区 | 未审 | 按地址/本机，无须切组 | 否，有在线报告 | 可上传生成报告，隐私模式可禁 | 是 | 借多源差异，不查邮件端口、不分享报告。 | 2026-11-19 · 未归档 · AGPL-3.0 |
| [tombcato/clash-ip-checker](https://github.com/tombcato/clash-ip-checker) | Python CLI/Web；IP 画像与导出 | 未审 | 是，切全局枚举；部署版可接订阅 | 否 | 外查 IP 评级；报告待核 | 未证 | 借分流注释；不读配置、不改组、不信单库评级。 | 2026-10-28 · 未归档 · GPL-3.0 |
| [Genuifx/NetPulse](https://github.com/Genuifx/NetPulse) | Python CLI/JSON；本机或 SSH 巡检 | 未审 | 非切组工具，无须订阅 | SSH 模式传脚本 | SSH 回传及对外查询 | 有 v6 项，非逐项双栈 | 借证据与耗时；排除 SSH、测速和风险分合成。 | 2026-10-06 · 未归档 · MIT |
| [dy0422/ipcheck-plus](https://github.com/dy0422/ipcheck-plus) | Bash CLI；接口与多站质量 | 未审 | 可选接口，不需切组 | 检测为主；未全审 | 第三方 IP/DNSBL；报告未知 | 是 | 借实际接口与挑战分类，不照搬外部依赖。 | 2026-06-14 · 未归档 · MIT |
| [lmc999/RegionRestrictionCheck](https://github.com/lmc999/RegionRestrictionCheck) | Bash CLI；跨区流媒体 | 是，含 cookie jar | 当前路径，不需订阅 | 否，可写 jar | 多站查询；报告未知 | 是 | 借覆盖分类。访客 Cookie 未必是登录态，本项目两种皆禁。 | 2025-12-08 · 未归档 · AGPL-3.0 |
| [nkeonkeo/MediaUnlockTest](https://github.com/nkeonkeo/MediaUnlockTest) | Go CLI；多服务监控 | 未审 | 默认示例不切；订阅未证 | 未审 | 多站查询；报告待核 | 是 | 借逐服务组织；未识别许可，不移植 handler。 | 2024-10-06 · 未归档 · API 未识别许可 |
| [sjlleo/netflix-verify](https://github.com/sjlleo/netflix-verify) | Go CLI；Netflix 单站 | 主入口未见，全链未审 | 当前线路，不需订阅 | 检测为主；未全审 | 访问媒体服务；报告未知 | 是 | 借双族对照；片名可达不是订阅或播放权。 | 2025-07-09 · 未归档 · GPL-3.0 |
| [dreamsxin/netutils-cli](https://github.com/dreamsxin/netutils-cli) | Rust CLI/JSON；路由、DNS、TLS | 分命令，未全审 | 只读诊断不需切组 | 非全工具只读 | 按目标发包；报告未知 | 是 | 借 route-get/dns-compare；不扫描、启浏览器或改 resolver。 | 2026-10-28 · 未归档 · MIT |
| [alexliyu7352/google_ip](https://github.com/alexliyu7352/google_ip) | Python CLI；ip-reanchor 持续访问 | 是，按出口落盘复用 | 按已有路径，非批切 | 否，写状态 | 主动送出持续行为；报告未知 | 可指定族 | 反例；不使用词库、坐标或干预行为。 | 2026-11-10 · 未归档 · API 未识别许可 |
| [vps8899/warp-google-unlock](https://github.com/vps8899/warp-google-unlock) | Linux Shell；WARP 分流 | 未审 | 是，刷新出口；非订阅工具 | 否，改网卡/路由 | 经隧道转送流量；报告未知 | 未证逐族 | 系统级副作用示例，不部署。 | 2026-09-06 · 未归档 · API 未识别许可 |
| [AnixOps/AnixOps-speedtest](https://github.com/AnixOps/AnixOps-speedtest) | Go CLI；代理批量测速 | 未审 | 是，读取配置或订阅并启动内核 | 否，测速/文件 | 可上行测试；非报告上传证据 | 未证逐族 | 借区分延迟与吞吐；不测速或处理订阅。 | 2026-11-19 · 未归档 · MIT |
| [missuo/OpenAI-Checker](https://github.com/missuo/OpenAI-Checker) | Shell CLI；单站检查 | 主脚本未见 | 不需切组或订阅 | 主脚本查询为主 | IP/平台查询；报告未知 | 是 | 已归档，仅对照输出，不采纳端点有效性。 | 2023-11-11 · 已归档 · MIT |
| [zoocoup/Vendor-Service-Hostnames-and-IPs](https://github.com/zoocoup/Vendor-Service-Hostnames-and-IPs) | 域名资料，不含探测器 | 不适用 | 否 | 是 | 否，静态 | 不适用 | 借共用域警示；未核授权不搬整库。 | 2024-03-03 · 未归档 · API 未识别许可 |
| [snapei/clash-pro-rules](https://github.com/snapei/clash-pro-rules) | 规则数据，不含探测器 | 不适用 | 清单不切 | 是 | 否，静态 | 不适用 | 核覆盖；匹配不证明所有权或依赖。 | 2026-09-06 · 未归档 · GPL-3.0 |
| [ACL4SSR/ACL4SSR](https://github.com/ACL4SSR/ACL4SSR) | 规则/配置模板 | 静态表不适用 | 读表不切，应用另计 | 仅读表 | 否，静态 | 不适用 | 交叉核名，不导入规则或订阅模板。 | 2026-11-21 · 未归档 · CC-BY-SA-4.0 |
| [blackmatrix7/ios_rule_script](https://github.com/blackmatrix7/ios_rule_script) | 规则和脚本；仅审静态表 | 静态表不适用 | 读表不切 | 仅读表 | 否，静态 | 不适用 | 不复制脚本或生成列表；多库收录不证明依赖。 | 2026-10-27 · 未归档 · GPL-2.0 |

**本项目与它们的差异化在证据模型、低噪声、安全边界、本机诊断，不是堆更多解锁站点。** 按服务、信号、路径保留冲突；不合成“纯净分”，不冒充代理客户端，不宣称能分离共享 CDN。只比较公开行为；不复制代码、选择器、片单、词库、数据或配置。AGPL/GPL、内容许可、无许可分别审核。

复核方法：以各行链接中的仓库名查询 `https://api.github.com/repos/{owner}/{repo}`。源码抽查 `vernette/ipregion:ipregion.sh`、`lmc999/RegionRestrictionCheck:check.sh`、`wusuoweiju48/google-anti-cn-doctor:scripts/check_gemini_nodes.py`，未运行、未保存 Cookie。`librarycloud/google-conf` 匿名元数据 404，不列为可访问的比较对象；替代核对见 [03](03-google-matrix.md) 和 [ADR-001](../../docs/decisions.md)。对标资料不能用作本项目的服务探针证据。
