# 进阶：用户自主阻断（Android / iOS 系统层）

> 这是**第三备选**，仅供进阶用户自行操作。本软件界面不显示此选项，也**不代为执行**任何系统级配置。
> 适用人群：愿意自行在设备网络设置中配置 DNS/过滤器的用户。普通用户请使用主方案（规则导入 / Win-Linux 系统阻断）。

## 前提与边界
- 本软件只做**只读诊断 + 规则/域名清单导出**；以下操作全部由你在自己的设备上完成。
- 不保证任何"解锁/绕过"效果；账号国家、付款档案、订阅资格不在本软件能力范围内（见 `eligibility-notice`）。
- 操作前请备份原有 DNS/代理配置；不确定时请咨询设备/网络管理员。

## Android（Android 9+，无需 root）
**方式 A：私人 DNS（Private DNS / DNS over TLS）**
1. 设置 → 网络和互联网 → 私人 DNS（不同品牌路径略有差异）。
2. 选"指定私人 DNS 提供商主机名"，填入你信任的过滤 DNS 服务 hostname（如你自建或第三方提供的 DoT 地址）。
3. 在对应 DNS 服务的控制台导入本软件导出的 `domainlist`（`guard export --format domainlist --out <dir>`）。
4. 保存后验证：访问一个被列入清单的域名，应无法加载；访问其他网站正常。
- 注意：私人 DNS 按域名过滤，**不影响 IP 直连**；部分 App 使用自有 DoH 会绕过。
- 不占用 Android 的单一 VPN 槽位，可与你在用的代理共存。

**方式 B：在你已有的代理客户端里导入规则**
- 使用 `guard export --format clash|singbox --mode provider` 导出的文件，按 `docs/cross-platform.md` 的 Merge/Script 说明导入。
- 本软件不建 VpnService、不申请 VPN 权限。

## iOS
**方式 A：私人 DNS / 加密 DNS 配置**
- iOS 支持配置 DoH/DoT 描述文件或使用支持加密 DNS 的 App；在对应服务中导入本软件导出的 `domainlist`。
- 具体步骤随 iOS 版本与服务商变化，请以官方说明为准。

**方式 B：Safari 内容拦截器（仅浏览器内）**
- 在支持自定义规则的 Safari 内容拦截器中导入域名规则；仅对 Safari 生效，不影响其他 App。

## 导出清单从这里来
```bash
parallaxe guard export --target <id> --format domainlist --out ./export
```
导出清单仅含 FQDN 域名，不含 IP / ASN / Cookie / 账号信息，默认脱敏。

## 常见问题
- **导入后部分网站打不开**：检查是否误把共享 CDN / 登录 / 付款域名加入清单——本软件默认保护这些域，不要手动绕过保护名单。
- **没有效果**：确认 DNS 配置已生效、App 未使用自有 DoH；或更换过滤服务。
- **想恢复**：删除对应的私人 DNS 配置 / 规则集即可，本软件未在设备上留下任何状态。

## 相关文档
- 主方案与备选层级：`docs/cross-platform.md`
- 阻断产品决策：`docs/decisions.md`
- Android 单 VPN 限制与 Private DNS 方案：见 [跨平台说明](cross-platform.md)。
