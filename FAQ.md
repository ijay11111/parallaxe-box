# Parallaxe box 常见问题（FAQ）

> 用大白话解释 Parallaxe box 的设计取舍、常见现象与安全边界。

---

## 1. 为什么它不叫「流媒体解锁脚本」或「IP 纯净度打分器」？

因为这两类概念在工程上经常误导人：
- 所谓「IP 纯净度 95 分」是把几个黑盒第三方数据库拍脑袋加权出来的伪精确数字，根本回答不了「为什么我的 Gemini 打不开、但 YouTube 正常、Google Books 又跳到了别处」。
- 「解锁」往往暗示绕过平台地区/付费/风控限制，既违背目标服务条款，也与只读诊断的定位冲突。

Parallaxe box 的定位很明确：**诊断出口地区判定，辅助选择线路；逐服务展示匿名证据，不选择或切换线路。**

---

## 2. 为什么我运行 `parallaxe check --headless --profile google-all` 显示选中 107 个、发包 0、退出码 3？

这是**正常且刻意设计的安全行为**。
- 内置目录完整登记了 107 个 Google 服务表面与 28 个对照目标（共 135 项），但发行包中 `admission.grants` 默认为空、所有目标默认 `allow_online = false`。
- 匿名 HTTP GET 依然属于对第三方站点的自动化访问；在未完成逐址条款复核与授权登记前，软件不会越权向任何第三方站点发包。
- 因此 `google-all` 会诚实汇报：`selected_count = 107`，`covered = 0`，`network_request_count = 0`，并在 `uncovered` 栏逐项写明未准入原因；因为选中目标未能完成观测，按 ADR-003 返回退出码 `3`（存在未完成/不确定项），绝不伪造为绿色 `0`。

如果你想离线体验完整的探测与判定流程，可以使用内置脱敏案例：
```bash
parallaxe check --headless --fixture gemini-positive --json
```

---

## 3. 收到 HTTP 200 为什么不是绿色 `OK`，而是 `REACHABLE_ONLY` 或 `UNKNOWN`？

因为 **「网页能打开（HTTP 200）」不等于「这个服务把你的出口判到了某国」，更不等于「你的账号能用」**。
- 例如 `www.google.com/generate_204`、`gstatic.com`、公开文档页或无地区信号的普通页面，收到 200/204 只能证明网络连通，因此状态为 **`REACHABLE_ONLY`**（仅连通）。
- 例如跳转到 `accounts.google.com` 的登录页、遇到验证码/同意页、或者预期地区字段缺失，软件无法证明归属地区，因此诚实标为 **`UNKNOWN`** 并给出具体原因（如 `REDIRECT_TO_LOGIN`、`FIELD_MISSING`、`RATE_LIMITED`）。
- 只有当观测证据满足所测主张（例如提取到明确且无冲突的目标信号）时，才会给出 **`OK`**。

---

## 4. 为什么同一个出口，Google 不同服务的地区表现会不一样？

Google 并没有一个全站统一的「出口国家」。不同产品依据不同的地区层次工作（详见 [docs/google-region-model.md](docs/google-region-model.md)）：
- 搜索、首页、Gemini Web 看到的可能是未登录位置或内联页面信号；
- Google News 跳转里的 `hl / gl / ceid` 和 YouTube 的 `GL` 属于**公开版次 / 用户可控参数（`PREFERENCE` 层）**，不是独立的出口投票；
- Google Play 商店页面、Play 账号国家、付款资料国家是三件不同的事；
- Google Cloud / Vertex AI 的 region 是**云资源部署区域**，不是你的出口国家；
- 共享 Anycast CDN 和出口 IP 历史画像也会导致不同服务给出不同反馈。

所以 Parallaxe box **绝不强行把所有 Google 服务合成一个「总国家」**，而是把分歧如实展示出来。

---

## 5. 为什么不能检测我的 Google 账号国家、Play 付款国家或 Gemini 会员资格？

因为 **账号隔离是本产品的硬性架构边界**（见 [docs/account-isolation.md](docs/account-isolation.md)）：
- 账号关联国家、付款资料国家、订阅资格、年龄与 Workspace 组织策略都属于**登录态私人属性**，匿名请求在技术上不可能获取，且本软件严格禁止读取浏览器配置、Cookie、OAuth Token、API Key 或引导登录。
- 在所有报告的 7 层归因字段中，`ACCOUNT`、`PAYMENTS` 固定标为 `NOT_OBSERVABLE_ANONYMOUSLY`，`DEVICE` 固定标为 `NOT_COLLECTED`。
- 若未来需要账号侧诊断，必须作为完全独立的项目与权限边界另行设计，不会混入本工具。

---

## 6. 为什么 Guard 预览总是提示 `safe_to_apply = false`、`NO_SAFE_BLOCK` 或 `BLOCK_DEFERRED_DEPENDENCY_CONFLICT`？

这是为了**防止误伤你的核心服务（尤其是登录、付款和 Google Play 应用下载）**。
- Google 大量服务复用相同主机名与共享 CDN（例如 `www.google.com` 同时承载搜索、Flights、Travel、Finance；`dl.google.com` 同时承载 Play 下载与 Android 更新；`gstatic.com` / `googleusercontent.com` / `gvt1.com` 承载全站静态资源与分发）。
- 域名级阻断无法在不解密 HTTPS（TLS 中间人）的情况下按路径拆分同一主机下的不同业务，而本软件绝不做 TLS 中间人解密，也禁止按 IP/ASN 粗暴封禁。
- 此外，如果目录中某主机缺乏「专属性证据（证明不会影响其他服务）」，保守引擎就会返回 `NO_SAFE_BLOCK`；命中共享 CDN 返回 `GUARD_DEFERRED_SHARED_CDN`；命中共享主机或关键依赖返回 `BLOCK_DEFERRED_DEPENDENCY_CONFLICT`。
- 这不是程序报错，而是工具在诚实告诉你：**这里不存在零副作用的阻断方案**。

---

## 7. 为什么没有 `--apply`？软件会修改我的 hosts、防火墙或 Clash/Mihomo 配置吗？

**不会。当前版本 Guard 为纯只读预览，零写盘能力。**
- 所有 Guard 计划恒为 `operation = "PREVIEW"`、`applied = false`、`rules_written = 0`。
- `parallaxe guard rollback` 恒输出 `NOTHING_APPLIED`（无可回滚改动）。
- 关于 hosts、Windows 防火墙和 loopback API，阶段 4 已完成独立的写操作生命周期安全评估（10 条门控 G0–G9，见 [docs/guard.md](docs/guard.md)），结论是**当前版本全部不开放**。
- 即便未来某个版本经裁决允许导出规则片段，规则生成、导出、导入与生效也严格分离，且必须由用户显式确认并支持原子回滚。

---

## 8. GUI、CLI 和 `agent --stdio` 会不会出现「界面显示绿色、JSON 显示红色」？

**不会。**
- `parallaxe-core` 是唯一的判定引擎，且零依赖 `egui`/`eframe`/`clap`。
- GUI、CLI 和 Agent 全部订阅同一套核心事件流（10 种事件），GUI 的 9 态徽章采用穷举 `match`（禁止 `_ =>` 兜底，且仅 `OK` 为绿色），不包含任何独立判定逻辑。
- 项目内置三端一致性回归测试（`parity_replay` 10/10 与 `gui_cli_agent_parity`），每次验收均校验三端结果逐字段一致。

---

## 9. `--state-dir` 是做什么的？为什么默认看不到 `SIGNAL_STALE`？

`SIGNAL_STALE` 用于提示「某条页面解析规则可能因为网站改版而连续失效（连续 3 次合格完整样本缺失字段）」。
- 按照 [ADR-004](docs/decisions.md) 的「默认零写盘」红线，软件**默认不读写任何历史状态目录**。此时每次运行独立计数（N=1），单次字段缺失只报 `UNKNOWN + FIELD_MISSING`，绝不擅自创建隐藏状态文件。
- 只有当你在命令行显式传入 `--state-dir <本地目录>` 时，才会启用去标识化的规则新鲜度计数（`freshness-v1.json`，不存 IP/URL 查询/正文，超过 7 天自动失效）。

---

## 10. 链路体检里为什么 `quic` 和 `dns_leak` 是 `UNTESTED`，`port_scan` 是 `PROHIBITED`？

- **`port_scan = PROHIBITED`**：本项目明确禁止扫描本机或远端端口，因此永久标为禁止。
- **`quic = UNTESTED` / `dns_leak = UNTESTED`**：当前版本未发起专门的 QUIC/UDP 探测，也未接入受控权威解析器去构造 DNS 泄露测试；没有证据就标「未测」，绝不拿普通 HTTP/1.1 或单一系统解析结果冒充通过。
- **`dns_compare = UNSUPPORTED_CAPABILITY`**：当前构建未引入第二套独立解析器（也不调用第三方 DoH），因此诚实标注不支持多解析器对比。
- **默认零发包**：`parallaxe link explain` 默认只投影已有报告或目录，只有显式附加 `--resolve`（且非 `--fixture` 模式）才会对内置目录主机执行受控系统 DNS 查询并计入 `dns_query_count`。

---

## 11. GUI 的「自定义导入」安全吗？为什么 CLI/Agent 还没有导入命令？

- **安全防线**（详见 [docs/user-import.md](docs/user-import.md)）：
  1. 导入自定义网址前须主动勾选《导入自定义网址责任声明》；导入自定义检查方式前须勾选更严格的《导入自定义检查方式责任声明》（版本 `liability-v1`）。
  2. 严格拦截内网/回环/私网 IP、显式端口、凭据 `userinfo`、`fragment`，以及 ReDoS 危险正则；只能使用 9 种白名单 reader，禁止绑定到内置 `google.*`/`control.*` 目标，禁止写回 `assets/`。
  3. 导入后默认仍是**零发包**；真要在线检查必须点击申请并经过居中模态弹窗确认，授予的是一次性会话级许可（运行预检即消耗，用完即焚，绝不落盘），且报告强制记录 `origin = "USER_IMPORTED"` 与规则摘要 `rule_digest`。
- **CLI/Agent 范围**：本阶段裁决 **CLI 与 Agent 暂不开放自定义导入操作**（`agent --stdio` 保持既有 9 个只读/预览操作），仅在 GUI 中提供带完整弹窗确认链路的受控导入体验。

---

## 12. 我的公网 IP、浏览器 Cookie 或系统配置会被读取或上传吗？

**绝不会。**
- **无遥测、无上报**：除你显式许可的诊断目标请求外，不向任何第三方服务器发送数据。
- **零账号/浏览器访问**：代码内没有任何读取浏览器目录、Cookie 数据库、代理订阅或认证环境变量的逻辑；HTTP 客户端编译期即禁用 `cookies` 特性，每次请求显式发送空 `Cookie` 头并丢弃 `Set-Cookie`。
- **默认脱敏**：所有终端表、JSON/JSONL、HTML、CSV、GUI 视图默认不输出原始公网 IP；在文档与离线案例中仅使用 RFC 5737 / RFC 3849 保留地址段（如 `203.0.113.x`、`198.51.100.x`、`2001:db8::/32`）作为示意。

---

## 13. 发现误报、页面改版或有权利/下架问题如何反馈？

- 若某条公开规则因目标站点改版失效，请在 Issue 中提供脱敏后的规则 ID、表面说明与复现条件（**请勿粘贴含 Cookie、Authorization 头、真实公网 IP 或个人账号信息的原始报文或 HAR 文件**）。
- 若涉及目标条款、商标或权利投诉，请在仓库公开渠道或联系维护人提出，维护人核实后将按 [docs/compliance.md](docs/compliance.md) 及时修正、降级或下架相关登记条目。
