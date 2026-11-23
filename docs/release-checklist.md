# 发行核查清单（Release Checklist）

> 核查日期：2026-11-23 · 依据：[docs/compliance.md](compliance.md)、[docs/decisions.md](decisions.md)、[docs/packaging.md](packaging.md)
> 发行立场：**有条件发布（Conditional Release）**
> **签署原则**：本清单如实记录当前公开仓库的实测结果；平台条款终审、法务与最终发布决定为人工门控项，保持 `[ ]` 待维护人签字，绝不代签。

## 当前仓库事实（2026-11-23）
| 项 | 当前值 |
|---|---|
| 公开树文件数 | **153**（单提交干净树） |
| Git 历史 | 1 个公开提交（已重建，旧过程文件不在历史中） |
| 仓库 URL | `https://github.com/ijay11111/parallaxe-box`（真实，非占位） |
| 根目录许可证 | `LICENSE-MIT` + `LICENSE-APACHE` 均存在 |
| 自动化测试 | 170 passed / 0 failed |
| 离线自检 | 28 / 0 |
| 基准 | `google-all` = 107 / 0 / 3；三端 parity 10/10 |
| Cargo 依赖 | 492 个 package（见 `Cargo.lock`） |

---

## 一、人工终审门控（维护人发行当日签字）
- [ ] **平台条款终审**：复核最新版 GitHub ToS / AUP 与分发地法律、加密出口管制要求。
- [ ] **商标与权利终审**：确认未使用 Google 等商标作背书暗示；第三方素材均已排除或获授权。
- [ ] **目标准入复核**：确认未获授权目标在默认 / `google-all` / `--full` 下均不发在线请求。

## 二、准入与零发包（仓库实测）
- [x] **目录与准入**：`assets/targets.toml` 含 Google 目标 107 项 + 独立对照 28 项（合计 135）；`admission.grants = 0`、`allow_online = true` 数量 0、`safe_default = true` 数量 0。
- [x] **规则与 profile**：`assets/signal-rules.toml` 含 12 条规则（含 1 条强制禁用负例 `rule.legacy-region`）；`assets/profiles.toml` 提供 simple / google-all / full 等 profile；`assets/priorities.example.toml` 标记 `example_only = true`。
- [x] **默认零发包基线**：`parallaxe check --headless --profile google-all --json` = 选中 107 / 覆盖 0 / 网络请求 0 / 退出码 3。
- [x] **退出码语义**：0=无明确异常、1=配置错误、2=明确异常、3=不确定；2 优先于 3。

## 三、脱敏与敏感信息扫描（当前公开树）
- [x] **无真实公网地址 / 凭据**：当前 153 文件扫描未发现真实用户公网 IP、Cookie、Bearer token、私钥、API key 或订阅凭据。
- [x] **允许出现的示例值**：RFC 5737 保留段（`203.0.113.x`、`198.51.100.x`）、私网段、回环 `127.0.0.1`、测试占位 `example.invalid`——均位于合成 fixture 或拒绝输入用例，不是真实观测。
- [x] **合成 fixture**：`tests/fixtures/` 中的样本标记为 SYNTHETIC，无用户数据。
- [x] **默认不保存完整正文**：报告只保存必要元数据与选定字段。

## 四、账号隔离与只读防线
- [x] **不读账号数据**：不读浏览器 Cookie / 账号 / 订阅 / 凭据；`docs/account-isolation.md` 为安全边界证明。
- [x] **默认零写盘**：状态目录仅在显式 `--state-dir` 时启用；Guard 预览 `applied=false`、`rules_written=0`。
- [x] **系统写入受控**：Win/Linux 系统阻断需显式开启 + 确认 + 可回滚；当前目录无专用路径证据时返回 `SYSBLOCK_NOT_SAFE`，不写系统。

## 五、自定义导入合规防线
- [x] **责任声明门槛**：两段责任声明中文原文一字不改，必须主动勾选；英文界面只提示"保持中文法律原文，未机翻"。
- [x] **一次性会话许可**：用户导入目标在授权前 `network_request_count = 0`；F3 确认弹窗用完即焚。
- [x] **导入零污染**：导入不覆盖内置 assets，不写系统。

## 六、权利与许可
- [x] **双许可证**：根目录 `LICENSE-MIT`、`LICENSE-APACHE` 全文存在；`Cargo.toml` 声明 `MIT OR Apache-2.0`。
- [x] **第三方依赖**：492 个 Cargo package 记录在 `Cargo.lock`；`THIRD_PARTY_NOTICES.md` 说明权利情况；无 GPL/AGPL 依赖进入产品。
- [x] **字体许可**：内嵌 NotoSansSC 子集，附 OFL 许可与来源说明。
- [x] **开发参考库已排除**：`refs/design-ui/` 等第三方参考素材不在当前公开树与发行包中（历史开发资料，不参与编译与分发）。

## 七、发行边界（当前公开树即发行源）
- [x] **无黑名单目录**：公开树不含 `refs/design-ui/`、`refs/ui-mockups/`、`refs/screenshots/`、`collab/`、`sessions/`、`.workbuddy/`、`spec/`。
- [x] **编译产物排除**：`target/`、`tests/release-smoke/latest/`、`__pycache__` 由 `.gitignore` 排除，不上传。
- [x] **截图审计为本地可选**：UI 截图审计使用本地素材，缺素材时 SKIP，不作为公开源码门槛（见 `tests/release-smoke/README.md`）。

## 八、门禁基线（实测 2026-11-23）
- [x] `cargo fmt --all -- --check` = 0
- [x] `cargo clippy --workspace --all-targets --offline -- -D warnings` = 0
- [x] `cargo test --workspace --offline` = **170 passed / 0 failed**
- [x] `parallaxe selftest --offline --json` = **28 / 0**
- [x] `google-all` = **107 / 0 / 3**
- [x] 三端 parity = **10/10**
- [x] wasm32 core+GUI、Android NDK、Linux musl x64/arm64 交叉编译 = 0

## 九、待维护人确认（不阻塞源码公开）
- [ ] 平台条款与法务终审（第一节三项）。
- [ ] en-US 法律译文：当前英文界面保留中文责任声明原文，待法务确认是否收录正式译文。
- [ ] 若未来恢复截图审计，需建立已授权测试素材包并明确许可证。

> 本清单随仓库实际状态更新；清理或新增文件后须重新生成，不保留过期的历史数字与结论。
