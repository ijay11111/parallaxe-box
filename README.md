<p align="center">
  <img src="assets/icons/parallaxe-box.png" alt="Parallaxe box" width="88" />
</p>

# Parallaxe box

**综合网络环境检验软件（含 Google 全服务地区信号探测）· 匿名只读诊断 · 安全规则导出 · 发行包开箱即用**

[English README](README.en.md) · [更新日志](CHANGELOG.md) · [快速开始](QUICKSTART.md) · [常见问题](FAQ.md)

> 🧪 **当前版本：第一个公开测试版 `v0.1.0-beta.1`**  
> 桌面（Windows GUI/CLI/Agent）、Web（WASM）、Android（双 ABI APK）、Linux（musl 无头 CLI）同版提供。
> **测试版不含系统级阻断**：Guard 只做只读影响闭包预览与带警示的手工规则导出，绝不自动写系统配置。
> 未测项见 [CHANGELOG](CHANGELOG.md) 与 [发行前检查清单](docs/release-checklist.md)。请勿在生产环境依赖测试版结果。

> **发行立场：有条件发布（Conditional Release）**  
> 只面向自有或获明确授权的网络环境，提供**匿名、只读、证据先行**的出口地区观测与链路诊断。  
> **它不是「流媒体解锁脚本」「IP 纯净度打分器」或「代理客户端」。**
> 使用前请读 [用途边界与免责声明](#usage-boundaries) 和 [合规文档](docs/compliance.md)。

---

## 一、它解决什么问题？

切换一条跨境线路后，常见割裂现象：Google 搜索显示在美国、Gemini 提示地区不支持、Google Books 跳到另一地区、Play 应用下不动。传统脚本只测一两个页面，再给一个看似精确的「纯净度 95 分」，既解释不了服务之间的分歧，还可能因为误封共享域名把登录和 Play 下载一起弄坏。

**Parallaxe box 把这件事拉回工程事实：诊断出口地区判定，辅助选线，不替你选线、切节点。**

1. **它不是解锁脚本，而是综合网络环境检验软件（Google 全服务探测是核心专项）**——通用链路诊断（DNS / 出口 / 双栈 / 路径 / 本地 QoS）之上维护 107 个 Google 公开表面（12 服务家族）+ 28 个对照目标，逐表面独立取证。
2. **它不是 IP 风险分数聚合器，而是逐服务、逐信号、逐路径记录证据**——不给伪精确总分，默认不调用第三方评分 API，每条结论附信号来源、归因层次、可信度与新鲜度。
3. **它不是代理客户端**——不切节点、不管订阅、不改系统网络；默认零发包起步，不读浏览器配置 / Cookie / 账号凭据。
4. **它不假装能分离共享 CDN**——保护登录链、付款链、Play 下载链与共享基础设施（如 `gstatic.com`、`googleusercontent.com`），无法安全阻断时直说 `NO_SAFE_BLOCK`。

---

## 二、源码构建 vs 正式发行包（重要）

本项目刻意区分两种分发方式，默认行为不同：

| 分发来源 | 默认模式 | 内置 107 项 Google | 设置页「关于软件」 |
|---|---|---|---|
| **GitHub Releases 成品**（Windows zip/exe、Android APK、Web、Linux） | **发行包模式**（编译期 `distribution-release` 标记，门禁解除） | 安装启动即可见「我的授权版 · 107 项」，一次确认即可真实检测 | **不显示**源码零发包开关 |
| **git clone 源码自行构建** | **零发包模式**（安全基线：`allow_online=false`、`admission.grants=[]`） | 默认不发包；可在设置→关于软件经测试者确认后切换到发行包模式 | **显示**「切换到发行包模式」入口 |

- **发行包开箱即用**：默认规则包 v2（107 项）已预置在发行包内，**用户不需要自己找 zip 导入**；授权仍需显式确认（不静默发包），但拿到目标是零操作。
- **两种授权范围**：确认时可选「仅本次生效」（退出即失效）或「永久生效」（保存到本机用户状态目录，设置页可一键撤销）；都不写回内置目录、不扩散到其他 URL。
- **两个开关独立**：「发行包模式」与「AI 自主调试」不是同一个开关。发行包模式 + AI 自主开启时，AI 可完全操作（仍守 3.5 秒间隔、分批、优先 TCP、403/429 即停的 QoS 红线）。
- **control 目标不因发行模式放行**：`control.*` 仍需用户导入 + F3 授权；`full` profile 仍排除 `user.*`。
- 用 `parallaxe build-info --json` 可查看当前二进制的分发标记（`parallaxe.distribution-release/v1` 或 `parallaxe.source-build/v1`）。

---

## 三、能做什么 / 不能做什么

### 能力

| 能力 | 说明 |
|---|---|
| **Google 全服务矩阵** | 107 个 Google 公开入口（身份、搜索、内容、Workspace、媒体、Play、AI、教育、云、开发者、静态资源、API）+ 28 个对照目标，逐表面取证；simple 快速档 16 项（含 Google One），默认不限时。检测范围只保留三档：简单 16 / Google 全服务 107 / 全部 135。 |
| **诚实 UA 与逐网址跳转配置** | **软件本体**全站统一诚实研究型 UA，**不伪造浏览器身份/TLS/JA3 指纹**、不带 Cookie、不登录。少数公开页（如 books 的 csi）只对浏览器 UA 返回完整内容——这类"需要伪装 UA 才能读"的信号**不属于本体能力**，规划由**插件/外部扩展**承担（默认关闭、需用户显式启用并自行判断合规）；当前插件协议为声明式、无脚本执行，尚未开放传输层钩子，故本体取不到时如实标 `UNKNOWN`。每个目标可在注册行声明 `max_redirects`（默认 0 不跟跳；需要读取登录展示页地区的目标显式设为 1–2），只跟 HTTPS、跳转链记入报告。Google 各子域独立判区（如 books=CN、youtube/news=US、Play 页脚地区）并列展示，不跨服务多数表决。 |
| **七层地区归因** | 区分出口网络、服务地区策略、账号关联、付款/Play 档案、用户偏好、设备位置、云资源区域；不把页面语言或 CDN 机房冒充出口国家。 |
| **9 态诚实判定** | `OK` / `RESTRICTED` / `BLOCKED` / `REACHABLE_ONLY` / `LOCAL_PATH_ERROR` / `CONFLICT` / `UNKNOWN` / `SIGNAL_STALE` / `UNSUPPORTED`；证据不足直说 `UNKNOWN`，同层矛盾直说 `CONFLICT`。 |
| **三端同核** | 桌面 GUI（egui/eframe）、无头 CLI（`parallaxe`）、NDJSON Agent（`parallaxe agent --stdio`）共享 `parallaxe-core` 事件流，零判定分叉。 |
| **AI 启动检测** | Agent 协议 v2，路径 A（F3 一键授权）/ 路径 B（opt-in 自主检测）；授权桥为临时目录短生命周期文件，无监听端口、带 TTL；报告一律脱敏。 |
| **四列只读链路体检** | `parallaxe link explain` 分开呈现 resolver / 连接边缘 / HTTP 出口 / 地址族；默认零发包，严格禁止端口扫描。 |
| **Guard 影响闭包预览** | 本地依赖图反查共享 CDN 冲突，生成只读审计；`applied=false`、`rules_written=0`，绝不擅自改本机配置。 |
| **自定义网址导入（GUI）** | 导入自有或获授权网址（`user.*`）与白名单检查方式；两段责任声明、默认零发包、运行前一次性会话许可（用完即焚）、报告审计留痕。 |
| **离线自检与演练** | 28 项离线自检 + 9 个脱敏 Fixture，断网即可验证判定逻辑与三端一致性。 |
| **Web / Android / Linux** | 同一核心编译为浏览器页面与双 ABI APK；Android 仅 `INTERNET` 权限、无插件面；Linux 提供 musl 无头 CLI 归档。 |
| **默认脱敏、零账号接触** | 禁用 Cookie Store、发送空 `Cookie` 头、丢弃 `Set-Cookie`；不读浏览器目录 / Token；默认隐藏 IP，可导出 JSON/JSONL/HTML/CSV/MD/TXT。 |
| **五种代理路径模式** | 设置→高级网络可选「自动（默认，探测环境/系统代理/常见端口并逐 host 选择）/ 不代理 / HTTP / SOCKS / PAC 网址」；自动模式有界探测、不扫网段、不改系统代理/路由/DNS；PAC 仅用最小语法子集并如实注明未覆盖调用；当前构建不支持 SOCKS 传输时明确报错而非静默直连。 |

### 硬边界（明确不能做）

- **不检测账号侧信息**：账号关联国家、Play 付款国家、订阅资格、年龄/组织策略、设备定位一律 `NOT_OBSERVABLE_ANONYMOUSLY` / `NOT_COLLECTED`。
- **不做解锁 / 绕过 / 洗白**：不破解验证码、不伪造 TLS/JA3 指纹、不做浏览器自动化、不重放登录。
- **不充当代理或修改网络**：不遍历订阅、不切节点、不测速、不扫端口、不改系统代理/DNS/hosts/防火墙。
- **测试版不含系统级阻断**：只提供 Guard 只读预览与规则手工导出（`applied=false`）。
- **源码默认空许可**：git clone 普通构建零发包；只有发行包或显式切换 + 确认才解除内置门禁。

---

## 四、5 分钟上手

详细命令见 [QUICKSTART.md](QUICKSTART.md)。需 Rust `1.78+`。

```bash
# 源码构建（默认零发包）
cargo build --release -p parallaxe-cli

# 纯无头构建（服务器 / CI / 容器）
cargo build --release -p parallaxe-cli --no-default-features

# 正式发行包构建（仅发布流水线使用）
cargo build --release -p parallaxe-cli --features distribution-release

# 离线自检（预期 28 passed / 0 failed）
parallaxe selftest --offline

# 查看分发标记
parallaxe build-info --json

# 目录 / 解释 / 离线演练
parallaxe targets list
parallaxe explain --target google.gemini
parallaxe check --headless --fixture gemini-positive --json

# 快速档 16 项（源码构建默认零发包，退出码 3）
parallaxe check --headless --profile simple --json

# GUI 与 Agent
parallaxe gui
parallaxe agent --stdio
```

产物位于 `target/release/parallaxe`（Windows 为 `.exe`）。直接下载成品请到 GitHub Releases——安装启动即可用，无需自行构建。

---

## 五、文档导航

**根目录**：[CHANGELOG](CHANGELOG.md) · [QUICKSTART](QUICKSTART.md) · [FAQ](FAQ.md) · [ARCHITECTURE](ARCHITECTURE.md) · [第三方许可](THIRD_PARTY_NOTICES.md) · [第三方插件](THIRD_PARTY_PLUGINS.md)

**`docs/` 专题**（索引见 [docs/README.md](docs/README.md)）：
- 核心模型：[七层地区模型](docs/google-region-model.md) · [全服务覆盖](docs/google-coverage.md) · [信号与 Fixture](docs/signals.md) · [9 态与退出码](docs/verdict.md)
- 接口与观测：[GUI/CLI 指南](docs/gui-headless.md) · [Agent 协议](docs/agent-interface.md) · [服务观测](docs/service-observation.md) · [自定义导入](docs/user-import.md)
- 安全：[Guard 与共享 CDN](docs/guard.md) · [依赖图与路径隔离](docs/isolation.md) · [账号隔离与请求预算](docs/account-isolation.md)
- 合规与发布：[平台条款与合规](docs/compliance.md) · [发行前检查](docs/release-checklist.md) · [打包与多平台](docs/packaging.md)

---

<a id="usage-boundaries"></a>

## 六、用途边界与免责声明

### 适用场景
正当的跨境商务与远程办公、留学与学术交流、开发者访问公开文档与 API、跨地区协作，以及自有或受托管理网络的故障排查。

### 免责声明
> 本软件仅用于自有或获授权网络的只读诊断。输出描述特定时间、服务与路径上的匿名观测，不代表实际所在地、账号国家、购买资格、IP 信誉或未来可用性。本机规则预览不改变远端服务判定。使用者须遵守适用法律、组织政策及目标服务条款；不获准的目标请勿探测。作者与所列服务商（包括但不限于 Google、Cloudflare、Netflix、OpenAI、Anthropic 等）无隶属、赞助或背书关系。本声明不排除依法不得排除的责任。

### 禁止用途
不得用于：侵犯版权、绕过付费墙/订阅、破解 DRM；欺诈、冒充、批量注册、刷量、骚扰；规避制裁或出口管制；未授权访问或对第三方服务超范围抓取、压测、滥用；任何违反目标服务条款的行为。

### 开源许可
原创代码采用 **MIT OR Apache-2.0** 双重许可。用途边界声明为项目使用规范与免责说明，不构成对标准 OSI 许可证条款的单方面缩减；商业化分发或跨司法辖区部署请由发布者自行合规复核。
