# Parallaxe box

**综合网络环境检验软件（含 Google 全服务地区信号探测）· 只读链路诊断 · 安全规则导出**

> 🚧 **开发状态：开发中（In Development）**  
> 核心判定与离线测试已通过，但**本软件仍在开发中，尚未发布稳定版本**。当前阶段为阶段 7（三端适配 + 网络取证安全 + 阻断能力）；Web 真机探测、Android APK/真机验证、系统阻断真实写入等仍未完成。请勿在生产环境依赖当前结果，以实际验收状态为准。

> **发行立场：有条件发布（Conditional Release）**  
> 本软件仅面向自有或获明确授权的网络环境提供**匿名、只读、证据先行**的出口地区观测与链路诊断。  
> **它不是「流媒体解锁脚本」，不是「IP 纯净度打分器」，也不是「代理客户端」。**  
> 使用前请阅读下方 [用途边界与免责声明](#用途边界与免责声明) 及 [合规文档](docs/compliance.md)。

---

## 一、用大白话说：它是什么？

当你切换一条跨境网络线路，可能会遇到「Google 搜索显示在美国、Gemini 提示地区不支持、Google Books 跳到另一个地区、Google Play 应用又下不动」的割裂现象。传统脚本往往只测一两个网页、给你打一个看似精确的「纯净度 95 分」或贴一个「解锁成功」标签，结果既解释不了为什么各服务打架，还可能因为瞎封共享域名把 Google Play 下载和登录一起弄坏。

**Parallaxe box** 把这件事拉回工程事实：**诊断出口地区判定，辅助选择线路；本工具逐服务展示匿名证据，不选择或切换线路。**

### 核心定位（四句话说清差异）

1. **它不是流媒体解锁脚本，而是综合网络环境检验软件（Google 全服务探测是核心专项）**——通用链路诊断（DNS/出口/双栈/路径/本地 QoS）之上，维护 107 个 Google 公开表面（12 服务家族）+ 28 个对照目标，逐表面独立取证，绝不把一个服务的结论套给另一个服务。
2. **它不是 IP 风险分数聚合器，而是逐服务、逐信号、逐路径记录证据**——不给出伪精确总分，默认不调用第三方 IP 风险评分 API，每条结论都附带信号来源、归因层次、可信度与新鲜度。
3. **它不是代理客户端，不切换节点、不管理订阅、不改变系统网络**——默认只读、零发包起步、绝不读取浏览器配置/Cookie/账号凭据，运行前后系统路由、代理、hosts、防火墙完全不变。
4. **它不会假装解决共享 CDN 的不可分离问题**——明确保护登录链、付款链、Play 下载链与共享基础设施（如 `gstatic.com`、`googleusercontent.com`、`dl.google.com`），无法安全阻断时直说 `NO_SAFE_BLOCK` 或 `BLOCK_DEFERRED_DEPENDENCY_CONFLICT`。

---

## 二、它能做什么 / 不能做什么？

### 能做什么

| 能力 | 大白话说明 |
|---|---|
| **Google 全服务矩阵覆盖** | 内置 107 个 Google 公开入口（身份登录、搜索、内容、Workspace、媒体、Play 商店、AI、教育、云、开发者、静态资源、API）及 28 个对照目标（AI/流媒体/社交开发/国内对照/CDN）。 |
| **七层地区归因** | 严格区分「出口网络、服务地区策略、账号关联、付款/Play 档案、用户偏好/公开版次、设备位置、云资源区域」，绝不把页面语言或 CDN 机房冒充出口国家。 |
| **9-State 诚实判定** | 仅使用 9 个具名状态：`OK`、`RESTRICTED`、`BLOCKED`、`REACHABLE_ONLY`、`LOCAL_PATH_ERROR`、`CONFLICT`、`UNKNOWN`、`SIGNAL_STALE`、`UNSUPPORTED`。证据不足直说 `UNKNOWN`，同层矛盾直说 `CONFLICT`。 |
| **GUI / CLI / Agent 三端一致** | 原生桌面 GUI（`egui/eframe`）、无头 CLI（`parallaxe`）与标准输入输出控制面（`parallaxe agent --stdio`）共享同一 `parallaxe-core` 核心与事件流，三端零判定分叉。 |
| **四列只读链路体检** | 通过 `parallaxe link explain` 与 GUI 处置中心，将 `resolver`、实际连接边缘、HTTP 出口、地址族四列分开呈现；默认零发包，显式 `--resolve` 才执行受控系统 DNS 查询，严格禁止端口扫描。 |
| **Guard 只读影响闭包预览** | 基于本地依赖图反查候选目标的反向依赖与共享 CDN 冲突，计算影响闭包并生成只读审计（`GuardAudit`）。当前版本恒为 `applied=false`、`rules_written=0`，绝不擅自改写本机配置。 |
| **自定义网址与检查方式导入（GUI）** | 支持用户导入自有或获授权的检查网址（`user.*`）及 9 类白名单检查方式；强制两段责任声明勾选门槛、默认零发包、运行前一次性会话级弹窗授权（用完即焚）并在报告中审计留痕。 |
| **离线自检与 Fixture 演练** | 内置 28 项离线自检（`parallaxe selftest --offline`）与 8 个脱敏合成离线案例（`--fixture`），断网即可验证全部判定逻辑与三端一致性。 |
| **默认脱敏与零账号接触** | 代码层面禁用 Cookie Store、发送空 `Cookie` 头、丢弃 `Set-Cookie`；不读浏览器目录/环境变量 Token；默认脱敏隐藏具体 IP，报告可离线导出为 JSON/JSONL/HTML/CSV。 |

### 明确不能做什么（硬边界）

- **不检测账号侧信息**：不能也不允许读取 Google 账号关联国家、Play 付款国家、订阅资格、年龄/组织策略或设备定位（一律显式标为 `NOT_OBSERVABLE_ANONYMOUSLY` / `NOT_COLLECTED`）。
- **不做「解锁」「绕过」「洗白」**：不破解验证码、不伪造 TLS/JA3 指纹、不做浏览器自动化、不自动重放登录、不承诺规避任何平台风控或恢复账号资格。
- **不充当代理或网络修改工具**：不批量遍历订阅、不切换节点、不测速跑分、不扫描端口、不自动修改系统代理、DNS、hosts 或防火墙。
- **默认空许可（零隐蔽发包）**：内置目录的 `admission.grants` 默认为空、所有内置目标默认 `allow_online = false`。未获明确逐址授权时，即使运行 `google-all` 或 `--full`，也只输出目录覆盖与未准入原因，真实网络请求数严格为 `0`。

---

## 三、5 分钟快速上手

> 详细命令行操作、参数说明与输出示例请参阅 [QUICKSTART.md](QUICKSTART.md)。

### 1. 构建二进制

需要 Rust `1.78+`（2021 edition）：

```bash
# 默认构建（含原生 GUI + CLI + Agent 单文件二进制）
cargo build --release -p parallaxe-cli

# 纯无头构建（不含 GUI 与图形库依赖，适合服务器 / CI / 容器）
cargo build --release -p parallaxe-cli --no-default-features
```

编译产物位于 `target/release/parallaxe`（Windows 为 `target/release/parallaxe.exe`）。

### 2. 离线自检（无需联网）

```bash
# 运行 28 项内置离线契约自检（预期：28 passed / 0 failed，退出码 0）
parallaxe selftest --offline
# 或使用快捷参数
parallaxe --selftest
```

### 3. 离线 Fixture 演练与目录查看

```bash
# 列出内置 Google 全服务目录（107 项，完整目录含 28 项对照共 135 项）
parallaxe targets list

# 解释单个目标的表面、保护状态、准入状态与绑定规则
parallaxe explain --target google.gemini

# 使用内置脱敏合成案例离线演练核心判定（不发包）
parallaxe check --headless --fixture gemini-positive --json
parallaxe check --headless --fixture conflict-tie --json

# 查看默认空许可下的 google-all 行为（选中 107 / 覆盖 0 / 发包 0 / 退出码 3）
parallaxe check --headless --profile google-all --json

# 只读 Guard 影响闭包预览与回滚状态检查（不写盘）
parallaxe guard plan --dry-run --target google.notebook --json
parallaxe guard rollback --json

# 四列只读链路体检（基于离线案例投影，零发包）
parallaxe link explain --fixture gemini-positive --json
```

### 4. 启动 GUI 与 Agent 控制面

```bash
# 启动原生桌面 GUI（仅本机窗口，不监听任何网络端口）
parallaxe gui

# 启动标准输入输出 NDJSON Agent 控制面（支持 9 种安全操作，无网络监听）
parallaxe agent --stdio
```

---

## 四、文档导航

### 根目录交付文档
- **[快速开始指南](QUICKSTART.md)**：安装构建、常用命令、离线演练、退出码速查与外部覆盖层用法。
- **[常见问题（FAQ）](FAQ.md)**：大白话解答「为什么发包 0」「为什么不是解锁脚本」「为什么 Guard 拒绝应用」等高频疑问。
- **[架构说明](ARCHITECTURE.md)**：4 Crate 工作区结构、共享核心事件流、数据契约、资产内嵌与 ADR 决策总览。
- **[第三方许可声明](THIRD_PARTY_NOTICES.md)**：从 `Cargo.lock` 完整提取的 488 项依赖许可证清单与参考库合规说明。

### `docs/` 体系化专题文档（完整索引见 [docs/README.md](docs/README.md)）
- **核心模型与目录**：[Google 七层地区模型](docs/google-region-model.md) · [Google 全服务覆盖](docs/google-coverage.md) · [信号规则与 Fixture](docs/signals.md) · [9-State 判定与退出码](docs/verdict.md)
- **三端接口与观测**：[GUI 与无头 CLI 指南](docs/gui-headless.md) · [Agent Stdio 协议](docs/agent-interface.md) · [服务观测与链路体检](docs/service-observation.md) · [自定义导入指南](docs/user-import.md)
- **安全与隔离**：[Guard 与共享 CDN 防护](docs/guard.md) · [服务优先级与用户决策](docs/priorities.md) · [依赖图与路径隔离](docs/isolation.md) · [账号隔离与请求预算](docs/account-isolation.md)
- **合规、核查与发布**：[平台条款与合规判断](docs/compliance.md) · [发行前检查清单](docs/release-checklist.md) · [打包与多平台构建说明](docs/packaging.md)

### 研究与架构设计归档
- 阶段 0 研究报告：[docs/research/](docs/research/01-related-work.md)（01–09）
- 阶段 1 架构契约：[docs/architecture/](docs/architecture/01-modules.md)（01–09）

---

## 五、用途边界与免责声明

### 适用场景
正当的跨境商务与远程办公、留学与学术交流、开发者访问公开文档与 API、跨地区协作，以及自有或明确受托管理的网络故障排查。

### 免责声明
> 本软件仅用于自有或获授权网络的只读诊断。输出描述特定时间、服务与路径上的匿名观测，不代表实际所在地、账号国家、购买资格、IP 信誉或未来可用性。本机规则预览不会改变远端服务的判定。使用者须遵守适用法律、组织政策及目标服务条款；不获准的目标请勿探测。作者与所列服务商（包括但不限于 Google、Cloudflare、Netflix、OpenAI、Anthropic 等）无隶属、赞助或背书关系。本声明不排除依法不得排除的责任。

### 禁止用途
使用本软件（及其衍生部署或托管服务）时，不得用于：
- 侵犯版权、绕过付费墙/订阅限制、破解 DRM 或技术保护措施；
- 欺诈、冒充、批量注册、刷量、垃圾信息或骚扰；
- 规避制裁、出口管制或违反适用法律法规；
- 未授权访问或对第三方服务实施超出正常范围的抓取、压测或滥用；
- 任何违反目标服务条款的行为。

### 开源许可
本项目原创代码采用 **MIT OR Apache-2.0** 双重许可（详见 `Cargo.toml` 及 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)）。请注意：上述用途边界声明为项目使用规范与免责说明，不构成对标准 OSI 开源许可证条款本身的单方面缩减；商业化分发或跨司法辖区部署请由发布者自行进行合规与法律复核。
