# 08 GUI 是核心报告的视图，不是第二个判定器

阶段 1 · 2026-11-21（2026-11-22 修正）。设计依据：[阶段 0 接口研究](../research/07-interfaces.md)、UI 参考说明与八张参考图目录（内部设计资料，不随仓库公开）。参考矩阵为 desktop/mobile × dark/light × normal/minimal；这里约定信息与交互，不声称做过像素比对。

**移动端是正式需求（非可选）**：8 张参考图中 4 张为移动端（1024×1536）。GUI 须按 desktop / mobile 双形态设计；当前实现的是响应式布局（窗口缩窄切单列），原生 Android / Web 构建见 `docs/cross-platform.md`。原"不承诺移动端构建"表述为笔误，已删除。

## 页面与操作

| 区域 | 桌面 normal | 窄屏 / minimal |
|---|---|---|
| 左：目标树 | 按 family 和矩阵分组，展示登记、在线准入、保护三种独立标记；提供 simple、google-all、full、自选/离线按钮 | 目标列表折叠为选择面板，保留选中数、获准数和不可测原因。minimal 不改变 profile 或请求预算 |
| 中：运行、时间线、覆盖 | 始终可见 run ID、路径/族、取消、实际请求数；覆盖/未覆盖并列，失败也算已尝试；把明确异常与不确定分开计数 | 按运行摘要→两栏内容→目标详情单列堆叠；两栏可用分区/页签切换，但标题同时写两组计数。无准入提示“选中 107 / 获准 0 / 未发请求”，不显示完成 107 个检查 |
| 右：证据与解释 | 依次展开 transport、service_observed、risk_signal、local_policy；显示来源、观察时刻、可信度、字段新鲜度、关键证据、冲突双方/缺失原因和脱敏路径 | 点击目标进入详情，可返回同一 run；minimal 折叠细节，但仍显示状态文字、冲突数量/首个原因、查看全部的入口。筛选只影响列表，固定摘要列全量冲突与 UNKNOWN，能一键清除筛选 |
| 辅助视图 | Guard 预览列影响域/服务/共享路径、原因及回滚；报告导出；fixture 演示标“离线、模拟请求”，解释历史观察的边界 | 主题和密度不改变数据。不能把预览或离线演示画成“防护已生效”；不给登录/绕过/应用规则按钮 |

暗/亮主题沿用相同层级与状态词；限制、冲突、未知、受保护分别有文字/符号，不以红绿作唯一编码。屏幕变窄时先移出辅助操作，不隐藏取消、覆盖总数或风险摘要。目标重测创建新 run；不会覆盖历史证据或根据用户选择自动切路径。

## 事件订阅：增量呈现、核心终态对账

| core 事件 | GUI 的动作（无判定计算） |
|---|---|
| `RunStarted` | 建立与 run_id 对应的视图，冻结目录快照与选择，区分在线/fixture |
| `PreflightReady` | 显示预算、获准/跳过表、请求路径与不可见项；无许可时展示原因，不请求“试一次” |
| `TargetStarted` | 给目标增加进行中标记和路径；动画不代表已取得 HTTP |
| `EvidenceObserved` | 按 evidence.id 追加脱敏证据；显示信号量纲、来源与日期，不尝试从文本找国家 |
| `TargetFinished` | 直接显示 Result 的 state、reason、duration_ms、实际/模拟请求数及冲突；不自行将 200 标 OK |
| `CoverageUpdated` | 用 core **整份快照替换**两栏；不把事件条数当覆盖目标数 |
| `GuardPlanReady` | 展示闭包、主/次原因、`applied=false` 和限制，与 JSON 中同一个 plan_id 对照 |
| `Error` | 区分 scope、recoverable 和脱敏原因。目标出错不自动停别的目标 |
| `RunFinished` | 定格最终 Report、退出码、覆盖与终止原因；方可基于这份终态导出 |
| `RunCancelled` | 定格部分结果和未启动项，停动画；不得强置“全部完成”或吞掉已知异常 |

按 `(run_id,seq)` 消费/去重，发现缺序请求门面重放或恢复至带序号的快照。切主题、调整密度、切换目标/返回详情均只读本地状态。后台并发不应让另一个运行的结果串入当前选项；显示压力大可合并重绘，不得合并或跳过判定事件。

## 给 agy 的契约验收（未来离线，不需要联网）

同一份内嵌 fixture 及固定时钟，分别输入 CLI `--json/--jsonl`、agent `check` 的异步 record、GUI 的 reducer/replay。去掉仅属传输层的命令 ID 后，比对 `run_id/seq/event/payload`；按 `target` 对照最终 `state/reason/conflicts/evidence.id/duration_ms/request_count`、覆盖两栏、退出码及 Guard 理由。若各自启动了独立运行，仅规范化生成的 run_id/墙钟，不容许规范化掉业务字段。

1. Drive 首跳 302 到登录域：三端显示收到响应、地区 null、登录交接 UNKNOWN；不是账号正常。
2. 确定拒绝与同层平票共存：显示 BLOCKED 与 CONFLICT 两项、平票无赢家、运行码 2；切到 minimal、暗色或过滤后仍可看到全局冲突提示。
3. 中途取消：未启动项有原因，已开始失败项留在覆盖栏、真实联网计数在 fixture 下为 0。重复终结事件不会重复计数。
4. Play 下载/Flights 两个 Guard 例：三端均拒绝应用、列相同反向依赖链；只读目录/解释/导出不触发网络。窄屏与宽屏八种主题/密度组合，只允许排版差异。

与阶段 3 GUI 契约的字段对齐（原开工交接为本地内部资料）：事件的 `ts` 即 UTC 时间，`is_fixture` 标记来源；单目标的 `transport.attempts` 有连接/首包分段，终态墙钟和严重度直接读 `payload.report.wall_duration_ms/exit_code`。仅由 core 提供已脱敏的投影；离线演示通过 `check(fixture=…)` 运行同一事件总线，不增加第十种 agent 操作。八张参考图已审阅，当前目录中的拷贝不是像素验收依据。
