# 04 目录注册与在线准入清点

阶段 1 · 2026-11-21。目录从 [03 Google 逐址矩阵](../research/03-google-matrix.md) 转录，不重跑网络。交付：[targets.toml](../../assets/targets.toml)。编号保持历史 `google.*`，不是按今天的产品名重编。表面、读取对象、失效条件、保护和验证状态均随条目保留。

| Google 矩阵组 | 注册 URL | 可在目录查到的代表表面 |
|---|---:|---|
| A 登录链 | 14 | 身份入口、OAuth、GSI、验证资源，受保护 |
| B 内容/AI/教育/生产力 | 49 | 首页、Books、Gemini、News、Drive 等，各自读取，不共用国家结论 |
| C 媒体/商店 | 13 | 首页 GL、Premium 营销页、Play 的入口与应用目录分别登记 |
| D 云/开发者 | 18 | 文档/控制台可达不等于项目权限或资源区域 |
| E 静态/API | 13 | 状态、内容类型、TLS 错误或 204；不读 CDN 国家作服务国家 |
| **合计** | **107** | **77 个不同入口主机、12 个 family；104 个既有 HTTP 首跳，3 个无 HTTP** |

`google.ggpht`、`google.ytimg`、`google.video` 的 NO_HTTP 仍占目录位置；`FIELD_UNVERIFIED` 有 HTTP，但不把它当字段已证。`google.notebook` 仍请求旧登记地址 `https://notebooklm.google.com/`（如果未来获准），301 的 `notebook.google.com` 只在 `observed_redirect_host` 展示；**未增加第 108 个目标，也不跟随跳转**。`google.play-detail` 只保留研究表里的公开常量应用 ID；不接受用户输入的查询。`google.home` 禁用旧选择器，`google.books` 不继承 Gemini 样本。

独立的 `controls` 表按 [04 对照矩阵](../research/04-nongoogle-matrix.md) 登记 28 个实际 URL，业务面与状态/帮助面不可混用；它们不是 Google 组。`google-all` 选中 107，`full` 选中 135。**本版在线批准数 0、默认可发请求数 0**；受信许可表为空。例：全量在线 check 的覆盖为 0、选中未覆盖 135（策略原因），网络请求 0，退出 3；目录列举和离线 selftest 不由此判失败。`--target google.youtube` 不能绕过。链路补测也不能在无许可时作为隐式替代。

复核者可在内存解析 TOML，以稳定 ID 对原矩阵做双向差集；除检查两个差集为空，还要逐 ID 比较完整 URL、family、A–E、源表面、失效条件、保护与验证状态映射，并检查引用的 reader 存在。仅核总数不足以发现串行/换址。新增目标先登记、注明历史证据及缺口，再单独走 [准入审查](03-schemas.md)；不改历史样本、不借同域的许可。
