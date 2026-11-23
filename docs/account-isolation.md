# 账号隔离、匿名边界与请求预算（Account Isolation）

> 本篇给出 Parallaxe box「绝对禁止接触任何账号与凭据」的架构证明、代码级可指认证据、硬请求预算表以及 `--state-dir` 最小持久化边界。  
> 源码实证：`Cargo.toml`、`crates/core/src/probe.rs`、`crates/core/src/targets.rs`、`crates/core/src/state.rs`；架构契约：[docs/architecture/05-security-boundary.md](architecture/05-security-boundary.md)。

---

## 1. 账号隔离是不可逾越的产品红线

Parallaxe box 只做出口网络路径、公开页面与匿名首跳观测。以下四项边界在架构上强制固定，无任何开关可放开：

```text
account_mode            = PROHIBITED
session_mode            = ANONYMOUS_ONLY
credential_sources      = []
browser_profile_sources = []
```

### 明确禁止读取或接受的 7 类敏感输入
1. 浏览器用户目录（Chrome / Edge / Firefox / Safari profile 等）与远程调试端口；
2. Cookie 数据库、Local Storage、Session Storage、IndexedDB；
3. Google、Microsoft、Apple 或第三方登录会话；
4. OAuth Token、API Key、`Authorization` / `Proxy-Authorization` 头、Refresh Token 及环境变量中的常见凭据；
5. Google Play、Google One、Workspace 或任何代理节点的订阅链接与凭据；
6. 付款资料、家庭/工作地址及操作系统设备定位；
7. 用户粘贴的账号导出文件、浏览器录制的 HAR 文件或带认证请求头的原始日志。

> **未来分支声明**：若未来需要「登录态账号国家或资格检测」，必须作为**完全独立的项目、独立二进制与独立权限边界**另行建设；本软件不预留任何「顺便读取账号」的隐藏入口。

---

## 2. 代码级可指认证据（可直接审计）

| 防线 | 代码位置 | 实际实现证据 |
|---|---|---|
| **编译期禁用 Cookie Store 与 OpenSSL** | `Cargo.toml` | `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "http2", "charset"] }`——未启用 `cookies` 特性，且 `Cargo.lock` 零 `cookie_store` / `openssl` / `webview` / `tauri` 依赖 |
| **每次请求独立匿名客户端** | `crates/core/src/probe.rs`（`HttpTransport::build_client`） | `.no_proxy()`（不隐式继承带认证的系统/环境变量代理）、`.redirect(Policy::none())`（禁止自动跟跳）、`.retry(never())`（禁止底层静默重试）、`.pool_max_idle_per_host(0)`（不跨请求复用连接） |
| **显式空 Cookie 头** | `crates/core/src/probe.rs`（`HttpTransport::attempt`） | `client.get(url).timeout(remaining).header(reqwest::header::COOKIE, "")`——每次请求显式写入空 `Cookie` 头 |
| **丢弃 `Set-Cookie` 与多余响应头** | `crates/core/src/probe.rs`（`HttpTransport::attempt`） | 仅提取 `status`、`Content-Type`、`Location`，其余响应头（含 `Set-Cookie`）直接丢弃，不缓存、不回送、不写入事件与日志 |
| **拒绝凭据、私网与非预期目标** | `crates/core/src/probe.rs`、`crates/core/src/targets.rs` | 拒绝含 `userinfo`（`@`）、`fragment`（`#`）、路径/查询的代理 URI；拒绝解析到回环/RFC1918/链路本地/私网地址的非测试目标；拒绝代理 URI 含认证信息 |
| **有界读取防内存滥用** | `crates/core/src/probe.rs` | 逐块读取响应体，严格截断于 `min(request.max_body_bytes, 16_777_216)`（16 MiB），超限即止并标 `truncated = true, complete = false` |
| **离线契约自检覆盖** | `crates/core/src/selftest.rs`、`crates/core/tests/zero_calls.rs` | 断网验证：空许可下传输端口调用次数为 `0`；非法凭据输入先报错、零发包且错误不回显密文；`Set-Cookie` 绝不跨请求传播 |

---

## 3. 硬请求预算（`HARD_LIMITS`，只可收紧、不可放宽）

`crates/core/src/targets.rs` 定义全项目唯一的硬预算上限，任何 CLI 参数、Profile TOML 或外部覆盖层均只能取更严值：

| 预算项 | 代码硬上限 | 说明 |
|---|---:|---|
| 全局并发槽位 `max_concurrency` | `6` | 跨全目标共享并发上限 |
| 单规范主机并发 `per_host` | `2` | 同主机不同路径共享槽位 |
| 单目标逻辑探测 `logical_gets_per_target` | `1` | 禁止 HEAD+GET 双请求，禁止重复抓同页 |
| 单目标额外尝试 `extra_attempts_per_target` | `1` | IPv6 失败/超时后的受控 IPv4 回落与可恢复传输重试共享这唯一 1 个额外槽；401/403/429/挑战永不重试 |
| 自动重定向次数 `max_redirects` | `0` | 仅取首跳，绝不自动跟跳 |
| 单响应最大字节 `max_body_bytes` | `16 MiB` (`16,777,216 B`) | 流式有界读取，超限截断 |
| IPv6 首包回落门槛 `v6_fallback_after_ms` | `8,000 ms` | 仅当能受控选择 v4 且剩余预算充足时才回落 |
| 单请求截止 `max_request_timeout_ms` | `12,000 ms` | 受整次运行剩余截止时间双重约束 |
| 整次运行截止 `max_run_timeout_ms` | `120,000 ms` | 队列等待、解析、建连、读体共用同一全局截止 |

---

## 4. 唯一可选磁盘状态：`--state-dir` 与自定义导入隔离

### 4.1 `--state-dir`（默认完全关闭）
- **默认零写盘**：不发现、不创建、不读取家目录或环境变量中的隐藏状态文件夹。
- **显式授权才写入**：仅当用户在命令行显式传入 `--state-dir <本地目录>`（且非 `--fixture`/`--selftest`、非 Agent 单条请求）时，写入去标识化的 `freshness-v1.json`，拒绝符号链接/重解析点。详见 [signals.md](signals.md#4-signal_stale-规则新鲜度机制n3-与---state-dir)。

### 4.2 自定义网址与检查方式导入（GUI F1–F5）
- 导入的自定义目标强制隔离在 `user.*` 命名空间，默认 `allow_online = false`、`protected = true`；
- 绝不回写内置 `assets/` 目录；
- 每次在线检查必须经居中模态弹窗显式确认，授予一次性会话许可（用完即焚、不落盘），报告强制标记 `origin = "USER_IMPORTED"` 与 `consent_version = "liability-v1"`。详见 [user-import.md](user-import.md)。
