//! 命令总线与后台工作线程（按钮意图 → `GuiCommand` → `parallaxe-core`）。
//!
//! 铁律：
//! - 所有探测、判定、Guard 预览、离线自检与导出均直接委派 `parallaxe-core`。
//! - GUI 不自行拼接 URL、不发网络包、不重算 state 或退出码、不重写导出格式。
//! - 每次重测都分配新的 `run_id`，永不覆盖历史运行证据。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use parallaxe_core::contracts::{
    reason, Attempt, AttemptFamily, ClosureNode, ClosureRole, Coverage, Covered, Event,
    LocalPolicy, Mode, ObservedFamily, Path, Report, ResultRecord, RiskSignal, ServiceObserved,
    State, Termination, TransportLayerResult, Uncovered,
};
use parallaxe_core::events::EventLog;
use parallaxe_core::network::{self, PathDiagnostic};
use parallaxe_core::probe::{FetchOutcome, HttpTransport, Transport};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::signals::{self, Sample};
use parallaxe_core::state::NullStore;
use parallaxe_core::targets::{Registry, HARD_LIMITS};
use parallaxe_core::verdict::{self, VerdictInput};
use parallaxe_core::{guard, linkcheck, reports, selftest, Locale};
use serde_json::Value;

/// 导出格式（全部直接调用 `parallaxe_core::reports`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Jsonl,
    Html,
    Csv,
    Markdown,
    Text,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Json => "JSON 报告 (.json)",
            ExportFormat::Jsonl => "JSONL 事件流 (.jsonl)",
            ExportFormat::Html => "离线 HTML 报告 (.html)",
            ExportFormat::Csv => "CSV 表格 (.csv)",
            ExportFormat::Markdown => "Markdown (.md)",
            ExportFormat::Text => "纯文本 (.txt)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Jsonl => "jsonl",
            ExportFormat::Html => "html",
            ExportFormat::Csv => "csv",
            ExportFormat::Markdown => "md",
            ExportFormat::Text => "txt",
        }
    }
}

/// 阶段 3 内置 6 类离线契约演示场景（全部由 `parallaxe-core` 引擎生成，标记 `is_fixture=true`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineScenario {
    /// 场景 1：Drive 302 跳转登录域 → `UNKNOWN + REDIRECT_TO_LOGIN`，地区 `null`。
    DriveLoginHandoff,
    /// 场景 2：确定拒绝（`BLOCKED`）与同层冲突平票（`CONFLICT`）共存 → 退出码 2（优先于 3）。
    BlockedAndConflictMix,
    /// 场景 3：验证码挑战页 → `UNKNOWN + CAPTCHA_CHALLENGE`，不猜国家。
    CaptchaChallenge,
    /// 场景 4：运行中途取消（`RunCancelled`）→ 保留部分结果与未启动项原因，网络请求 0。
    MidRunCancelled,
    /// 场景 5：Books / Play 共享 CDN Guard 预览 → `safe_to_apply=false`、`NO_SAFE_BLOCK`、依赖闭包。
    BooksAndSharedCdnGuard,
    /// 场景 6：IPv6 三态（`IPv6_SLOW` / `IPv6_BROKEN` / `IPV6_ONLY_EGRESS`）→ 仅作传输层诊断。
    Ipv6ThreeStates,
}

impl OfflineScenario {
    pub const ALL: [OfflineScenario; 6] = [
        OfflineScenario::DriveLoginHandoff,
        OfflineScenario::BlockedAndConflictMix,
        OfflineScenario::CaptchaChallenge,
        OfflineScenario::MidRunCancelled,
        OfflineScenario::BooksAndSharedCdnGuard,
        OfflineScenario::Ipv6ThreeStates,
    ];

    pub fn id(self) -> &'static str {
        match self {
            OfflineScenario::DriveLoginHandoff => "scenario-drive-302",
            OfflineScenario::BlockedAndConflictMix => "scenario-blocked-conflict",
            OfflineScenario::CaptchaChallenge => "scenario-captcha",
            OfflineScenario::MidRunCancelled => "scenario-cancelled",
            OfflineScenario::BooksAndSharedCdnGuard => "scenario-books-shared-cdn-guard",
            OfflineScenario::Ipv6ThreeStates => "scenario-ipv6-three-states",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OfflineScenario::DriveLoginHandoff => "① Drive 302 登录跳转 (UNKNOWN)",
            OfflineScenario::BlockedAndConflictMix => "② 同层冲突 + BLOCKED 混合 (退出码 2)",
            OfflineScenario::CaptchaChallenge => "③ 验证码挑战页 (CAPTCHA_CHALLENGE)",
            OfflineScenario::MidRunCancelled => "④ 运行中途取消 (RunCancelled)",
            OfflineScenario::BooksAndSharedCdnGuard => "⑤ Books / 共享 CDN Guard 预览",
            OfflineScenario::Ipv6ThreeStates => "⑥ IPv6 三态链路诊断",
        }
    }
}

/// UI 提交给核心协调器的命令枚举（与 CLI / agent 保持同源能力边界）。
#[derive(Debug, Clone)]
// T-7-09a 让 CheckOptions 增加了低噪声预算字段；命令枚举仅 UI 线程短暂持有。
#[allow(clippy::large_enum_variant)]
pub enum GuiCommand {
    /// 启动一次只读 `check`（含 simple / google-all / full / 自选 / 内置 fixture / 单项重测）。
    RunCheck { options: CheckOptions },
    /// 请求取消指定运行。
    CancelRun { run_id: String },
    /// 请求对指定运行的结果生成只读 Guard 预览计划（`applied=false`）。
    RequestGuardPreview {
        source_run_id: String,
        targets: Vec<String>,
        source_results: Vec<ResultRecord>,
        source_interrupted: bool,
        is_fixture: bool,
    },
    /// 运行内置离线自检套件（不触网）。
    RunSelftest { suite: Option<String> },
    /// 查询目标的静态目录解释与依赖边（只读，不发请求）。
    ExplainStaticTarget { target_id: String },
    /// 运行 6 类离线契约演示场景（通过 `parallaxe-core` 构造真实事件流，不触网）。
    RunOfflineScenario { scenario: OfflineScenario },
    /// 运行用户已导入目标与检查方式的离线合成预览（零发包，保留 `origin=USER_IMPORTED` 审计留痕）。
    RunUserOfflinePreview { target_ids: Vec<String> },
    /// 对当前选中目标执行可选只读系统 DNS 解析（调用 `parallaxe_core::linkcheck::probe_resolver`）。
    RunLinkResolve {
        target_ids: Vec<String>,
        source_report: Option<Box<Report>>,
        show_ip: bool,
    },
    /// 同步 UI 线程更新后的 `Registry`（含用户运行时导入的目标、规则与一次性会话许可句柄）。
    SyncRegistry { registry: Box<Registry> },
    /// 关闭后台工作线程。
    Shutdown,
}

/// 静态目标解释（来自 `core::runtime::explain_target` + `explain_dependencies`）。
#[derive(Debug, Clone)]
pub struct StaticTargetExplain {
    pub target_id: String,
    pub target_doc: Value,
    pub dependencies_doc: Value,
}

/// 后台工作线程回报给 UI 线程的消息。
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    /// 来自 `parallaxe-core` 的单条事件。
    Event(Event),
    /// 静态目录与依赖解释结果。
    StaticExplain(StaticTargetExplain),
    /// 离线自检摘要。
    SelftestSummary {
        run_id: String,
        passed: u32,
        failed: u32,
        summary_line: String,
    },
    /// 取消请求的确认状态。
    CancelAck {
        run_id: String,
        status: &'static str,
    },
    /// 链路体检只读 DNS 解析完成后的报告快照。
    LinkReportReady(Box<linkcheck::LinkReport>),
    GuardAuditReady(Box<guard::GuardAudit>),
    /// run 之外的脱敏命令错误（例如非法参数）。
    CommandError(String),
}

/// 同源报告导出：直接调用 `parallaxe_core::reports`，绝不二次实现。
pub fn render_export(report: &Report, events: &[Event], format: ExportFormat) -> String {
    render_export_locale(report, events, format, Locale::ZhCn)
}

/// 带 locale 的同源导出（md/txt 的列头与说明随界面语言变化）。
pub fn render_export_locale(
    report: &Report,
    events: &[Event],
    format: ExportFormat,
    locale: Locale,
) -> String {
    match format {
        ExportFormat::Json => reports::to_json(report),
        ExportFormat::Jsonl => reports::to_jsonl(events),
        ExportFormat::Html => reports::to_html(report),
        ExportFormat::Csv => reports::to_csv(report),
        ExportFormat::Markdown => reports::to_markdown(report, locale),
        ExportFormat::Text => reports::to_text(report, locale),
    }
}

/// 将导出文本写入用户显式选择的本地路径（默认不写盘，仅用户触发保存时调用）。
pub fn write_export_file(path: &PathBuf, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|err| format!("写入导出文件失败：{err}"))
}

/// 后台命令协调器句柄。
pub struct CommandBus {
    tx: mpsc::Sender<GuiCommand>,
    rx: mpsc::Receiver<WorkerMessage>,
    cancel_flag: Arc<AtomicBool>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    seq_counter: Arc<AtomicU64>,
    _handle: Option<thread::JoinHandle<()>>,
}

impl CommandBus {
    /// 启动后台 tokio 工作线程并绑定重绘回调。
    pub fn spawn(registry: Registry, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<GuiCommand>();
        let (msg_tx, msg_rx) = mpsc::channel::<WorkerMessage>();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_flag);
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
        let worker_cancel_tx = cancel_tx.clone();
        let seq_counter = Arc::new(AtomicU64::new(1));
        let worker_seq = Arc::clone(&seq_counter);

        let handle = thread::Builder::new()
            .name("parallaxe-gui-worker".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => {
                        let _ = msg_tx.send(WorkerMessage::CommandError(
                            "无法创建后台异步运行时".to_string(),
                        ));
                        wake();
                        return;
                    }
                };
                rt.block_on(worker_loop(
                    registry,
                    cmd_rx,
                    msg_tx,
                    worker_cancel,
                    worker_cancel_tx,
                    worker_seq,
                    wake,
                ));
            })
            .ok();

        CommandBus {
            tx: cmd_tx,
            rx: msg_rx,
            cancel_flag,
            cancel_tx,
            seq_counter,
            _handle: handle,
        }
    }

    /// 分配单调不重复的运行 ID（重测永远创建新 run）。
    pub fn next_run_id(&self, prefix: &str) -> String {
        let n = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
        format!("{prefix}-{nanos}-{n}")
    }

    /// 提交命令到后台工作线程。
    pub fn send(&self, command: GuiCommand) -> bool {
        if let GuiCommand::CancelRun { .. } = &command {
            self.cancel_flag.store(true, Ordering::SeqCst);
            let _ = self.cancel_tx.send_replace(true);
        }
        self.tx.send(command).is_ok()
    }

    /// 非阻塞拉取全部待处理的后台消息。
    pub fn drain(&self) -> Vec<WorkerMessage> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            out.push(msg);
        }
        out
    }
}

impl Drop for CommandBus {
    fn drop(&mut self) {
        let _ = self.cancel_tx.send_replace(true);
        let _ = self.tx.send(GuiCommand::Shutdown);
    }
}

async fn worker_loop(
    mut registry: Registry,
    cmd_rx: mpsc::Receiver<GuiCommand>,
    msg_tx: mpsc::Sender<WorkerMessage>,
    cancel_flag: Arc<AtomicBool>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    seq_counter: Arc<AtomicU64>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let send = |msg: WorkerMessage| {
        let _ = msg_tx.send(msg);
        wake();
    };

    while let Ok(command) = cmd_rx.recv() {
        match command {
            GuiCommand::Shutdown => break,
            GuiCommand::SyncRegistry { registry: updated } => {
                registry = *updated;
            }
            GuiCommand::RunUserOfflinePreview { target_ids } => {
                let n = seq_counter.fetch_add(1, Ordering::SeqCst);
                let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
                let run_id = format!("run-user-preview-{nanos}-{n}");
                match runtime::run_user_import_offline_preview(
                    &registry,
                    &target_ids,
                    None,
                    Some(run_id),
                )
                .await
                {
                    Ok(output) => {
                        for event in output.events {
                            send(WorkerMessage::Event(event));
                        }
                    }
                    Err(err) => send(WorkerMessage::CommandError(err.to_string())),
                }
            }
            GuiCommand::RunLinkResolve {
                target_ids,
                source_report,
                show_ip,
            } => {
                let now_str = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
                let mut report = match source_report {
                    Some(rep) => linkcheck::analyze(&registry, &rep, now_str),
                    None => linkcheck::analyze_selection(&registry, &target_ids, now_str),
                };
                match linkcheck::probe_resolver(&registry, &mut report, show_ip, 2_500).await {
                    Ok(()) => send(WorkerMessage::LinkReportReady(Box::new(report))),
                    Err(err) => send(WorkerMessage::CommandError(err.to_string())),
                }
            }
            GuiCommand::CancelRun { run_id } => {
                // OPT-1：通过 watch::Sender<bool> 驱动 core::runtime::run_check_streaming 实时中断探测。
                cancel_flag.store(true, Ordering::SeqCst);
                let _ = cancel_tx.send_replace(true);
                send(WorkerMessage::CancelAck {
                    run_id,
                    status: "CANCEL_SIGNAL_DISPATCHED",
                });
            }
            GuiCommand::RunCheck { mut options } => {
                cancel_flag.store(false, Ordering::SeqCst);
                let _ = cancel_tx.send_replace(false);
                let cancel_rx = cancel_tx.subscribe();
                if options.run_id.is_none() {
                    let n = seq_counter.fetch_add(1, Ordering::SeqCst);
                    let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
                    options.run_id = Some(format!("run-gui-{nanos}-{n}"));
                }
                let transport: Option<Arc<dyn Transport>> = if options.fixture.is_some() {
                    None
                } else {
                    match HttpTransport::new(options.proxy.clone(), None) {
                        Ok(t) => Some(Arc::new(t) as Arc<dyn Transport>),
                        Err(err) => {
                            send(WorkerMessage::CommandError(err));
                            continue;
                        }
                    }
                };
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
                let forward_tx = msg_tx.clone();
                let forward_wake = Arc::clone(&wake);
                let forwarder = tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        let _ = forward_tx.send(WorkerMessage::Event(event));
                        forward_wake();
                    }
                });
                let result = runtime::run_check_streaming(
                    &registry,
                    &options,
                    transport,
                    true,
                    None,
                    cancel_rx,
                    Some(event_tx),
                )
                .await;
                let _ = forwarder.await;
                if let Err(err) = result {
                    send(WorkerMessage::CommandError(err.to_string()));
                }
            }
            GuiCommand::RequestGuardPreview {
                source_run_id,
                targets,
                source_results,
                source_interrupted,
                is_fixture,
            } => {
                let n = seq_counter.fetch_add(1, Ordering::SeqCst);
                let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
                let run_id = format!("run-guard-{nanos}-{n}");
                let (events, audit) = guard_events_and_audit(
                    &registry,
                    &run_id,
                    Some(&source_run_id),
                    &targets,
                    &source_results,
                    is_fixture,
                    source_interrupted,
                );
                for event in events {
                    send(WorkerMessage::Event(event));
                }
                send(WorkerMessage::GuardAuditReady(Box::new(audit)));
            }
            GuiCommand::RunSelftest { suite } => {
                let n = seq_counter.fetch_add(1, Ordering::SeqCst);
                let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
                let run_id = format!("run-selftest-{nanos}-{n}");
                match selftest::run(&registry, suite.as_deref(), Some(run_id)).await {
                    Ok(outcome) => {
                        let summary_line = selftest::summary(&outcome);
                        let passed = outcome.passed;
                        let failed = outcome.failed;
                        let run_id = outcome.report.run_id.clone();
                        for event in outcome.events {
                            send(WorkerMessage::Event(event));
                        }
                        send(WorkerMessage::SelftestSummary {
                            run_id,
                            passed,
                            failed,
                            summary_line,
                        });
                    }
                    Err(err) => send(WorkerMessage::CommandError(err.to_string())),
                }
            }
            GuiCommand::ExplainStaticTarget { target_id } => {
                match (
                    runtime::explain_target(&registry, &target_id),
                    runtime::explain_dependencies(&registry, &target_id),
                ) {
                    (Ok(target_doc), Ok(dependencies_doc)) => {
                        send(WorkerMessage::StaticExplain(StaticTargetExplain {
                            target_id,
                            target_doc,
                            dependencies_doc,
                        }));
                    }
                    (Err(err), _) | (_, Err(err)) => {
                        send(WorkerMessage::CommandError(err.to_string()));
                    }
                }
            }
            GuiCommand::RunOfflineScenario { scenario } => {
                let n = seq_counter.fetch_add(1, Ordering::SeqCst);
                let nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
                let run_id = format!("run-scenario-{}-{nanos}-{n}", scenario.id());
                match build_offline_scenario_events(&registry, scenario, &run_id).await {
                    Ok(events) => {
                        for event in events {
                            send(WorkerMessage::Event(event));
                        }
                    }
                    Err(err) => send(WorkerMessage::CommandError(err)),
                }
            }
        }
    }
}

/// 生成与 agent `guard_plan` 完全同构的只读预览事件流（阶段 4 直接调用 `guard::plan_preview` 计算真实反向依赖闭包与否决主因优先级）。
pub fn build_guard_preview_events(
    registry: &Registry,
    run_id: &str,
    source_run_id: Option<&str>,
    targets: &[String],
    source_results: &[ResultRecord],
    is_fixture: bool,
) -> Vec<Event> {
    guard_events_and_audit(
        registry,
        run_id,
        source_run_id,
        targets,
        source_results,
        is_fixture,
        false,
    )
    .0
}

fn guard_events_and_audit(
    registry: &Registry,
    run_id: &str,
    source_run_id: Option<&str>,
    targets: &[String],
    source_results: &[ResultRecord],
    is_fixture: bool,
    source_interrupted: bool,
) -> (Vec<Event>, guard::GuardAudit) {
    let now = time::OffsetDateTime::now_utc();
    let recorded_at = parallaxe_core::state::format_time(now);
    let preview = guard::plan_preview(
        registry,
        guard::GuardRequest {
            run_id,
            source_run_id,
            targets,
            results: source_results,
            recorded_at: recorded_at.clone(),
            as_of: now.date(),
            source_interrupted,
        },
    );
    let plan = preview.plan;
    let mut log = EventLog::new(run_id, is_fixture, now);
    log.run_started(
        if is_fixture { "FIXTURE" } else { "ONLINE" },
        Some("guard-preview"),
        "parallaxe.assets/v1",
        targets,
    );
    log.guard_plan_ready(&plan);
    let mut report = Report::new(
        parallaxe_core::VERSION,
        recorded_at,
        run_id,
        if is_fixture {
            Mode::Fixture
        } else {
            Mode::Online
        },
    );
    report.coverage = Coverage::empty(registry.catalog_size());
    report.plans = vec![plan];
    report.termination = Termination::Complete;
    report.exit_code = reports::aggregate_report(&report);
    log.run_finished(&report);
    (log.into_events(), preview.audit)
}

/// 构造 6 类离线契约演示场景的事件流（完全由 `parallaxe-core` 的判定与聚合产出，无 GUI 侧推导）。
pub async fn build_offline_scenario_events(
    registry: &Registry,
    scenario: OfflineScenario,
    run_id: &str,
) -> Result<Vec<Event>, String> {
    let now = time::OffsetDateTime::now_utc();
    match scenario {
        OfflineScenario::DriveLoginHandoff => {
            let options = CheckOptions {
                fixture: Some("drive-login".to_string()),
                run_id: Some(run_id.to_string()),
                ..Default::default()
            };
            let output = runtime::run_check(registry, &options, None, false, None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(output.events)
        }
        OfflineScenario::BlockedAndConflictMix => {
            let gemini = registry
                .get("google.gemini")
                .ok_or_else(|| "缺少目标 google.gemini".to_string())?;
            let books = registry
                .get("google.books")
                .ok_or_else(|| "缺少目标 google.books".to_string())?;
            let path = Path::system("path-fixture-mix");
            let store = NullStore::default();
            let now_str = parallaxe_core::state::format_time(now);

            // 1. google.gemini 同层平票冲突（由 core signals + verdict 真实产出）
            let conflict_sample = Sample {
                status: Some(200),
                content_type: Some("text/html; charset=utf-8".to_string()),
                location: None,
                body: b"<html>,2,1,200,\"USA\",2,1,200,\"CHN\"</html>".to_vec(),
                complete: true,
                truncated: false,
                observed_family: Some(ObservedFamily::V4),
            };
            let eval = signals::evaluate(
                registry,
                gemini,
                &conflict_sample,
                &path.path_id,
                &now_str,
                true,
            );
            let outcome = FetchOutcome {
                status: conflict_sample.status,
                content_type: conflict_sample.content_type.clone(),
                location: None,
                body: conflict_sample.body.clone(),
                complete: true,
                truncated: false,
                attempts: vec![Attempt {
                    family: AttemptFamily::V4,
                    phase: "FIXTURE".to_string(),
                    connect_ms: Some(1),
                    first_byte_ms: Some(4),
                    http_status: Some(200),
                    fallback: false,
                    request_sent: true,
                }],
                request_count: 1,
                error: None,
                observed_family: Some(ObservedFamily::V4),
            };
            let gemini_result = verdict::decide(VerdictInput {
                registry,
                target: gemini,
                evaluation: &eval,
                outcome: &outcome,
                path: &path,
                path_diagnostic: PathDiagnostic::System,
                fixture: true,
                now,
                store: &store,
                expectation: None,
                capability_unsupported: None,
                cancelled: false,
                global_timeout: false,
            });

            // 2. google.books 模拟明确 BLOCKED（用于测试 2+3 混合退出码优先取 2）
            let books_result = ResultRecord {
                target: books.raw.id.clone(),
                state: State::Blocked,
                reason: Some("POLICY_BLOCKED_FIXTURE".to_string()),
                confidence: parallaxe_core::Confidence::Medium,
                source: Some(parallaxe_core::Source::DirectAnonymousObservation),
                path: path.clone(),
                duration_ms: 5,
                request_count: 1,
                evidence: Vec::new(),
                region_layers: verdict::all_layers_null(),
                observed: Vec::new(),
                conflicts: Vec::new(),
                agreement: Vec::new(),
                transport: TransportLayerResult {
                    attempts: vec![Attempt {
                        family: AttemptFamily::V4,
                        phase: "FIXTURE".to_string(),
                        connect_ms: Some(1),
                        first_byte_ms: Some(5),
                        http_status: Some(403),
                        fallback: false,
                        request_sent: true,
                    }],
                    path_diagnostic: Some(PathDiagnostic::System.as_str().to_string()),
                },
                service_observed: ServiceObserved {
                    surface: books.raw.surface.clone(),
                    observed_country: None,
                    evidence_ids: Vec::new(),
                    reason: Some("POLICY_BLOCKED_FIXTURE".to_string()),
                },
                risk_signal: RiskSignal {
                    markers: vec!["EXPLICIT_BLOCK_FIXTURE".to_string()],
                    evidence_ids: Vec::new(),
                    reason: Some("POLICY_BLOCKED_FIXTURE".to_string()),
                },
                local_policy: LocalPolicy {
                    protected: books.raw.protected,
                    guard_eligible: !books.raw.protected,
                    plan_id: None,
                    reason: Some(reason::GUARD_STAGE4_ONLY.to_string()),
                },
                expectation: None,
                origin: None,
                consent_version: None,
                imported_at: None,
                rule_digest: None,
                eligibility_notes: registry.eligibility_notes_for(&books.raw.id),
            };

            let selected_ids = vec![gemini.raw.id.clone(), books.raw.id.clone()];
            let mut log = EventLog::new(run_id, true, now);
            log.run_started(
                "FIXTURE",
                Some("scenario-blocked-conflict"),
                "parallaxe.fixtures/v1",
                &selected_ids,
            );
            log.preflight_ready(
                &selected_ids,
                &[],
                serde_json::json!({"simulated": true}),
                &path,
            );
            for res in [&gemini_result, &books_result] {
                log.target_started(&res.target, &path.path_id, 1);
                for ev in &res.evidence {
                    log.evidence_observed(&res.target, ev);
                }
                log.target_finished(res);
            }
            let coverage = build_scenario_coverage(
                registry,
                &selected_ids,
                &[
                    Covered {
                        target: gemini.raw.id.clone(),
                        request_count: 1,
                        http_received: true,
                    },
                    Covered {
                        target: books.raw.id.clone(),
                        request_count: 1,
                        http_received: true,
                    },
                ],
                &[],
            );
            let mut report = Report::new(parallaxe_core::VERSION, now_str, run_id, Mode::Fixture);
            report.results = vec![gemini_result, books_result];
            report.coverage = coverage;
            report.simulated_request_count = 2;
            report.network_request_count = 0;
            report.termination = Termination::Complete;
            report.exit_code = reports::aggregate_report(&report);
            log.coverage_updated(&report.coverage);
            log.run_finished(&report);
            Ok(log.into_events())
        }
        OfflineScenario::CaptchaChallenge => {
            let target = registry
                .get("google.home")
                .ok_or_else(|| "缺少目标 google.home".to_string())?;
            let path = Path::system("path-fixture-captcha");
            let store = NullStore::default();
            let now_str = parallaxe_core::state::format_time(now);
            let captcha_sample = Sample {
                status: Some(302),
                content_type: Some("text/html".to_string()),
                location: Some("https://www.google.com/sorry/index".to_string()),
                body: Vec::new(),
                complete: true,
                truncated: false,
                observed_family: Some(ObservedFamily::V4),
            };
            let eval = signals::evaluate(
                registry,
                target,
                &captcha_sample,
                &path.path_id,
                &now_str,
                true,
            );
            let outcome = FetchOutcome {
                status: Some(302),
                content_type: Some("text/html".to_string()),
                location: captcha_sample.location.clone(),
                body: Vec::new(),
                complete: true,
                truncated: false,
                attempts: vec![Attempt {
                    family: AttemptFamily::V4,
                    phase: "FIXTURE".to_string(),
                    connect_ms: Some(1),
                    first_byte_ms: Some(3),
                    http_status: Some(302),
                    fallback: false,
                    request_sent: true,
                }],
                request_count: 1,
                error: None,
                observed_family: Some(ObservedFamily::V4),
            };
            let result = verdict::decide(VerdictInput {
                registry,
                target,
                evaluation: &eval,
                outcome: &outcome,
                path: &path,
                path_diagnostic: PathDiagnostic::System,
                fixture: true,
                now,
                store: &store,
                expectation: None,
                capability_unsupported: None,
                cancelled: false,
                global_timeout: false,
            });
            let selected_ids = vec![target.raw.id.clone()];
            let mut log = EventLog::new(run_id, true, now);
            log.run_started(
                "FIXTURE",
                Some("scenario-captcha"),
                "parallaxe.fixtures/v1",
                &selected_ids,
            );
            log.preflight_ready(
                &selected_ids,
                &[],
                serde_json::json!({"simulated": true}),
                &path,
            );
            log.target_started(&target.raw.id, &path.path_id, 1);
            for ev in &result.evidence {
                log.evidence_observed(&target.raw.id, ev);
            }
            log.target_finished(&result);
            let coverage = build_scenario_coverage(
                registry,
                &selected_ids,
                &[Covered {
                    target: target.raw.id.clone(),
                    request_count: 1,
                    http_received: true,
                }],
                &[],
            );
            let mut report = Report::new(parallaxe_core::VERSION, now_str, run_id, Mode::Fixture);
            report.results = vec![result];
            report.coverage = coverage;
            report.simulated_request_count = 1;
            report.network_request_count = 0;
            report.termination = Termination::Complete;
            report.exit_code = reports::aggregate_report(&report);
            log.coverage_updated(&report.coverage);
            log.run_finished(&report);
            Ok(log.into_events())
        }
        OfflineScenario::MidRunCancelled => {
            let gemini = registry
                .get("google.gemini")
                .ok_or_else(|| "缺少目标 google.gemini".to_string())?;
            let books = registry
                .get("google.books")
                .ok_or_else(|| "缺少目标 google.books".to_string())?;
            let path = Path::system("path-fixture-cancel");
            let store = NullStore::default();
            let now_str = parallaxe_core::state::format_time(now);
            let sample = Sample {
                status: Some(200),
                content_type: Some("text/html; charset=utf-8".to_string()),
                location: None,
                body: b"<html><title>Gemini</title><script>AF_initDataCallback({data:[,2,1,200,\"USA\"]})</script></html>".to_vec(),
                complete: true,
                truncated: false,
                observed_family: Some(ObservedFamily::V4),
            };
            let eval = signals::evaluate(registry, gemini, &sample, &path.path_id, &now_str, true);
            let outcome = FetchOutcome {
                status: Some(200),
                content_type: sample.content_type.clone(),
                location: None,
                body: sample.body.clone(),
                complete: true,
                truncated: false,
                attempts: vec![Attempt {
                    family: AttemptFamily::V4,
                    phase: "FIXTURE".to_string(),
                    connect_ms: Some(1),
                    first_byte_ms: Some(2),
                    http_status: Some(200),
                    fallback: false,
                    request_sent: true,
                }],
                request_count: 1,
                error: None,
                observed_family: Some(ObservedFamily::V4),
            };
            let gemini_result = verdict::decide(VerdictInput {
                registry,
                target: gemini,
                evaluation: &eval,
                outcome: &outcome,
                path: &path,
                path_diagnostic: PathDiagnostic::System,
                fixture: true,
                now,
                store: &store,
                expectation: None,
                capability_unsupported: None,
                cancelled: false,
                global_timeout: false,
            });
            let selected_ids = vec![gemini.raw.id.clone(), books.raw.id.clone()];
            let mut log = EventLog::new(run_id, true, now);
            log.run_started(
                "FIXTURE",
                Some("scenario-cancelled"),
                "parallaxe.fixtures/v1",
                &selected_ids,
            );
            log.preflight_ready(
                &selected_ids,
                &[],
                serde_json::json!({"simulated": true}),
                &path,
            );
            log.target_started(&gemini.raw.id, &path.path_id, 1);
            for ev in &gemini_result.evidence {
                log.evidence_observed(&gemini.raw.id, ev);
            }
            log.target_finished(&gemini_result);
            let coverage = build_scenario_coverage(
                registry,
                &selected_ids,
                &[Covered {
                    target: gemini.raw.id.clone(),
                    request_count: 1,
                    http_received: true,
                }],
                &[(books.raw.id.clone(), reason::CANCELLED.to_string())],
            );
            let mut report = Report::new(parallaxe_core::VERSION, now_str, run_id, Mode::Fixture);
            report.results = vec![gemini_result];
            report.coverage = coverage;
            report.simulated_request_count = 1;
            report.network_request_count = 0;
            report.termination = Termination::Cancelled;
            report.exit_code = reports::aggregate_report(&report);
            log.coverage_updated(&report.coverage);
            log.run_cancelled(&report);
            Ok(log.into_events())
        }
        OfflineScenario::BooksAndSharedCdnGuard => {
            let books = registry
                .get("google.books")
                .ok_or_else(|| "缺少目标 google.books".to_string())?;
            let path = Path::system("path-fixture-guard");
            let now_str = parallaxe_core::state::format_time(now);
            let books_result = ResultRecord {
                target: books.raw.id.clone(),
                state: State::Restricted,
                reason: Some("REGION_RESTRICTED_FIXTURE".to_string()),
                confidence: parallaxe_core::Confidence::Medium,
                source: Some(parallaxe_core::Source::DirectAnonymousObservation),
                path: path.clone(),
                duration_ms: 4,
                request_count: 1,
                evidence: Vec::new(),
                region_layers: verdict::all_layers_null(),
                observed: Vec::new(),
                conflicts: Vec::new(),
                agreement: Vec::new(),
                transport: TransportLayerResult {
                    attempts: vec![Attempt {
                        family: AttemptFamily::V4,
                        phase: "FIXTURE".to_string(),
                        connect_ms: Some(1),
                        first_byte_ms: Some(4),
                        http_status: Some(200),
                        fallback: false,
                        request_sent: true,
                    }],
                    path_diagnostic: Some(PathDiagnostic::System.as_str().to_string()),
                },
                service_observed: ServiceObserved {
                    surface: books.raw.surface.clone(),
                    observed_country: Some("CN".to_string()),
                    evidence_ids: Vec::new(),
                    reason: Some("REGION_RESTRICTED_FIXTURE".to_string()),
                },
                risk_signal: RiskSignal {
                    markers: Vec::new(),
                    evidence_ids: Vec::new(),
                    reason: None,
                },
                local_policy: LocalPolicy {
                    protected: books.raw.protected,
                    guard_eligible: !books.raw.protected,
                    plan_id: Some(format!("plan-{run_id}")),
                    reason: Some(reason::NO_SAFE_BLOCK.to_string()),
                },
                expectation: None,
                origin: None,
                consent_version: None,
                imported_at: None,
                rule_digest: None,
                eligibility_notes: registry.eligibility_notes_for(&books.raw.id),
            };

            let mut plan = guard::preview_plan(
                run_id,
                Some(run_id),
                std::slice::from_ref(&books.raw.id),
                std::slice::from_ref(&books_result),
                false,
            );
            // 填充共享 CDN / Play 下载依赖链的静态依赖边（来自 registry.dependencies，只读说明）
            plan.affected_domains = vec![
                "books.google.com".to_string(),
                "gstatic.com".to_string(),
                "googleusercontent.com".to_string(),
            ];
            plan.affected_services = vec!["google.gemini".to_string(), "google.play".to_string()];
            plan.affected_shared_paths = vec![
                "gstatic.com (SHARED CDN)".to_string(),
                "dl.google.com / gvt1.com (GOOGLE_PLAY 下载依赖受保护)".to_string(),
            ];
            plan.impact_closure = vec![
                ClosureNode {
                    node: "books.google.com".to_string(),
                    via: None,
                    kind: "CANDIDATE".to_string(),
                    source_ref: "assets/targets.toml".to_string(),
                    role: ClosureRole::Affected,
                },
                ClosureNode {
                    node: "gstatic.com".to_string(),
                    via: Some("books.google.com".to_string()),
                    kind: "SHARED_CDN".to_string(),
                    source_ref: "assets/dependencies.toml".to_string(),
                    role: ClosureRole::Affected,
                },
                ClosureNode {
                    node: "google.play".to_string(),
                    via: Some("gstatic.com".to_string()),
                    kind: "PROTECTED_DOWNLOAD_CHAIN".to_string(),
                    source_ref: "assets/dependencies.toml".to_string(),
                    role: ClosureRole::Associated,
                },
            ];
            plan.reasons
                .push("BLOCK_DEFERRED_DEPENDENCY_CONFLICT".to_string());
            plan.primary_reason = Some(reason::NO_SAFE_BLOCK.to_string());
            plan.safe_to_apply = false;
            plan.applied = false;

            let selected_ids = vec![books.raw.id.clone()];
            let mut log = EventLog::new(run_id, true, now);
            log.run_started(
                "FIXTURE",
                Some("scenario-books-shared-cdn-guard"),
                "parallaxe.fixtures/v1",
                &selected_ids,
            );
            log.preflight_ready(
                &selected_ids,
                &[],
                serde_json::json!({"simulated": true}),
                &path,
            );
            log.target_started(&books.raw.id, &path.path_id, 1);
            log.target_finished(&books_result);
            log.guard_plan_ready(&plan);
            let coverage = build_scenario_coverage(
                registry,
                &selected_ids,
                &[Covered {
                    target: books.raw.id.clone(),
                    request_count: 1,
                    http_received: true,
                }],
                &[],
            );
            let mut report = Report::new(parallaxe_core::VERSION, now_str, run_id, Mode::Fixture);
            report.results = vec![books_result];
            report.coverage = coverage;
            report.plans = vec![plan];
            report.simulated_request_count = 1;
            report.network_request_count = 0;
            report.termination = Termination::Complete;
            report.exit_code = reports::aggregate_report(&report);
            log.coverage_updated(&report.coverage);
            log.run_finished(&report);
            Ok(log.into_events())
        }
        OfflineScenario::Ipv6ThreeStates => {
            let t_slow = registry
                .get("google.gmail")
                .ok_or_else(|| "缺少目标 google.gmail".to_string())?;
            let t_broken = registry
                .get("google.drive")
                .ok_or_else(|| "缺少目标 google.drive".to_string())?;
            let t_v6only = registry
                .get("google.http204")
                .ok_or_else(|| "缺少目标 google.http204".to_string())?;
            let store = NullStore::default();
            let now_str = parallaxe_core::state::format_time(now);
            let path = Path::system("path-fixture-v6");

            let build_v6_result = |target: &parallaxe_core::Target,
                                   attempts: Vec<Attempt>,
                                   observed_family: Option<ObservedFamily>,
                                   status: u16| {
                let diag = network::diagnose_attempts(&attempts, HARD_LIMITS.v6_fallback_after_ms);
                let sample = Sample {
                    status: Some(status),
                    content_type: Some("text/plain".to_string()),
                    location: None,
                    body: Vec::new(),
                    complete: true,
                    truncated: false,
                    observed_family,
                };
                let eval =
                    signals::evaluate(registry, target, &sample, &path.path_id, &now_str, true);
                let outcome = FetchOutcome {
                    status: Some(status),
                    content_type: sample.content_type.clone(),
                    location: None,
                    body: Vec::new(),
                    complete: true,
                    truncated: false,
                    attempts,
                    request_count: 1,
                    error: None,
                    observed_family,
                };
                verdict::decide(VerdictInput {
                    registry,
                    target,
                    evaluation: &eval,
                    outcome: &outcome,
                    path: &path,
                    path_diagnostic: diag,
                    fixture: true,
                    now,
                    store: &store,
                    expectation: None,
                    capability_unsupported: None,
                    cancelled: false,
                    global_timeout: false,
                })
            };

            // 1. IPv6_SLOW：v6 首包 11_000 ms > 8_000 ms 但最终成功
            let r_slow = build_v6_result(
                t_slow,
                vec![Attempt {
                    family: AttemptFamily::V6,
                    phase: "HTTP_RESPONSE".to_string(),
                    connect_ms: None,
                    first_byte_ms: Some(11_000),
                    http_status: Some(200),
                    fallback: false,
                    request_sent: true,
                }],
                Some(ObservedFamily::V6),
                200,
            );
            // 2. IPv6_BROKEN：v6 超时失败，受控回落 v4 成功
            let r_broken = build_v6_result(
                t_broken,
                vec![
                    Attempt {
                        family: AttemptFamily::V6,
                        phase: "TIMEOUT".to_string(),
                        connect_ms: None,
                        first_byte_ms: None,
                        http_status: None,
                        fallback: false,
                        request_sent: false,
                    },
                    Attempt {
                        family: AttemptFamily::V4,
                        phase: "HTTP_RESPONSE".to_string(),
                        connect_ms: None,
                        first_byte_ms: Some(120),
                        http_status: Some(200),
                        fallback: true,
                        request_sent: true,
                    },
                ],
                Some(ObservedFamily::V4),
                200,
            );
            // 3. IPV6_ONLY_EGRESS：v4 失败但 v6 成功（不得判节点故障）
            let r_v6only = build_v6_result(
                t_v6only,
                vec![
                    Attempt {
                        family: AttemptFamily::V4,
                        phase: "CONNECT".to_string(),
                        connect_ms: None,
                        first_byte_ms: None,
                        http_status: None,
                        fallback: false,
                        request_sent: false,
                    },
                    Attempt {
                        family: AttemptFamily::V6,
                        phase: "HTTP_RESPONSE".to_string(),
                        connect_ms: None,
                        first_byte_ms: Some(45),
                        http_status: Some(204),
                        fallback: false,
                        request_sent: true,
                    },
                ],
                Some(ObservedFamily::V6),
                204,
            );

            let selected_ids = vec![
                t_slow.raw.id.clone(),
                t_broken.raw.id.clone(),
                t_v6only.raw.id.clone(),
            ];
            let mut log = EventLog::new(run_id, true, now);
            log.run_started(
                "FIXTURE",
                Some("scenario-ipv6-three-states"),
                "parallaxe.fixtures/v1",
                &selected_ids,
            );
            log.preflight_ready(
                &selected_ids,
                &[],
                serde_json::json!({"simulated": true, "v6_fallback_after_ms": HARD_LIMITS.v6_fallback_after_ms}),
                &path,
            );
            for res in [&r_slow, &r_broken, &r_v6only] {
                log.target_started(&res.target, &path.path_id, 1);
                for ev in &res.evidence {
                    log.evidence_observed(&res.target, ev);
                }
                log.target_finished(res);
            }
            let coverage = build_scenario_coverage(
                registry,
                &selected_ids,
                &[
                    Covered {
                        target: t_slow.raw.id.clone(),
                        request_count: 1,
                        http_received: true,
                    },
                    Covered {
                        target: t_broken.raw.id.clone(),
                        request_count: 1,
                        http_received: true,
                    },
                    Covered {
                        target: t_v6only.raw.id.clone(),
                        request_count: 1,
                        http_received: true,
                    },
                ],
                &[],
            );
            let mut report = Report::new(parallaxe_core::VERSION, now_str, run_id, Mode::Fixture);
            report.results = vec![r_slow, r_broken, r_v6only];
            report.coverage = coverage;
            report.simulated_request_count = 3;
            report.network_request_count = 0;
            report.termination = Termination::Complete;
            report.exit_code = reports::aggregate_report(&report);
            log.coverage_updated(&report.coverage);
            log.run_finished(&report);
            Ok(log.into_events())
        }
    }
}

fn build_scenario_coverage(
    registry: &Registry,
    selected_ids: &[String],
    covered_items: &[Covered],
    explicit_uncovered_reasons: &[(String, String)],
) -> Coverage {
    let selected: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
    let covered_map: std::collections::HashMap<&str, &Covered> = covered_items
        .iter()
        .map(|item| (item.target.as_str(), item))
        .collect();
    let reason_map: std::collections::HashMap<&str, &str> = explicit_uncovered_reasons
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let mut covered = Vec::new();
    let mut uncovered = Vec::new();
    for entry in registry.entries() {
        let id = entry.id();
        if let Some(item) = covered_map.get(id) {
            covered.push((*item).clone());
        } else {
            let is_selected = selected.contains(id);
            let reason_text = reason_map.get(id).copied().unwrap_or(if is_selected {
                reason::NO_ADMISSION
            } else {
                reason::NOT_SELECTED
            });
            uncovered.push(Uncovered {
                target: id.to_string(),
                selected: is_selected,
                reason: reason_text.to_string(),
            });
        }
    }
    Coverage {
        catalog_size: registry.catalog_size() as u64,
        selected_count: selected_ids.len() as u64,
        covered,
        uncovered,
    }
}

#[cfg(test)]
mod guard_audit_tests {
    use super::*;

    #[tokio::test]
    async fn interrupted_source_is_preserved_in_both_plan_and_audit() {
        let registry = Registry::embedded().unwrap();
        let events = build_offline_scenario_events(
            &registry,
            OfflineScenario::BooksAndSharedCdnGuard,
            "source",
        )
        .await
        .unwrap();
        let state = crate::state::AppState::from_events(&events);
        let source = state.runs.get("source").unwrap();
        let results: Vec<_> = source.ordered_results().into_iter().cloned().collect();
        for interrupted in [false, true] {
            let (events, audit) = guard_events_and_audit(
                &registry,
                "preview",
                Some("source"),
                &["google.books".into()],
                &results,
                true,
                interrupted,
            );
            let plan = events.iter().find_map(|e| e.payload.get("plan")).unwrap();
            assert_eq!(audit.selection.candidates, ["google.books"]);
            assert_eq!(audit.closure_complete, !interrupted);
            assert_eq!(plan["closure_complete"], audit.closure_complete);
            assert_eq!(plan["plan_id"], audit.plan_id);
        }
    }
}
