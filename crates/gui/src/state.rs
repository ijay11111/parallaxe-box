//! 纯事件驱动状态机（`AppState::apply(&mut self, event: &Event)`）。
//!
//! 铁律：
//! - 零 `egui` / 零网络 / 零判定计算：所有 state、冲突、覆盖、退出码与 Guard 理由原样投影自 `parallaxe-core`。
//! - 按 `(run_id, seq)` 去重并检测缺口；终结事件（`RunFinished` / `RunCancelled`）后不再接纳迟到事件。
//! - 全局统计（冲突数、明确异常数、不确定数、覆盖数）永远基于全量数据计算，不受 UI 筛选/搜索/密度/主题影响。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use parallaxe_core::contracts::{
    Coverage, Event, EventName, Evidence, GuardPlan, Mode, Path, Report, ResultRecord, State,
    Termination,
};
use parallaxe_core::i18n::Locale;
use serde_json::Value;

/// 预检中因准入或策略被跳过的目标项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    pub target: String,
    pub reason: String,
}

/// 作用域内错误记录（原样保存脱敏信息）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunErrorRecord {
    pub seq: u64,
    pub ts: String,
    pub scope: String,
    pub code: String,
    pub message_safe: String,
    pub recoverable: bool,
    pub target: Option<String>,
}

/// 时间线条目（只读投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub seq: u64,
    pub ts: String,
    pub event: EventName,
    pub target: Option<String>,
    pub summary: String,
}

/// 单次运行终结形态（`RunFinished` 与 `RunCancelled` 互斥）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    Finished,
    Cancelled,
}

/// 9 态精确计数（按 `State` 枚举原样累计，不含伪精确总分）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateCounts {
    pub ok: usize,
    pub restricted: usize,
    pub blocked: usize,
    pub reachable_only: usize,
    pub local_path_error: usize,
    pub conflict: usize,
    pub unknown: usize,
    pub signal_stale: usize,
    pub unsupported: usize,
}

impl StateCounts {
    /// 穷举累加（禁止 `_ =>`）。
    pub fn record(&mut self, state: State) {
        match state {
            State::Ok => self.ok += 1,
            State::Restricted => self.restricted += 1,
            State::Blocked => self.blocked += 1,
            State::ReachableOnly => self.reachable_only += 1,
            State::LocalPathError => self.local_path_error += 1,
            State::Conflict => self.conflict += 1,
            State::Unknown => self.unknown += 1,
            State::SignalStale => self.signal_stale += 1,
            State::Unsupported => self.unsupported += 1,
        }
    }

    /// 明确异常总数（对应退出码 2 类：`RESTRICTED + BLOCKED + LOCAL_PATH_ERROR`）。
    pub fn definite_anomalies(&self) -> usize {
        self.restricted + self.blocked + self.local_path_error
    }

    /// 不确定总数（对应退出码 3 类：`UNKNOWN + CONFLICT + SIGNAL_STALE + UNSUPPORTED`）。
    pub fn uncertain_total(&self) -> usize {
        self.unknown + self.conflict + self.signal_stale + self.unsupported
    }
}

/// 单次运行的完整只读视图模型。
#[derive(Debug, Clone)]
pub struct RunView {
    pub run_id: String,
    pub is_fixture: bool,
    pub mode_label: String,
    pub mode: Option<Mode>,
    pub profile: Option<String>,
    pub catalog_revision: String,
    pub selected_ids: Vec<String>,
    pub admitted_ids: Vec<String>,
    pub skipped: Vec<SkippedEntry>,
    pub budget: Option<Value>,
    pub preflight_path: Option<Path>,
    pub preflight_ready: bool,
    pub running: bool,
    pub terminal: Option<TerminalKind>,
    pub in_progress_targets: BTreeSet<String>,
    pub live_evidence: BTreeMap<String, Vec<Evidence>>,
    /// 目录/提交序的结果表（按 target ID 索引，保持顺序）。
    pub result_order: Vec<String>,
    pub results_by_target: HashMap<String, ResultRecord>,
    pub coverage: Option<Coverage>,
    pub plans: Vec<GuardPlan>,
    pub guard_audit: Option<parallaxe_core::guard::GuardAudit>,
    pub errors: Vec<RunErrorRecord>,
    pub timeline: Vec<TimelineEntry>,
    pub raw_events: Vec<Event>,
    pub report: Option<Report>,
    pub last_seq: u64,
    pub seen_seqs: BTreeSet<u64>,
    pub seq_gap_detected: bool,
    pub ignored_duplicate_events: usize,
    pub ignored_late_events: usize,
    pub cancel_requested: bool,
}

impl RunView {
    pub fn new(run_id: String, is_fixture: bool) -> Self {
        RunView {
            run_id,
            is_fixture,
            mode_label: if is_fixture {
                "FIXTURE".to_string()
            } else {
                "ONLINE".to_string()
            },
            mode: None,
            profile: None,
            catalog_revision: String::new(),
            selected_ids: Vec::new(),
            admitted_ids: Vec::new(),
            skipped: Vec::new(),
            budget: None,
            preflight_path: None,
            preflight_ready: false,
            running: true,
            terminal: None,
            in_progress_targets: BTreeSet::new(),
            live_evidence: BTreeMap::new(),
            result_order: Vec::new(),
            results_by_target: HashMap::new(),
            coverage: None,
            plans: Vec::new(),
            guard_audit: None,
            errors: Vec::new(),
            timeline: Vec::new(),
            raw_events: Vec::new(),
            report: None,
            last_seq: 0,
            seen_seqs: BTreeSet::new(),
            seq_gap_detected: false,
            ignored_duplicate_events: 0,
            ignored_late_events: 0,
            cancel_requested: false,
        }
    }

    /// 终态以 `report.results`（目录顺序）为准，进行中按已收到的 `result_order` 返回。
    pub fn ordered_results(&self) -> Vec<&ResultRecord> {
        if let Some(report) = &self.report {
            return report.results.iter().collect();
        }
        self.result_order
            .iter()
            .filter_map(|id| self.results_by_target.get(id))
            .collect()
    }

    /// 查找指定目标的结果。
    pub fn result_for(&self, target_id: &str) -> Option<&ResultRecord> {
        if let Some(report) = &self.report {
            return report.results.iter().find(|r| r.target == target_id);
        }
        self.results_by_target.get(target_id)
    }

    /// 9 态全局计数（永远基于全量结果，不受筛选影响）。
    pub fn state_counts(&self) -> StateCounts {
        let mut counts = StateCounts::default();
        for result in self.ordered_results() {
            counts.record(result.state);
        }
        counts
    }

    /// 全局冲突总数（包含 `state == Conflict` 或 `conflicts` 非空的目标；不受筛选影响）。
    pub fn total_conflict_targets(&self) -> usize {
        self.ordered_results()
            .into_iter()
            .filter(|r| r.state == State::Conflict || !r.conflicts.is_empty())
            .count()
    }

    /// 全局未知项总数（`state == Unknown`；不受筛选影响）。
    pub fn total_unknown_targets(&self) -> usize {
        self.ordered_results()
            .into_iter()
            .filter(|r| r.state == State::Unknown)
            .count()
    }

    /// 全局已观察请求计数（优先读终态报告字段；运行中按已完结目标累加）。
    pub fn request_counts(&self) -> (u64, u64) {
        if let Some(report) = &self.report {
            return (report.network_request_count, report.simulated_request_count);
        }
        let sum: u64 = self.ordered_results().iter().map(|r| r.request_count).sum();
        if self.is_fixture {
            (0, sum)
        } else {
            (sum, 0)
        }
    }

    /// 退出码（严格取自 core 终态报告；未终结返回 None，绝不猜测）。
    pub fn exit_code(&self) -> Option<u8> {
        self.report.as_ref().map(|r| r.exit_code)
    }

    /// 终止原因（严格取自 core 终态报告）。
    pub fn termination(&self) -> Option<Termination> {
        self.report.as_ref().map(|r| r.termination)
    }

    /// 是否处于「选中但零准入、未发请求」状态（文档 08 明确要求如实提示）。
    pub fn is_zero_admission_online(&self) -> bool {
        !self.is_fixture
            && self.preflight_ready
            && !self.selected_ids.is_empty()
            && self.admitted_ids.is_empty()
    }
}

/// 顶层 GUI 事件状态机（无头可测）。
#[derive(Debug, Clone, Default)]
pub struct AppState {
    /// 全部已知运行（含历史，按启动先后保留，重测永不覆盖旧记录）。
    pub run_order: Vec<String>,
    pub runs: HashMap<String, RunView>,
    /// 当前正在查看的运行 ID。
    pub active_run_id: Option<String>,
    /// 当前选中的目标 ID（供右侧证据解释抽屉显示）。
    pub selected_target_id: Option<String>,
    /// 列表筛选：状态（None 表示全部；不影响顶部固定全局摘要）。
    pub filter_state: Option<State>,
    /// 列表筛选：关键字（仅过滤列表行，不影响全局冲突/UNKNOWN 摘要）。
    pub search_query: String,
    /// 最近一次命令级脱敏错误（run 之外，如非法参数）。
    pub last_command_error: Option<String>,
    /// 当前界面语言（与主题/密度正交，仅影响界面文案，绝不改变机器码或判定）。
    pub locale: Locale,
    /// 服务矩阵中用户已展开内联资格说明（ⓘ）的目标 ID 集合。
    pub expanded_eligibility: BTreeSet<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回当前生效的界面语言。
    pub fn current_locale(&self) -> Locale {
        self.locale
    }

    /// 切换界面语言（只更新展示文案，不影响任何运行结果或判定）。
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// 切换指定目标在服务矩阵中的资格说明内联展开态。
    pub fn toggle_eligibility_expanded(&mut self, target_id: &str) {
        if !self.expanded_eligibility.remove(target_id) {
            self.expanded_eligibility.insert(target_id.to_string());
        }
    }

    /// 从完整事件序列重放状态机（供无头一致性测试直接调用）。
    pub fn from_events(events: &[Event]) -> Self {
        let mut state = Self::new();
        for event in events {
            state.apply(event);
        }
        state
    }

    /// 当前激活运行视图。
    pub fn active_run(&self) -> Option<&RunView> {
        self.active_run_id
            .as_deref()
            .and_then(|id| self.runs.get(id))
    }

    /// 当前激活运行的可变引用。
    pub fn active_run_mut(&mut self) -> Option<&mut RunView> {
        let id = self.active_run_id.clone()?;
        self.runs.get_mut(&id)
    }

    /// 清除所有列表筛选。
    pub fn clear_filters(&mut self) {
        self.filter_state = None;
        self.search_query.clear();
    }

    /// 是否有生效的列表筛选。
    pub fn has_active_filters(&self) -> bool {
        self.filter_state.is_some() || !self.search_query.trim().is_empty()
    }

    /// 过滤后的结果列表（仅用于表格显示；绝不用于顶部统计）。
    pub fn filtered_results<'a>(&self, run: &'a RunView) -> Vec<&'a ResultRecord> {
        let query = self.search_query.trim().to_ascii_lowercase();
        run.ordered_results()
            .into_iter()
            .filter(|result| {
                if let Some(state) = self.filter_state {
                    if result.state != state {
                        return false;
                    }
                }
                if !query.is_empty() {
                    let hit_id = result.target.to_ascii_lowercase().contains(&query);
                    let hit_reason = result
                        .reason
                        .as_deref()
                        .is_some_and(|r| r.to_ascii_lowercase().contains(&query));
                    let hit_country = result
                        .service_observed
                        .observed_country
                        .as_deref()
                        .is_some_and(|c| c.to_ascii_lowercase().contains(&query));
                    if !hit_id && !hit_reason && !hit_country {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// 标记指定运行已请求取消（仅 UI 状态提示，真实终结仍以 `RunCancelled` / `RunFinished` 为准）。
    pub fn mark_cancel_requested(&mut self, run_id: &str) {
        if let Some(run) = self.runs.get_mut(run_id) {
            if run.running {
                run.cancel_requested = true;
            }
        }
    }

    /// 纯函数式事件投影：消费 `parallaxe-core` 单条 `Event`。
    ///
    /// - 按 `(run_id, seq)` 严格去重。
    /// - 终结事件后拒绝迟到事件，不重复计数。
    /// - 不做任何判定推导，完全信任并原样投影 `event.payload`。
    pub fn apply(&mut self, event: &Event) {
        let run_id = event.run_id.clone();
        let is_new_run = !self.runs.contains_key(&run_id);
        if is_new_run {
            self.run_order.push(run_id.clone());
            self.runs.insert(
                run_id.clone(),
                RunView::new(run_id.clone(), event.is_fixture),
            );
        }

        let Some(run) = self.runs.get_mut(&run_id) else {
            return;
        };

        // 1. 按 (run_id, seq) 去重
        if !run.seen_seqs.insert(event.seq) {
            run.ignored_duplicate_events += 1;
            return;
        }

        // 2. 终结后拒绝迟到事件（包含重复终结事件）
        if run.terminal.is_some() {
            run.ignored_late_events += 1;
            return;
        }

        // 3. 序号连续性检查（首条应为 1，其余应为 last_seq + 1）
        if event.seq != run.last_seq + 1 {
            run.seq_gap_detected = true;
        }
        run.last_seq = run.last_seq.max(event.seq);
        run.raw_events.push(event.clone());

        // 4. 穷举十种事件名（禁用 `_ =>`）
        match event.event {
            EventName::RunStarted => {
                self.last_command_error = None;
                self.active_run_id = Some(run_id.clone());
                run.is_fixture = event.is_fixture;
                run.mode_label = event
                    .payload
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or(if event.is_fixture {
                        "FIXTURE"
                    } else {
                        "ONLINE"
                    })
                    .to_string();
                run.mode = match run.mode_label.as_str() {
                    "FIXTURE" => Some(Mode::Fixture),
                    "ONLINE" => Some(Mode::Online),
                    _ => None,
                };
                run.profile = event
                    .payload
                    .get("profile")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                run.catalog_revision = event
                    .payload
                    .get("catalog_revision")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                run.selected_ids = event
                    .payload
                    .get("selected_ids")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                run.timeline.push(TimelineEntry {
                    seq: event.seq,
                    ts: event.ts.clone(),
                    event: event.event,
                    target: None,
                    summary: format!(
                        "运行开始 · 模式={} · 选中 {} 项{}",
                        run.mode_label,
                        run.selected_ids.len(),
                        run.profile
                            .as_deref()
                            .map(|p| format!(" · profile={p}"))
                            .unwrap_or_default()
                    ),
                });
            }
            EventName::PreflightReady => {
                run.preflight_ready = true;
                run.admitted_ids = event
                    .payload
                    .get("admitted_ids")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                run.skipped = event
                    .payload
                    .get("skipped")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let target = item.get("target")?.as_str()?.to_string();
                                let reason = item
                                    .get("reason")
                                    .and_then(Value::as_str)
                                    .unwrap_or("UNKNOWN")
                                    .to_string();
                                Some(SkippedEntry { target, reason })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                run.budget = event.payload.get("budget").cloned();
                run.preflight_path = event
                    .payload
                    .get("path")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Path>(v).ok());
                run.timeline.push(TimelineEntry {
                    seq: event.seq,
                    ts: event.ts.clone(),
                    event: event.event,
                    target: None,
                    summary: format!(
                        "预检就绪 · 获准 {} / 跳过 {}（预检不等于探测成功）",
                        run.admitted_ids.len(),
                        run.skipped.len()
                    ),
                });
            }
            EventName::TargetStarted => {
                let target = event
                    .payload
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let path_id = event
                    .payload
                    .get("path_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let attempt = event
                    .payload
                    .get("logical_attempt")
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                if !target.is_empty() {
                    run.in_progress_targets.insert(target.clone());
                }
                run.timeline.push(TimelineEntry {
                    seq: event.seq,
                    ts: event.ts.clone(),
                    event: event.event,
                    target: if target.is_empty() {
                        None
                    } else {
                        Some(target.clone())
                    },
                    summary: format!("开始目标 {target} · 路径 {path_id} · 逻辑尝试 #{attempt}"),
                });
            }
            EventName::EvidenceObserved => {
                let target = event
                    .payload
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some(evidence) = event
                    .payload
                    .get("evidence")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Evidence>(v).ok())
                {
                    let summary = format!(
                        "观察证据 {} · claim={} · value={}",
                        evidence.id, evidence.claim, evidence.value
                    );
                    let entry = run.live_evidence.entry(target.clone()).or_default();
                    if !entry.iter().any(|existing| existing.id == evidence.id) {
                        entry.push(evidence);
                    }
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: Some(target),
                        summary,
                    });
                }
            }
            EventName::TargetFinished => {
                if let Some(result) = event
                    .payload
                    .get("result")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<ResultRecord>(v).ok())
                {
                    let target = result.target.clone();
                    run.in_progress_targets.remove(&target);
                    if !run.results_by_target.contains_key(&target) {
                        run.result_order.push(target.clone());
                    }
                    let summary = format!(
                        "目标完成 {} · {} · {} ms · 请求 {}{}",
                        target,
                        result.state.as_str(),
                        result.duration_ms,
                        result.request_count,
                        result
                            .reason
                            .as_deref()
                            .map(|r| format!(" · 原因={r}"))
                            .unwrap_or_default()
                    );
                    run.results_by_target.insert(target.clone(), result);
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: Some(target.clone()),
                        summary,
                    });
                    if self.selected_target_id.is_none() {
                        self.selected_target_id = Some(target);
                    }
                }
            }
            EventName::CoverageUpdated => {
                if let Some(coverage) = event
                    .payload
                    .get("coverage")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Coverage>(v).ok())
                {
                    let summary = format!(
                        "覆盖快照更新 · 目录 {} / 选中 {} / 已覆盖 {} / 未覆盖 {}",
                        coverage.catalog_size,
                        coverage.selected_count,
                        coverage.covered.len(),
                        coverage.uncovered.len()
                    );
                    run.coverage = Some(coverage);
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: None,
                        summary,
                    });
                }
            }
            EventName::GuardPlanReady => {
                if let Some(plan) = event
                    .payload
                    .get("plan")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<GuardPlan>(v).ok())
                {
                    let summary = format!(
                        "Guard 只读预览 {} · safe_to_apply={} · applied={} · 主因={}",
                        plan.plan_id,
                        plan.safe_to_apply,
                        plan.applied,
                        plan.primary_reason.as_deref().unwrap_or("无")
                    );
                    if let Some(existing) = run.plans.iter_mut().find(|p| p.plan_id == plan.plan_id)
                    {
                        *existing = plan;
                    } else {
                        run.plans.push(plan);
                    }
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: None,
                        summary,
                    });
                }
            }
            EventName::Error => {
                let scope = event
                    .payload
                    .get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or("global")
                    .to_string();
                let code = event
                    .payload
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("ERROR")
                    .to_string();
                let message_safe = event
                    .payload
                    .get("message_safe")
                    .and_then(Value::as_str)
                    .unwrap_or("发生未说明错误")
                    .to_string();
                let recoverable = event
                    .payload
                    .get("recoverable")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let target = event
                    .payload
                    .get("target")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let summary =
                    format!("错误 [{scope}/{code}] recoverable={recoverable}：{message_safe}");
                run.errors.push(RunErrorRecord {
                    seq: event.seq,
                    ts: event.ts.clone(),
                    scope,
                    code,
                    message_safe,
                    recoverable,
                    target: target.clone(),
                });
                run.timeline.push(TimelineEntry {
                    seq: event.seq,
                    ts: event.ts.clone(),
                    event: event.event,
                    target,
                    summary,
                });
            }
            EventName::RunFinished => {
                run.running = false;
                run.terminal = Some(TerminalKind::Finished);
                run.in_progress_targets.clear();
                if let Some(report) = event
                    .payload
                    .get("report")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Report>(v).ok())
                {
                    let summary = format!(
                        "运行结束 · termination={} · 退出码={} · 墙钟={} ms · 网络请求={} / 模拟={}",
                        report.termination.as_str(),
                        report.exit_code,
                        report.wall_duration_ms,
                        report.network_request_count,
                        report.simulated_request_count
                    );
                    sync_terminal_report(run, report);
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: None,
                        summary,
                    });
                    if self.selected_target_id.is_none() {
                        if let Some(first) = run
                            .report
                            .as_ref()
                            .and_then(|r| r.results.first())
                            .map(|r| r.target.clone())
                        {
                            self.selected_target_id = Some(first);
                        }
                    }
                }
            }
            EventName::RunCancelled => {
                run.running = false;
                run.terminal = Some(TerminalKind::Cancelled);
                run.in_progress_targets.clear();
                if let Some(report) = event
                    .payload
                    .get("report")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Report>(v).ok())
                {
                    let summary = format!(
                        "运行已取消 · termination={} · 退出码={} · 已覆盖 {} / 未覆盖 {}",
                        report.termination.as_str(),
                        report.exit_code,
                        report.coverage.covered.len(),
                        report.coverage.uncovered.len()
                    );
                    sync_terminal_report(run, report);
                    run.timeline.push(TimelineEntry {
                        seq: event.seq,
                        ts: event.ts.clone(),
                        event: event.event,
                        target: None,
                        summary,
                    });
                }
            }
        }
    }
}

/// 终结事件与 `Report` 对账：直接采用 core 终态快照，不自算覆盖或结果。
fn sync_terminal_report(run: &mut RunView, report: Report) {
    run.mode = Some(report.mode);
    run.coverage = Some(report.coverage.clone());
    run.result_order.clear();
    run.results_by_target.clear();
    for result in &report.results {
        run.result_order.push(result.target.clone());
        run.results_by_target
            .insert(result.target.clone(), result.clone());
    }
    if !report.plans.is_empty() {
        run.plans = report.plans.clone();
    }
    run.report = Some(report);
}
