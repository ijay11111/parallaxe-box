# 打包、多平台构建与单文件资产说明（Packaging）

> 本篇说明 Parallaxe box 的六平台构建矩阵、完整 GUI 与纯无头（`--no-default-features`）双构建形态、单文件 `include_str!` 资产内嵌与外部覆盖层机制、发行白名单与 SHA-256 校验和规范。  
> 对应配置：`Cargo.toml`、`crates/cli/Cargo.toml`、`crates/core/src/targets.rs`。

---

## 1. 两种构建形态（共享同一核心）

`crates/cli/Cargo.toml` 通过 Cargo feature 精确控制是否编入图形界面：

```toml
[features]
default = ["gui"]
gui = ["dep:parallaxe-gui"]
distribution-release = ["parallaxe-core/distribution-release", "parallaxe-agent/distribution-release", "parallaxe-gui?/distribution-release"]
```

> `distribution-release` 是**编译期分发标记**，不等于 Cargo 的 `--release` profile。普通 `cargo build` / `cargo build --release` 都不带该 feature，保持源码零发包；只有正式发行流水线显式传 `--features distribution-release`。

| 形态 | 构建命令 | 包含能力 | 典型适用场景 |
|---|---|---|---|
| **完整单文件（默认）** | `cargo build --release -p parallaxe-cli` | 原生 `egui/eframe` GUI + CLI + `agent --stdio` + 8 份内嵌 TOML 资产 | 桌面日常排障（Windows / macOS / Linux 桌面） |
| **纯无头单文件** | `cargo build --release -p parallaxe-cli --no-default-features` | 仅 CLI + `agent --stdio` + 8 份内嵌 TOML 资产（**零图形库、零 `epaint_default_fonts`、零窗口依赖**） | 服务器、容器、CI/CD、SSH 终端、纯 Agent 自动化 |

> 注：纯无头构建下若用户运行 `parallaxe gui`，程序会输出明确中文提示并返回退出码 `1`，不会崩溃。

### Release 编译优化配置（`Cargo.toml`）

```toml
[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
```

TLS 底层固定使用 `rustls`（`reqwest` 已禁用默认特性，不依赖系统 OpenSSL），大幅降低跨平台链接复杂度。

---

## 2. 六平台构建矩阵与交叉编译说明

目标平台矩阵（六个交叉编译 target + Web / Android）：

| 目标三元组（Target Triple） | 平台 | 无头版（`--no-default-features`） | 完整 GUI 版（默认 `gui`） | 链接与环境注意事项 |
|---|---|---|---|---|
| `x86_64-pc-windows-msvc` | Windows x64 | ✅ 推荐静态 CRT | ✅ 推荐静态 CRT | 可配 `RUSTFLAGS="-C target-feature=+crt-static"` 消除 VCRUNTIME 动态依赖 |
| `aarch64-pc-windows-msvc` | Windows ARM64 | ✅ 推荐静态 CRT | ✅ 推荐静态 CRT | 需 MSVC ARM64 工具链（`rustup target add aarch64-pc-windows-msvc`） |
| `x86_64-apple-darwin` | macOS Intel | ✅ | ✅ | 需 Xcode Command Line Tools；可在 macOS ARM64 上跨编 |
| `aarch64-apple-darwin` | macOS Apple Silicon | ✅ | ✅ | 需 Xcode Command Line Tools；公证/签名按需由发布者配置 |
| `x86_64-unknown-linux-musl` | Linux x64 | ✅ **全静态单文件** | ⚠️ 需注意系统图形栈（见下） | 无头版直接 `musl` 全静态零外部 `.so` 依赖 |
| `aarch64-unknown-linux-musl` | Linux ARM64 | ✅ **全静态单文件** | ⚠️ 需注意系统图形栈（见下） | 交叉编译需 `aarch64-linux-musl-gcc` 或 `cross` / `zigbuild` |

### 关于 Linux `musl` 与 GUI 版本的诚实工程说明
- **纯无头版（`--no-default-features`）**：不依赖 OpenGL / X11 / Wayland，在 `x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl` 上可干净编出**完全静态链接的零依赖单文件**。
- **完整 GUI 版（默认 `gui` 特性）**：`eframe` / `glutin` / `winit` 在 Linux 桌面运行时需要访问 X11 / Wayland 显示服务与 OpenGL/EGL（`libGL` / `libEGL` / `libxkbcommon` 等系统共享库），纯 `musl` 全静态往往无法动态加载 GPU 驱动。因此在 Linux 桌面分发完整 GUI 二进制时，建议构建 `*-unknown-linux-gnu` 动态链接目标，或明确将 `*-unknown-linux-musl` 定位为**纯无头单文件发行产物**。
- **CI 诚实原则**：若某构建环境缺少特定交叉编译链接器或 SDK，应在工作流中明确声明并标注跳过原因，绝不谎报交叉编译已通过。

### 常用构建命令参考

```bash
# Windows MSVC 静态 CRT（PowerShell）
$env:RUSTFLAGS = "-C target-feature=+crt-static"
# 正式发行产物：显式带 distribution-release
cargo build --release --locked -p parallaxe-cli --target x86_64-pc-windows-msvc --features distribution-release
cargo build --release --locked -p parallaxe-cli --no-default-features --target x86_64-pc-windows-msvc --features distribution-release

# Linux musl 纯无头全静态
cargo build --release --locked -p parallaxe-cli --no-default-features --target x86_64-unknown-linux-musl --features distribution-release

# macOS（Intel / Apple Silicon）
cargo build --release --locked -p parallaxe-cli --target aarch64-apple-darwin --features distribution-release
cargo build --release --locked -p parallaxe-cli --target x86_64-apple-darwin --features distribution-release

# Web / Android
trunk build --offline --locked --features distribution-release
cargo ndk -t x86_64 -t arm64-v8a build -p parallaxe-gui --offline --locked -j 4 --features distribution-release
```

---

## 3. 单文件 `include_str!` 资产内嵌与外部覆盖层

### 3.1 编译期内嵌的 9 份只读资产

`crates/core/src/targets.rs`、`i18n.rs` 与 `eligibility.rs` 在编译期将以下 8 份文件直接嵌入二进制，确保用户把单个 `parallaxe` 可执行文件复制到任意空目录即可完整运行（离线自检、目录查询、Fixture 演练、Guard 闭包预览、链路体检、资格提示与中英语言包全部可用）：

1. `assets/targets.toml`（107 Google 目标 + 28 对照目标 + 空许可表 `admission.grants = []`）
2. `assets/signal-rules.toml`（安全停步 `[detection]` + 12 条信号规则）
3. `assets/profiles.toml`（硬上限 `[limits]` + 6 个内置 Profile）
4. `assets/dependencies.toml`（保护种子 + CDN 分类 + 4 条已验证显式边）
5. `assets/priorities.example.toml`（仅格式参考，`example_only = true`，不作为用户状态自动加载）
6. `assets/eligibility-notes.toml`（静态资格说明：访问可达 ≠ 订阅可用；不参与判定）
7. `assets/i18n/zh-CN.toml`（默认中文语言包）
8. `assets/i18n/en-US.toml`（英文语言包；法律文本暂为待确认 stub）
9. `assets/bundled/default-rules-v2.zip`（107 项「我的授权版」默认规则包；安装启动即可见，无需手动导入）

### 3.2 外部覆盖层目录用法与防提权规则

当需要扩展自定义只读目录、自定义 Profile 预算或加载用户优先级时，可通过三个全局参数显式指定本地路径（程序**绝不自动扫描**家目录、环境变量、浏览器配置或代理软件目录）：

```bash
parallaxe \
  --targets-dir ./overlay/targets \
  --profiles-dir ./overlay/profiles \
  --config ./overlay/priorities.toml \
  targets list
```

**外部覆盖层安全红线（在网络 I/O 前强制校验，违规退出码 `1`）**：
- `--targets-dir` 下的 `.toml` 按文件名稳定排序加载；**严禁包含 `[admission]` 许可块**，不得放宽预算或取消已有目标的 `protected = true`；
- `--profiles-dir` 仅能在 `HARD_LIMITS`（并发 ≤ 6、单主机 ≤ 2、逻辑 GET = 1、自动跟跳 = 0、响应体 ≤ 16 MiB）以内收紧参数；
- `--config` 仅接受 `example_only = false` 的真实优先级文件，拒绝内置示例文件；
- 运行期采用启动时一次加载的不可变快照，**不支持运行中热切换配置**。

---

## 4. 发行白名单、归档结构与校验和

### 4.1 发行包白名单与严禁入包清单

**允许进入源码发行 / 二进制归档的内容（白名单）**：
- 二进制产物：`parallaxe` / `parallaxe.exe`（分别区分完整版与 `-headless` 无头版）
- 根目录法律与说明文档：`README.md`、`QUICKSTART.md`、`FAQ.md`、`ARCHITECTURE.md`、`THIRD_PARTY_NOTICES.md`、`THIRD_PARTY_PLUGINS.md`
- 体系化交付文档：`docs/`（12 篇核心文档 + `README.md` + `user-import.md` + `plugin-protocol.md` + `release-checklist.md` + `packaging.md`）
- 插件示例：`plugins/samples/`（仅 `manifest.json` 与 `README.md`；不启用时不加载、不执行，可整目录删除）
- 源码发行额外包含：`Cargo.toml`、`Cargo.lock`、`crates/`、`assets/`（含第一方应用图标 `assets/icons/`）、`packaging/android/app/src/main/res/`、`tests/fixtures/`

**绝对禁止进入任何发行归档的内容（黑名单）**：
- `sessions/`（原始历史实战会话记录）
- `refs/archives/`（原始输入压缩包）
- `refs/design-ui/`（开发期设计参考仓库，含 GPL-3.0 与无 LICENSE 第三方源码）
- `refs/ui-mockups/` 与 `refs/screenshots/`（内部评审截图，视需单独评估权利后决定，默认不随二进制分发）
- `.workbuddy/`（内部协作记忆）与 `collab/`（内部开发过程交接单与任务卡）
- `target/`、用户本地 `--state-dir` 状态文件、任何测试临时输出

### 4.2 建议归档命名与 SHA-256 校验和

```text
parallaxe-box-v0.1.0-beta.1-x86_64-pc-windows-msvc.zip
parallaxe-box-v0.1.0-beta.1-x86_64-pc-windows-msvc-headless.zip
parallaxe-box-v0.1.0-beta.1-aarch64-pc-windows-msvc.zip
parallaxe-box-v0.1.0-beta.1-x86_64-apple-darwin.tar.gz
parallaxe-box-v0.1.0-beta.1-aarch64-apple-darwin.tar.gz
parallaxe-box-v0.1.0-beta.1-x86_64-unknown-linux-musl-headless.tar.gz
parallaxe-box-v0.1.0-beta.1-aarch64-unknown-linux-musl-headless.tar.gz
parallaxe-box-v0.1.0-beta.1-web.zip
parallaxe-box-v0.1.0-beta.1-android-x86_64.apk
parallaxe-box-v0.1.0-beta.1-android-arm64-v8a.apk
SHA256SUMS.txt
```

> 版本口径：归档名与 Release 标签用 `v0.1.0-beta.1`；二进制内部 `--version` 仍为 `0.1.0`
> （workspace 版本未改，报告 schema `parallaxe.report/v1` 不变），两者不矛盾，见 `CHANGELOG.md`。
> 发布归档由构建者放在仓库外（如 `_release/`），不以文件名形式进入 git（`.gitignore` 已排除 `*.exe`、`dist/`、`*.plugin`）。

生成校验和示例：

```powershell
# Windows PowerShell
Get-FileHash -Algorithm SHA256 parallaxe-box-v0.1.0-*.zip |
  ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path $_.Path -Leaf) } |
  Out-File -Encoding ascii SHA256SUMS.txt
```

```bash
# Linux / macOS
sha256sum parallaxe-box-v0.1.0-*.tar.gz > SHA256SUMS.txt
# macOS 如无 sha256sum 可用：
shasum -a 256 parallaxe-box-v0.1.0-*.tar.gz > SHA256SUMS.txt
```

### 4.3 发行前归档烟雾自检

每个发行归档解压到全新空白目录后，至少在断网环境下验证：
1. `parallaxe --version` 正常输出版本号；
2. `parallaxe selftest --offline` 输出 `28 passed / 0 failed`，退出码 `0`；
3. 源码构建：`parallaxe check --headless --profile google-all --json` 输出选中 `107`、发包 `0`、退出码 `3`；发行构建：改用 `check --headless --plan --profile google-all --json`，断言 `target_count=107`、`zero_packet=true`；
4. `parallaxe check --headless --fixture gemini-positive --json` 输出 `OK`、退出码 `0`、`network_request_count = 0`；
5. 归档内包含 `README.md`、`THIRD_PARTY_NOTICES.md` 与 `assets/bundled/default-rules-v2.zip`，且不含任何黑名单目录或真实公网 IP / 凭据；
6. `python tools/verify_release.py --release-dir <发行目录> --expect distribution` 校验所有二进制成员带 `parallaxe.distribution-release/v1`，并复核 `SHA256SUMS.txt`。
