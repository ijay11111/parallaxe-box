# Parallaxe box 体系化文档索引

> 本索引列出 **12 篇产品核心文档**，并收纳专题指南、发行核查、打包说明与研究 / 架构归档。设计取舍见 [工程决策摘要](decisions.md)。

---

## 一、产品核心文档（12 篇）

| 序号 | 文档路径 | 主题 | 对齐依据 / 源码锚点 | 状态 |
|---:|---|---|---|---|
| 1 | [google-region-model.md](google-region-model.md) | Google 七层地区归因模型与不可观测边界 | 产品契约§3.2；`research/02`；`contracts.rs::Layer` | ✅ 完整 |
| 2 | [google-coverage.md](google-coverage.md) | Google 全服务目录（107 + 28 对照）、Profile 与准入清点 | 产品契约§6.2；`architecture/04`；`assets/targets.toml`、`profiles.toml` | ✅ 完整 |
| 3 | [signals.md](signals.md) | 信号提取规则、可信度分级、失效案例与离线 Fixture 契约 | 产品契约；`research/05`、`architecture/09`；`assets/signal-rules.toml`、`signals.rs`、`fixtures.rs` | ✅ 完整 |
| 4 | [verdict.md](verdict.md) | 9-State 判定模型、分桶冲突投票与退出码归约 | 产品契约§9；[decisions.md](decisions.md)；`verdict.rs`、`reports.rs` | ✅ 完整 |
| 5 | [gui-headless.md](gui-headless.md) | GUI 与无头 CLI 使用指南及三端一致性契约 | 产品契约；`architecture/07`、`08`；`crates/cli/src/main.rs`、`crates/gui/` | ✅ 完整 |
| 6 | [agent-interface.md](agent-interface.md) | Agent `--stdio` NDJSON 控制面协议（9 op + 流式取消） | 产品契约；`architecture/02`；`crates/agent/src/lib.rs` | ✅ 完整 |
| 7 | [guard.md](guard.md) | Guard 只读影响闭包、共享 CDN 防护、审计与写操作评估 | 产品契约§17；`research/06`、`architecture/06`；`crates/core/src/guard.rs` | ✅ 完整 |
| 8 | [priorities.md](priorities.md) | 服务优先级配置、用户决策权与硬保护下限 | 产品契约§20.2；`assets/priorities.example.toml`、`targets.rs` | ✅ 完整 |
| 9 | [isolation.md](isolation.md) | 服务依赖图、路径隔离解释与边界声明 | 产品契约；`architecture/06`；`assets/dependencies.toml`、`crates/cli/src/main.rs` | ✅ 完整 |
| 10 | [account-isolation.md](account-isolation.md) | 账号隔离、匿名客户端、请求预算与 `--state-dir` 边界 | 产品契约§20.1；`architecture/05`；`probe.rs`、`state.rs` | ✅ 完整 |
| 11 | [service-observation.md](service-observation.md) | 服务观测记录与四列只读链路体检（双栈/DNS/能力声明） | 产品契约§20.4；`crates/core/src/linkcheck.rs`、`network.rs` | ✅ 完整 |
| 12 | [compliance.md](compliance.md) | 平台条款、合规判断（有条件发布）、免责与禁止用途 | 产品契约；`research/08-compliance.md` | ✅ 完整 |

---

## 二、专题指南与发行交付文档

| 文档 | 说明 |
|---|---|
| [user-import.md](user-import.md) | 自定义检查网址与检查方式导入指南（F1–F5 规范、两段责任声明 `liability-v1`、一次性会话弹窗授权） |
| [plugin-protocol.md](plugin-protocol.md) | 本地插件协议 `parallaxe.plugin/v1`：manifest 字段、唯一权限、页面组件集、CLI 启用/校验与沙箱边界 |
| [../THIRD_PARTY_PLUGINS.md](../THIRD_PARTY_PLUGINS.md) | 插件来源与许可声明：随仓库分发的插件逐项登记 author/license/source/sha256（依赖库另见 `THIRD_PARTY_NOTICES.md`） |
| [release-checklist.md](release-checklist.md) | 发行前合规 / 脱敏 / 权利清单与当前仓库实测核查记录 |
| [cross-platform.md](cross-platform.md) | 阶段 7 三端（桌面 / Android / Web）与四层阻断能力说明、未验证项与待办 |
| [packaging.md](packaging.md) | 六平台构建矩阵、单文件 `include_str!` 资产内嵌、外部覆盖层与校验和归档说明 |
| [../README.md](../README.md) | 项目首页：大白话定位、快速入口、免责声明与总导航 |
| [../CHANGELOG.md](../CHANGELOG.md) | 版本变更日志（首个测试版 `v0.1.0-beta.1`：功能、安全边界、未测项、版本口径） |
| [../QUICKSTART.md](../QUICKSTART.md) | 5 分钟快速开始与常用命令速查 |
| [../FAQ.md](../FAQ.md) | 常见问题与设计边界大白话解答 |
| [../ARCHITECTURE.md](../ARCHITECTURE.md) | 开发者架构全景说明 |
| [../THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) | 从 `Cargo.lock` 全量提取的 488 项依赖许可清单与参考库零复制合规说明 |

---

## 三、研究与架构设计归档（只读历史依据）

- **阶段 0 研究报告（`docs/research/`）**：
  - [01-related-work.md](research/01-related-work.md) 同类方案对比
  - [02-region-model.md](research/02-region-model.md) 七种地区含义研究
  - [03-google-matrix.md](research/03-google-matrix.md) Google 107 项全服务逐址矩阵
  - [04-nongoogle-matrix.md](research/04-nongoogle-matrix.md) 28 项非 Google 对照矩阵
  - [05-signals.md](research/05-signals.md) 信号可信度与失效风险
  - [06-blocking.md](research/06-blocking.md) 本机阻断方案比较
  - [07-interfaces.md](research/07-interfaces.md) 三端接口形态方案
  - [08-compliance.md](research/08-compliance.md) 合规研究与有条件发布判断
  - [09-caveats.md](research/09-caveats.md) 不可靠目标清单与证据账本
- **阶段 1 架构契约（`docs/architecture/`）**：
  - [01-modules.md](architecture/01-modules.md) 模块边界与单向依赖
  - [02-data-model.md](architecture/02-data-model.md) 核心值类型、事件流与 JSON Schema
  - [03-schemas.md](architecture/03-schemas.md) 四份 TOML 校验契约
  - [04-catalog.md](architecture/04-catalog.md) 目录注册与准入清点
  - [05-security-boundary.md](architecture/05-security-boundary.md) 账号隔离、请求预算与离线证明
  - [06-dependencies.md](architecture/06-dependencies.md) 依赖图与影响闭包算法
  - [07-cli.md](architecture/07-cli.md) CLI 命令树与退出码设计
  - [08-gui.md](architecture/08-gui.md) GUI 视图与事件消费契约
  - [09-signals-and-fixtures.md](architecture/09-signals-and-fixtures.md) 信号规则表与 Fixture 规范
