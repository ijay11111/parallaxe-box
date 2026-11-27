# 示例插件（第一方，本地目录）

本目录是 `parallaxe.plugin/v1` 协议的两个**第一方示例**，用来做协议自测、CLI 演示与界面形态参考。它们不是第三方插件，也不随宿主自动加载：用户不显式启用时，宿主完全看不到它们。

| 目录 | 形态 | 演示什么 |
|---|---|---|
| `light-report-badge/` | 轻插件 | 覆盖计数徽标、画像名改写、开始/完成/出错三处文案 |
| `heavy-report-guide/` | 重插件 | 自带一个只读页面，覆盖协议允许的全部 9 种展示组件 |

## 一个插件就是一个 `manifest.json`

- 目录里**只能有** `manifest.json`（加载器会拒绝多余文件、符号链接和超过 64 KiB 的清单）；
- 没有 JS、没有动态库、没有外部进程；协议里没有任何文件系统或网络调用；
- 文本只是**展示建议**：不写 `state`/`reason`/`exit_code`，不改变九态判定；
- `author`/`license`/`source` 三个字段必填，缺一个就拒绝加载（fail-closed），并且会出现在公开声明 `THIRD_PARTY_PLUGINS.md` 里。

## 本地试跑（不会发任何网络请求）

```text
# 1) 校验清单并拿到 sha256
parallaxe plugin verify plugins/samples/light-report-badge

# 2) 用上一步的 sha256 显式批准启用（写入显式指定的设置文件）
parallaxe plugin enable plugins/samples/light-report-badge --settings %LOCALAPPDATA%\parallaxe\settings.jsonl --approve-sha256 <上一步输出的 sha256>

# 3) 查看 / 关闭
parallaxe plugin list --settings %LOCALAPPDATA%\parallaxe\settings.jsonl
parallaxe plugin disable sample.light.report-badge --settings %LOCALAPPDATA%\parallaxe\settings.jsonl
```

`manifest.json` 任意一个字节变化都会让已批准的摘要失效，加载时直接拒绝——这是有意设计，不是缓存问题。要升级插件就重新 `verify` 再 `enable`。

## 打包成分发形态（可选）

```text
python tools/pack_plugin.py pack plugins/samples/heavy-report-guide --out-dir dist
python tools/pack_plugin.py verify dist/sample.heavy.report-guide.plugin
python tools/pack_plugin.py unpack dist/sample.heavy.report-guide.plugin --out-dir plugins/local
```

`.plugin` 是只含 `manifest.json` 的存储型（不压缩）ZIP，固定时间戳，因此同一目录两次打包字节完全相同；`.sha256` 覆盖的是 `manifest.json` 的裸字节（与加载器的信任边界一致，而不是整个压缩包）。宿主只读目录，所以 `.plugin` 必须先 `unpack` 才能启用。

## 已知限制（阶段 8 如实记录）

- **插件文本不参与 i18n**：`manifest.json` 里的文案是插件作者写死的单一语言，切换界面语言不会翻译它。重插件页面在英文模式下会显示中文示例文案。
- **文本上限按 UTF-8 字节计**：`name` 128 字节、组件正文 4096 字节。中文一个字 3 字节，所以 `name` 实际约 42 个汉字，超限报的是通用的 `invalid plugin declaration`，定位不直观。
- **表单与按钮是静态描述**：本协议版本不接收输入、不回调宿主，宿主可以渲染成禁用态。

## 红线（示例同样遵守）

1. 不读 Cookie/凭据/浏览器配置，不代替用户发请求；
2. 不参与判定，失败只影响插件自身；
3. 不新增页面以外的入口，移动端不出现插件界面；
4. 新增任何插件都要在 `THIRD_PARTY_PLUGINS.md` 登记真实来源与 sha256，并由 `tools/verify_plugins.py` 校验通过。
