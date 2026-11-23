//! `parallaxe` 命令行入口：clap 解析 → 共享 core → 投影输出与退出码。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use parallaxe_core::contracts::{GuardPlan, Report};
use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::linkcheck;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::{ConfigError, LoadPaths, Registry};
use parallaxe_core::{guard, guard_export, reports, selftest};
use time::OffsetDateTime;

// print!/println! 在断管时可能 panic；所有命令按运行错误返回 1。
macro_rules! emit {
    ($write:ident, $stream:ident, $($arg:tt)*) => {{
        use std::io::Write;
        if $write!(std::io::$stream().lock(), $($arg)*).is_err() {return ExitCode::from(1);}
    }};
}

#[derive(Parser)]
#[command(
    name = "parallaxe",
    bin_name = "parallaxe",
    version,
    about = "Parallaxe box：Google 全服务出口地区观测 / 链路诊断 / 安全防护（只读）",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// 离线自检快捷方式（等价于 `selftest --offline`）
    #[arg(long)]
    selftest: bool,
    /// 显式配置文件（仅允许真实优先级文件，示例文件会被拒绝）
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// 显式 targets 目录覆盖层（只读、不得提权）
    #[arg(long, global = true)]
    targets_dir: Option<PathBuf>,
    /// 显式 profiles 目录覆盖层
    #[arg(long, global = true)]
    profiles_dir: Option<PathBuf>,
    /// 界面语言（仅影响人类可读输出；JSON/JSONL/agent 的机器字段不受影响）
    #[arg(
        long,
        global = true,
        default_value = "zh-CN",
        value_parser = ["zh-CN", "en-US"]
    )]
    locale: String,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 只读探测（默认空许可：选中但不发包）
    Check(CheckArgs),
    /// 解释目标目录或离线样本
    Explain(ExplainArgs),
    /// 目录操作
    Targets {
        #[command(subcommand)]
        command: TargetsCommand,
    },
    /// 离线自检（不触网）
    Selftest(SelftestArgs),
    /// Guard 只读预览（阶段 2 只给拒绝性预览）
    Guard {
        #[command(subcommand)]
        command: GuardCommand,
    },
    /// 隔离解释（不创建、不切换线路）
    Isolate {
        #[command(subcommand)]
        command: IsolateCommand,
    },
    /// 优先级列表（仅用户显式选择）
    Priorities {
        #[command(subcommand)]
        command: PrioritiesCommand,
    },
    /// 依赖图解释
    Dependencies {
        #[command(subcommand)]
        command: DependenciesCommand,
    },
    /// 链路体检（只读；默认零发包，禁端口扫描）
    Link {
        #[command(subcommand)]
        command: LinkCommand,
    },
    /// NDJSON stdio agent
    Agent {
        /// 仅支持 --stdio；不监听端口
        #[arg(long, required = true)]
        stdio: bool,
    },
    /// 启动原生 egui 图形界面（仅本机窗口，消费共享核心事件流）
    Gui,
}

#[derive(Args, Default)]
struct CheckArgs {
    /// 无 GUI 运行（本阶段唯一模式）
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long, action = ArgAction::Append)]
    group: Vec<String>,
    #[arg(long, action = ArgAction::Append)]
    target: Vec<String>,
    #[arg(long)]
    full: bool,
    #[arg(long)]
    expect: Option<String>,
    #[arg(long)]
    fast: bool,
    #[arg(long)]
    ipv4: bool,
    #[arg(long)]
    interface: Option<String>,
    #[arg(long)]
    proxy: Option<String>,
    #[arg(long)]
    timeout: Option<u64>,
    #[arg(long = "max-concurrency")]
    max_concurrency: Option<u32>,
    /// 同 host / 节点 / 协议的发送间隔（毫秒，0–5000；默认 3500）
    #[arg(long = "min-interval-ms")]
    min_interval_ms: Option<u64>,
    /// 小批次大小（1–15，默认 5）
    #[arg(long = "batch-size")]
    batch_size: Option<u32>,
    /// 批次冷却（毫秒，0–60000，默认 45000）
    #[arg(long = "batch-cooldown-ms")]
    batch_cooldown_ms: Option<u64>,
    /// 单节点复测次数（0–1）
    #[arg(long = "node-retest")]
    node_retest: Option<u32>,
    /// 只生成运行前风险预览（零发包），不执行检测
    #[arg(long)]
    plan: bool,
    /// 受控全量：新建连接超过安全预算（30）时显式确认
    #[arg(long = "confirm-budget")]
    confirm_budget: bool,
    /// 节点筛选（低噪声两阶段第一阶段）：每节点最小探针；需要 --proxy
    #[arg(long = "node-sweep")]
    node_sweep: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    jsonl: bool,
    #[arg(long)]
    html: bool,
    #[arg(long)]
    csv: bool,
    /// 统一格式选择：json / jsonl / html / csv / table / md / txt（与旧布尔开关二选一）
    #[arg(long, value_parser = ["json", "jsonl", "html", "csv", "table", "md", "txt"])]
    format: Option<String>,
    #[arg(long)]
    trace: bool,
    #[arg(long = "request-log")]
    request_log: bool,
    #[arg(long)]
    redact: bool,
    #[arg(long = "show-ip")]
    show_ip: bool,
    #[arg(long = "state-dir")]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    fixture: Option<String>,
}

#[derive(Args)]
struct ExplainArgs {
    #[arg(long)]
    target: String,
    #[arg(long)]
    fixture: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum TargetsCommand {
    List {
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct SelftestArgs {
    #[arg(long)]
    offline: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    suite: Option<String>,
}

#[derive(Subcommand)]
enum GuardCommand {
    Plan {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        target: Vec<String>,
        /// 只读取既有 `check --json` 报告作为结果来源（不发起新请求）
        #[arg(long = "source-report")]
        source_report: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// 规则导出：静态文件 / 剪贴板来源；无回环服务、无临时订阅端口
    Export {
        #[arg(long)]
        target: Vec<String>,
        /// 只读取既有 `check --json` 报告作为结果来源（不发起新请求）
        #[arg(long = "source-report")]
        source_report: Option<PathBuf>,
        #[arg(
            long,
            default_value = "clash",
            value_parser = ["clash", "singbox", "domainlist"]
        )]
        format: String,
        /// provider = 不污染式（推荐）；full = 整包替换（第二备选，强警示）
        #[arg(long, default_value = "provider", value_parser = ["provider", "full"])]
        mode: String,
        /// 输出目录（会写入 1–3 个静态文件；不存在的目录会被创建）
        #[arg(long)]
        out: PathBuf,
        /// 整包替换必须显式确认：会覆盖原有分流规则，请先备份
        #[arg(long = "acknowledge-overwrite")]
        acknowledge_overwrite: bool,
        #[arg(long)]
        json: bool,
    },
    /// 系统级阻断：仅 Win/Linux，显式开启、可一键撤销（需管理员/root）
    Block {
        #[arg(long)]
        target: Vec<String>,
        #[arg(long = "source-report")]
        source_report: Option<PathBuf>,
        /// 显式解析候选 FQDN（会产生 DNS 查询）
        #[arg(long)]
        resolve: bool,
        /// 手动提供 host=ip（可重复；与 --resolve 二选一）
        #[arg(long = "ip")]
        ip: Vec<String>,
        /// 真正写入系统；缺省只打印计划与命令（dry-run）
        #[arg(long)]
        apply: bool,
        /// 二次确认：没有它不写系统
        #[arg(long)]
        confirm: bool,
        #[arg(long = "audit-out")]
        audit_out: Option<PathBuf>,
        #[arg(long = "rule-tag", default_value = "default")]
        rule_tag: String,
        #[arg(long)]
        json: bool,
    },
    /// 一键撤销系统阻断（按规则标签删除，零残留）
    Unblock {
        #[arg(long = "rule-tag")]
        rule_tag: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    /// 回滚预览：本版本没有 apply 能力，只报告“无可回滚”
    Rollback {
        #[arg(long = "plan-id")]
        plan_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum LinkCommand {
    /// 四列只读链路体检（resolver / 连接边缘 / HTTP 出口 / 地址族）
    Explain {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, action = ArgAction::Append)]
        group: Vec<String>,
        #[arg(long, action = ArgAction::Append)]
        target: Vec<String>,
        #[arg(long)]
        fixture: Option<String>,
        /// 显式执行一次系统 DNS 查询（仅内置目录主机；计入 dns_query_count）
        #[arg(long, conflicts_with = "fixture")]
        resolve: bool,
        #[arg(long = "show-ip")]
        show_ip: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum IsolateCommand {
    Plan {
        #[arg(long = "protect", action = ArgAction::Append)]
        protect: Vec<String>,
        #[arg(long = "defer", action = ArgAction::Append)]
        defer: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    Explain {
        #[arg(long)]
        target: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum PrioritiesCommand {
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum DependenciesCommand {
    Explain {
        #[arg(long)]
        target: String,
        #[arg(long)]
        json: bool,
    },
}

fn load_registry(cli: &Cli) -> Result<Registry, ConfigError> {
    let paths = LoadPaths {
        targets_dir: cli.targets_dir.clone(),
        profiles_dir: cli.profiles_dir.clone(),
        config: cli.config.clone(),
    };
    Registry::load(&paths)
}

/// 单进程只设置一次；仅影响 CLI 的人类可读输出。
static LOCALE: std::sync::OnceLock<Locale> = std::sync::OnceLock::new();

fn locale() -> Locale {
    *LOCALE.get().unwrap_or(&Locale::ZhCn)
}

fn report_exit(report: &Report) -> ExitCode {
    ExitCode::from(report.exit_code)
}

fn print_error(error: &ConfigError) -> ExitCode {
    emit!(
        writeln,
        stderr,
        "{}{}",
        translate(locale(), "cli.error_prefix", &[]),
        error.localized(locale())
    );
    ExitCode::from(reports::usage_error_code())
}

async fn run_check(registry: &Registry, args: CheckArgs) -> ExitCode {
    if args.redact && args.show_ip {
        emit!(writeln, stderr, "错误：--redact 与 --show-ip 互斥");
        return ExitCode::from(1);
    }
    let formats = [args.json, args.jsonl, args.html, args.csv]
        .iter()
        .filter(|flag| **flag)
        .count();
    if formats > 1 || (formats > 0 && args.format.is_some()) {
        emit!(
            writeln,
            stderr,
            "错误：--json/--jsonl/--html/--csv/--format 只能选一种"
        );
        return ExitCode::from(1);
    }
    let format = args.format.clone().or_else(|| {
        if args.json {
            Some("json".to_string())
        } else if args.jsonl {
            Some("jsonl".to_string())
        } else if args.html {
            Some("html".to_string())
        } else if args.csv {
            Some("csv".to_string())
        } else {
            None
        }
    });
    if args.fixture.is_some() && (args.proxy.is_some() || args.interface.is_some()) {
        emit!(
            writeln,
            stderr,
            "错误：--fixture 与 --proxy/--interface 互斥"
        );
        return ExitCode::from(1);
    }
    let options = CheckOptions {
        profile: args.profile.clone(),
        groups: args.group.clone(),
        targets: args.target.clone(),
        full: args.full,
        expect: args.expect.clone(),
        fixture: args.fixture.clone(),
        state_dir: args.state_dir.clone(),
        proxy: args.proxy.clone(),
        interface: args.interface.clone(),
        ipv4: args.ipv4,
        fast: args.fast,
        timeout_ms: args.timeout,
        max_concurrency: args.max_concurrency,
        min_interval_ms: args.min_interval_ms,
        batch_size: args.batch_size,
        batch_cooldown_ms: args.batch_cooldown_ms,
        node_retest: args.node_retest,
        plan: args.plan,
        confirm_budget: args.confirm_budget,
        node_sweep: args.node_sweep,
        run_id: None,
    };
    if options.plan {
        return match runtime::network_plan(registry, &options) {
            Ok(preview) => {
                if args.json || args.format.as_deref() == Some("json") {
                    emit!(
                        writeln,
                        stdout,
                        "{}",
                        serde_json::to_string_pretty(&preview).unwrap_or_default()
                    );
                } else {
                    emit!(writeln, stdout, "运行前风险预览（零发包）");
                    emit!(
                        writeln,
                        stdout,
                        "范围：{} / profile：{} / 模式：{} / 目标 {} / 已知节点 {}",
                        preview.scope,
                        preview.profile.as_deref().unwrap_or("（未指定）"),
                        preview.scan_mode,
                        preview.target_count,
                        preview.node_count
                    );
                    emit!(
                        writeln,
                        stdout,
                        "协议：TCP/HTTP {} / UDP/QUIC {} / 独立 DNS {}",
                        preview.tcp_http_targets,
                        preview.udp_quic_targets,
                        preview.dns_queries
                    );
                    emit!(
                        writeln,
                        stdout,
                        "预计新连接 {} / UDP-QUIC 会话 {} / 最大并发 {}（TCP {} / UDP {}）",
                        preview.estimated_new_connections,
                        preview.estimated_udp_quic_sessions,
                        preview.max_concurrency,
                        preview.tcp_concurrency,
                        preview.udp_quic_concurrency
                    );
                    emit!(
                        writeln,
                        stdout,
                        "低噪声预算：间隔 {} ms / 批次 {} / 冷却 {} ms / 单节点复测 {}",
                        preview.min_interval_ms,
                        preview.batch_size,
                        preview.batch_cooldown_ms,
                        preview.node_retest
                    );
                    emit!(
                        writeln,
                        stdout,
                        "预计耗时约 {} ms；高风险协议：{}；需要额外确认：{}",
                        preview.estimated_wall_ms,
                        if preview.high_risk_protocols.is_empty() {
                            "无".to_string()
                        } else {
                            preview.high_risk_protocols.join(", ")
                        },
                        preview.requires_confirmation
                    );
                    emit!(writeln, stdout, "{}", preview.note);
                }
                ExitCode::SUCCESS
            }
            Err(error) => print_error(&error),
        };
    }
    let transport = if options.fixture.is_some() {
        None
    } else {
        match parallaxe_core::probe::HttpTransport::new(options.proxy.clone(), None) {
            Ok(transport) => Some(std::sync::Arc::new(transport)
                as std::sync::Arc<dyn parallaxe_core::probe::Transport>),
            Err(message) => {
                emit!(
                    writeln,
                    stderr,
                    "{}{message}",
                    translate(locale(), "cli.error_prefix", &[])
                );
                return ExitCode::from(1);
            }
        }
    };
    match runtime::run_check(registry, &options, transport, true, None).await {
        Ok(output) => {
            let report = &output.report;
            if args.trace || args.request_log {
                emit!(
                    writeln,
                    stderr,
                    "阶段记录：选中 {} / 已覆盖 {} / 网络请求 {} / 终止 {}",
                    report.coverage.selected_count,
                    report.coverage.covered.len(),
                    report.network_request_count,
                    report.termination.as_str()
                );
            }
            if args.json {
                emit!(writeln, stdout, "{}", reports::to_json(report));
            } else if args.jsonl {
                emit!(write, stdout, "{}", reports::to_jsonl(&output.events));
            } else if args.html {
                emit!(write, stdout, "{}", reports::to_html(report));
            } else if args.csv {
                emit!(write, stdout, "{}", reports::to_csv(report));
            } else if format.as_deref() == Some("md") {
                emit!(write, stdout, "{}", reports::to_markdown(report, locale()));
            } else if format.as_deref() == Some("txt") {
                emit!(write, stdout, "{}", reports::to_text(report, locale()));
            } else {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    reports::to_table_locale(report, locale())
                );
            }
            report_exit(report)
        }
        Err(error) => print_error(&error),
    }
}

fn run_targets(registry: &Registry, command: TargetsCommand) -> ExitCode {
    let TargetsCommand::List { group, json } = command;
    let selection = match &group {
        Some(group) => registry.select(None, std::slice::from_ref(group), &[], false),
        None => registry.select(Some("google-all"), &[], &[], false),
    };
    match selection {
        Ok(selection) => {
            let document = reports::targets_json(registry, &selection);
            if json {
                emit!(writeln, stdout, "{document}");
            } else {
                let value: serde_json::Value =
                    serde_json::from_str(&document).unwrap_or(serde_json::Value::Null);
                let items = value
                    .get("targets")
                    .and_then(|items| items.as_array())
                    .cloned()
                    .unwrap_or_default();
                for item in items {
                    emit!(
                        writeln,
                        stdout,
                        "{}\t{}\t{}={}\t{}={}",
                        item.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        item.get("group").and_then(|v| v.as_str()).unwrap_or(""),
                        translate(locale(), "targets.protected", &[]),
                        item.get("protected")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        translate(locale(), "targets.admission", &[]),
                        item.get("admission_status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => print_error(&error),
    }
}

async fn run_explain(registry: &Registry, args: ExplainArgs) -> ExitCode {
    if let Some(fixture) = &args.fixture {
        let options = CheckOptions {
            fixture: Some(fixture.clone()),
            ..Default::default()
        };
        return match runtime::run_check(registry, &options, None, false, None).await {
            Ok(output) => {
                let result = output.report.results.first();
                let document = serde_json::json!({
                    "schema": "parallaxe.explain/v1",
                    "fixture": fixture,
                    "mode": "FIXTURE",
                    "result": result,
                    "coverage": output.report.coverage,
                    "limitations": ["fixture 只用于离线演练，不是在线证据"],
                });
                if args.json {
                    emit!(
                        writeln,
                        stdout,
                        "{}",
                        serde_json::to_string_pretty(&document).unwrap_or_default()
                    );
                } else if let Some(result) = result {
                    let country = format!("{:?}", result.service_observed.observed_country);
                    emit!(
                        writeln,
                        stdout,
                        "{}",
                        translate(
                            locale(),
                            "explain.fixture_result",
                            &[
                                &result.target,
                                result.state.as_str(),
                                result.reason.as_deref().unwrap_or_default(),
                                &country,
                            ]
                        )
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => print_error(&error),
        };
    }
    match runtime::explain_target(registry, &args.target) {
        Ok(document) => {
            if args.json {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&document).unwrap_or_default()
                );
            } else {
                let family = document["target"]["family"].as_str().unwrap_or("");
                let group = document["target"]["group"].as_str().unwrap_or("");
                let url = document["target"]["url"].as_str().unwrap_or("");
                let surface = document["target"]["surface"].as_str().unwrap_or("");
                let protected = document["target"]["protected"].as_bool().unwrap_or(false);
                let status = document["target"]["admission_status"]
                    .as_str()
                    .unwrap_or("");
                let reason = document["target"]["reason"].as_str().unwrap_or("");
                let failure = document["target"]["failure"].as_str().unwrap_or("");
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.target", &[&args.target, family, group])
                );
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.url", &[url])
                );
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.surface", &[surface])
                );
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.protected", &[&protected.to_string()])
                );
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.admission", &[status, reason])
                );
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    translate(locale(), "explain.failure", &[failure])
                );
                if let Some(rules) = document["rules"].as_array() {
                    for rule in rules {
                        let version = rule["version"].as_u64().unwrap_or(0).to_string();
                        emit!(
                            writeln,
                            stdout,
                            "{}",
                            translate(
                                locale(),
                                "explain.rule",
                                &[
                                    rule["id"].as_str().unwrap_or(""),
                                    &version,
                                    rule["kind"].as_str().unwrap_or(""),
                                    rule["credibility"].as_str().unwrap_or("")
                                ]
                            )
                        );
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => print_error(&error),
    }
}

async fn run_selftest(registry: &Registry, args: SelftestArgs) -> ExitCode {
    if !args.offline {
        emit!(
            writeln,
            stderr,
            "{}{}",
            translate(locale(), "cli.error_prefix", &[]),
            translate(locale(), "cli.offline_only", &[])
        );
        return ExitCode::from(1);
    }
    match selftest::run(registry, args.suite.as_deref(), None).await {
        Ok(outcome) => {
            if args.json {
                emit!(writeln, stdout, "{}", outcome.to_json());
            } else {
                emit!(writeln, stdout, "{}", selftest::summary(&outcome));
                for case in &outcome.cases {
                    emit!(
                        writeln,
                        stdout,
                        "[{}] {} — {}",
                        if case.ok {
                            translate(locale(), "selftest.pass", &[])
                        } else {
                            translate(locale(), "selftest.fail", &[])
                        },
                        case.name,
                        case.detail
                    );
                }
            }
            ExitCode::from(outcome.exit_code())
        }
        Err(error) => print_error(&error),
    }
}

fn read_source_report(path: &std::path::Path) -> Result<Report, String> {
    let meta = std::fs::metadata(path).map_err(|_| "无法读取来源报告".to_string())?;
    if !meta.is_file() {
        return Err("来源报告必须是普通文件".to_string());
    }
    if meta.len() > 8 * 1024 * 1024 {
        return Err("来源报告超过 8 MiB 上限".to_string());
    }
    let text = std::fs::read_to_string(path).map_err(|_| "来源报告不是可读 UTF-8".to_string())?;
    serde_json::from_str::<Report>(&text).map_err(|_| "来源报告不是合法报告 JSON".to_string())
}

/// 共用：从用户选择与可选来源报告构建只读 Guard 预览。
fn build_guard_preview(
    registry: &Registry,
    target: &[String],
    source_report: &Option<PathBuf>,
) -> Result<(guard::GuardPreview, Option<Report>), String> {
    for id in target {
        if registry.get(id).is_none() {
            return Err(format!("未知目标 ID {id}"));
        }
    }
    let source = match source_report {
        Some(path) => Some(read_source_report(path)?),
        None => None,
    };
    let now = OffsetDateTime::now_utc();
    let recorded_at = parallaxe_core::state::format_time(now);
    let empty: Vec<parallaxe_core::ResultRecord> = Vec::new();
    let (results, source_run_id, interrupted) = match &source {
        Some(report) => (
            &report.results,
            Some(report.run_id.as_str()),
            report.termination != parallaxe_core::Termination::Complete,
        ),
        None => (&empty, None, false),
    };
    let preview = guard::plan_preview(
        registry,
        guard::GuardRequest {
            run_id: "run-guard-preview",
            source_run_id,
            targets: target,
            results,
            recorded_at,
            as_of: now.date(),
            source_interrupted: interrupted,
        },
    );
    Ok((preview, source))
}

fn run_guard(registry: &Registry, command: GuardCommand) -> ExitCode {
    match command {
        GuardCommand::Plan {
            target,
            source_report,
            json,
            ..
        } => run_guard_plan(registry, target, source_report, json),
        GuardCommand::Export {
            target,
            source_report,
            format,
            mode,
            out,
            acknowledge_overwrite,
            json,
        } => run_guard_export(
            registry,
            target,
            source_report,
            &format,
            &mode,
            &out,
            acknowledge_overwrite,
            json,
        ),
        GuardCommand::Block {
            target,
            source_report,
            resolve,
            ip,
            apply,
            confirm,
            audit_out,
            rule_tag,
            json,
        } => run_guard_block(
            registry,
            target,
            source_report,
            resolve,
            ip,
            apply,
            confirm,
            audit_out,
            &rule_tag,
            json,
        ),
        GuardCommand::Unblock {
            rule_tag,
            apply,
            json,
        } => run_guard_unblock(&rule_tag, apply, json),
        GuardCommand::Rollback { plan_id, json } => {
            let document = serde_json::json!({
                "schema": "parallaxe.guard-rollback/v1",
                "plan_id": plan_id,
                "status": "NOTHING_APPLIED",
                "applied": false,
                "writes_performed": 0,
                "rollback_required": false,
                "note": "本版本没有 apply 能力，也没有写入任何规则；没有可回滚的改动",
            });
            if json {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&document).unwrap_or_default()
                );
            } else {
                emit!(
                    writeln,
                    stdout,
                    "回滚预览：本版本未应用任何规则，无可回滚（写入 0）"
                );
            }
            ExitCode::SUCCESS
        }
    }
}

fn run_guard_plan(
    registry: &Registry,
    target: Vec<String>,
    source_report: Option<PathBuf>,
    json: bool,
) -> ExitCode {
    let (preview, _source) = match build_guard_preview(registry, &target, &source_report) {
        Ok(result) => result,
        Err(message) => {
            emit!(
                writeln,
                stderr,
                "{}{message}",
                translate(locale(), "cli.error_prefix", &[])
            );
            return ExitCode::from(1);
        }
    };
    let plan: &GuardPlan = &preview.plan;
    if json {
        let mut document = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
        if let Some(map) = document.as_object_mut() {
            map.insert(
                "audit".to_string(),
                serde_json::to_value(&preview.audit).unwrap_or(serde_json::Value::Null),
            );
        }
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    } else {
        emit!(writeln, stdout, "Guard 预览（只读、未落盘）");
        emit!(
            writeln,
            stdout,
            "候选：{}；可应用：{}；闭包完整：{}；已应用：{}",
            if plan.candidates.is_empty() {
                "无".to_string()
            } else {
                plan.candidates.join(", ")
            },
            plan.safe_to_apply,
            plan.closure_complete,
            plan.applied
        );
        emit!(
            writeln,
            stdout,
            "主原因：{}",
            plan.primary_reason.as_deref().unwrap_or("无")
        );
        for reason in &plan.reasons {
            emit!(writeln, stdout, "  原因：{reason}");
        }
        for domain in &plan.affected_domains {
            emit!(writeln, stdout, "  受影响域：{domain}");
        }
        for service in &plan.affected_services {
            emit!(writeln, stdout, "  受影响服务：{service}");
        }
        for path in &plan.affected_shared_paths {
            emit!(writeln, stdout, "  共享/保护路径：{path}");
        }
        emit!(
            writeln,
            stdout,
            "审计：{} / 写入 {} 条 / 回滚 {}",
            preview.audit.result,
            preview.audit.after.rules_written,
            preview.audit.rollback.status
        );
        for limitation in &plan.limitations {
            emit!(writeln, stdout, "  限制：{limitation}");
        }
    }
    ExitCode::from(reports::preview_exit_code(plan))
}

#[allow(clippy::too_many_arguments)]
fn run_guard_export(
    registry: &Registry,
    target: Vec<String>,
    source_report: Option<PathBuf>,
    format: &str,
    mode: &str,
    out: &std::path::Path,
    acknowledge_overwrite: bool,
    json: bool,
) -> ExitCode {
    let Some(rule_format) = guard_export::RuleFormat::parse(format) else {
        emit!(writeln, stderr, "错误：未知格式 {format}");
        return ExitCode::from(1);
    };
    let Some(export_mode) = guard_export::ExportMode::parse(mode) else {
        emit!(writeln, stderr, "错误：未知模式 {mode}");
        return ExitCode::from(1);
    };
    if export_mode == guard_export::ExportMode::Full && !acknowledge_overwrite {
        emit!(
            writeln,
            stderr,
            "错误：整包替换属于第二备选，不主动推荐。{} 确认无误后重新运行并加 --acknowledge-overwrite。",
            guard_export::FULL_MODE_WARNING
        );
        return ExitCode::from(1);
    }
    let (preview, source) = match build_guard_preview(registry, &target, &source_report) {
        Ok(result) => result,
        Err(message) => {
            emit!(
                writeln,
                stderr,
                "{}{message}",
                translate(locale(), "cli.error_prefix", &[])
            );
            return ExitCode::from(1);
        }
    };
    let forensic_pause = source
        .as_ref()
        .and_then(parallaxe_core::forensics::guard_pause_from_report);
    let bundle = match guard_export::export_rules(guard_export::ExportRequest {
        registry,
        plan: &preview.plan,
        format: rule_format,
        mode: export_mode,
        generated_at: parallaxe_core::state::format_time(OffsetDateTime::now_utc()),
        forensic_pause,
    }) {
        Ok(bundle) => bundle,
        Err(refusal) => {
            if json {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema": guard_export::EXPORT_SCHEMA,
                        "status": "REFUSED",
                        "code": refusal.code,
                        "detail": refusal.detail,
                        "applied": false,
                    }))
                    .unwrap_or_default()
                );
            } else {
                emit!(
                    writeln,
                    stdout,
                    "导出被闸门拒绝：{} — {}",
                    refusal.code,
                    refusal.detail
                );
            }
            return ExitCode::from(3);
        }
    };

    // 落盘：只写静态文件；任何一步失败都删除本次已写文件，不留半成品。
    if std::fs::create_dir_all(out).is_err() {
        emit!(writeln, stderr, "错误：无法创建输出目录 {}", out.display());
        return ExitCode::from(1);
    }
    let mut written: Vec<PathBuf> = Vec::new();
    for artifact in &bundle.artifacts {
        let path = out.join(&artifact.name);
        if std::fs::write(&path, &artifact.content).is_err() {
            for created in &written {
                let _ = std::fs::remove_file(created);
            }
            emit!(
                writeln,
                stderr,
                "错误：写入 {} 失败，已回滚本次导出",
                path.display()
            );
            return ExitCode::from(1);
        }
        written.push(path);
    }

    if json {
        let mut document = serde_json::to_value(&bundle).unwrap_or(serde_json::Value::Null);
        if let Some(map) = document.as_object_mut() {
            map.insert(
                "written".to_string(),
                serde_json::Value::Array(
                    written
                        .iter()
                        .map(|path| serde_json::Value::String(path.display().to_string()))
                        .collect(),
                ),
            );
        }
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    } else {
        emit!(
            writeln,
            stdout,
            "Guard 规则导出（{} / {}）：计划 {}",
            bundle.format,
            bundle.mode,
            bundle.plan_id
        );
        emit!(
            writeln,
            stdout,
            "域名（{} 个）：{}",
            bundle.domains.len(),
            bundle.domains.join(", ")
        );
        for path in &written {
            emit!(writeln, stdout, "  已写入：{}", path.display());
        }
        for warning in &bundle.warnings {
            emit!(writeln, stdout, "  警示：{warning}");
        }
        for artifact in &bundle.artifacts {
            if let Some(warning) = &artifact.warning {
                emit!(writeln, stdout, "  {}：{warning}", artifact.name);
            }
        }
        emit!(writeln, stdout, "{}", bundle.note);
    }
    ExitCode::SUCCESS
}

fn parse_ip_pairs(pairs: &[String]) -> Result<Vec<parallaxe_core::sysblock::ResolvedHost>, String> {
    use std::collections::BTreeMap;
    use std::net::IpAddr;
    let mut map: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();
    for pair in pairs {
        let (host, ip) = pair
            .split_once('=')
            .ok_or_else(|| format!("--ip 需要 host=ip 形式：{pair}"))?;
        let address: IpAddr = ip
            .parse()
            .map_err(|_| format!("--ip 的地址无法解析：{ip}"))?;
        map.entry(host.trim().to_ascii_lowercase())
            .or_default()
            .push(address);
    }
    Ok(map
        .into_iter()
        .map(|(fqdn, addresses)| parallaxe_core::sysblock::ResolvedHost { fqdn, addresses })
        .collect())
}

fn resolve_candidates(
    registry: &Registry,
    plan: &GuardPlan,
) -> Result<Vec<parallaxe_core::sysblock::ResolvedHost>, String> {
    use std::net::ToSocketAddrs;
    let mut hosts: Vec<parallaxe_core::sysblock::ResolvedHost> = Vec::new();
    for id in &plan.candidates {
        let Some(target) = registry.get(id) else {
            continue;
        };
        let host = target.host();
        let addresses = (host.as_str(), 443u16)
            .to_socket_addrs()
            .map_err(|_| format!("解析 {host} 失败：无法生成精确 IP 计划"))?
            .map(|address| address.ip())
            .collect::<Vec<_>>();
        hosts.push(parallaxe_core::sysblock::ResolvedHost {
            fqdn: host,
            addresses,
        });
    }
    Ok(hosts)
}

fn run_system_command(command: &str) -> Result<String, String> {
    #[cfg(windows)]
    let output = std::process::Command::new("cmd")
        .args(["/C", command])
        .output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .output();
    let output = output.map_err(|error| format!("无法启动命令：{error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

fn existing_rule_present(
    platform: parallaxe_core::sysblock::Platform,
    rule_tag: &str,
) -> Result<bool, String> {
    use parallaxe_core::sysblock::Platform;
    match platform {
        Platform::Windows => {
            let command =
                format!("netsh advfirewall firewall show rule name=\"parallaxe-block-{rule_tag}\"");
            match run_system_command(&command) {
                Ok(text) => Ok(text.contains("Rule Name") || text.contains("规则名称")),
                Err(text) => {
                    if text.contains("No rules match")
                        || text.contains("没有与指定条件相匹配的规则")
                    {
                        Ok(false)
                    } else {
                        Err(text)
                    }
                }
            }
        }
        Platform::Linux => {
            let command = format!("nft list table inet parallaxe_{rule_tag}");
            match run_system_command(&command) {
                Ok(_) => Ok(true),
                Err(text) => {
                    if text.contains("No such file or directory") || text.contains("does not exist")
                    {
                        Ok(false)
                    } else {
                        Err(text)
                    }
                }
            }
        }
    }
}

fn write_audit(path: &std::path::Path, document: &serde_json::Value) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(document).map_err(|_| "审计记录无法序列化".to_string())?;
    std::fs::write(path, text).map_err(|error| format!("写入审计记录失败：{error}"))
}

#[allow(clippy::too_many_arguments)]
fn run_guard_block(
    registry: &Registry,
    target: Vec<String>,
    source_report: Option<PathBuf>,
    resolve: bool,
    ip_pairs: Vec<String>,
    apply: bool,
    confirm: bool,
    audit_out: Option<PathBuf>,
    rule_tag: &str,
    json: bool,
) -> ExitCode {
    let Some(platform) = parallaxe_core::sysblock::Platform::current() else {
        emit!(
            writeln,
            stderr,
            "错误：系统阻断仅支持 Windows / Linux；本平台不提供系统写入。"
        );
        return ExitCode::from(1);
    };
    if apply && !confirm {
        emit!(
            writeln,
            stderr,
            "错误：--apply 需要同时给出 --confirm；不确认就不写系统。"
        );
        return ExitCode::from(1);
    }
    if resolve && !ip_pairs.is_empty() {
        emit!(writeln, stderr, "错误：--resolve 与 --ip 二选一");
        return ExitCode::from(1);
    }
    let (preview, _source) = match build_guard_preview(registry, &target, &source_report) {
        Ok(result) => result,
        Err(message) => {
            emit!(
                writeln,
                stderr,
                "{}{message}",
                translate(locale(), "cli.error_prefix", &[])
            );
            return ExitCode::from(1);
        }
    };
    let manual = match parse_ip_pairs(&ip_pairs) {
        Ok(items) => items,
        Err(message) => {
            emit!(writeln, stderr, "错误：{message}");
            return ExitCode::from(1);
        }
    };
    let mut resolved = manual;
    if resolve {
        match resolve_candidates(registry, &preview.plan) {
            Ok(items) => {
                for item in items {
                    if !resolved.iter().any(|existing| existing.fqdn == item.fqdn) {
                        resolved.push(item);
                    }
                }
            }
            Err(message) => {
                emit!(writeln, stderr, "错误：{message}");
                return ExitCode::from(1);
            }
        }
    }
    let sys_plan = match parallaxe_core::sysblock::build_plan(
        registry,
        &preview.plan,
        &resolved,
        platform,
        parallaxe_core::state::format_time(OffsetDateTime::now_utc()),
        rule_tag,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            if json {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
                        "status": "REFUSED",
                        "code": error.code,
                        "detail": error.detail,
                        "applied": false,
                    }))
                    .unwrap_or_default()
                );
            } else {
                emit!(
                    writeln,
                    stdout,
                    "系统阻断被闸门拒绝：{} — {}",
                    error.code,
                    error.detail
                );
            }
            return ExitCode::from(3);
        }
    };
    if !apply {
        if json {
            emit!(
                writeln,
                stdout,
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
                    "status": "DRY_RUN",
                    "applied": false,
                    "plan": sys_plan,
                }))
                .unwrap_or_default()
            );
        } else {
            emit!(writeln, stdout, "系统阻断计划（DRY RUN，未写系统）");
            emit!(
                writeln,
                stdout,
                "平台：{} / 引擎：{} / 标签：{} / 需要管理员：{}",
                sys_plan.platform,
                sys_plan.engine,
                sys_plan.rule_tag,
                sys_plan.requires_admin
            );
            for entry in &sys_plan.entries {
                emit!(
                    writeln,
                    stdout,
                    "  {} → {} ({})",
                    entry.fqdn,
                    entry.ip,
                    entry.family
                );
            }
            for command in &sys_plan.apply_commands {
                emit!(writeln, stdout, "  将执行：{command}");
            }
            for command in &sys_plan.rollback_commands {
                emit!(writeln, stdout, "  回滚：{command}");
            }
            for warning in &sys_plan.warnings {
                emit!(writeln, stdout, "  警示：{warning}");
            }
            emit!(writeln, stdout, "加 --apply --confirm 才会真正写入系统。");
        }
        return ExitCode::SUCCESS;
    }

    match existing_rule_present(platform, rule_tag) {
        Ok(true) => {
            emit!(
                writeln,
                stderr,
                "错误：已存在同名规则 {}；请先 guard unblock --rule-tag {} 再重新应用。",
                sys_plan.rule_tag,
                rule_tag
            );
            return ExitCode::from(1);
        }
        Ok(false) => {}
        Err(message) => {
            emit!(writeln, stderr, "错误：无法检查既有规则：{message}");
            return ExitCode::from(1);
        }
    }

    let created_at = sys_plan.created_at.clone();
    let audit_path =
        audit_out.unwrap_or_else(|| PathBuf::from(format!("sysblock-{rule_tag}.json")));
    for command in &sys_plan.apply_commands {
        if let Err(message) = run_system_command(command) {
            let mut rollback_failed = Vec::new();
            for rollback in &sys_plan.rollback_commands {
                if let Err(error) = run_system_command(rollback) {
                    rollback_failed.push(error);
                }
            }
            let document = serde_json::json!({
                "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
                "status": "APPLY_FAILED_ROLLED_BACK",
                "created_at": created_at,
                "rule_tag": sys_plan.rule_tag,
                "error": message,
                "rollback_failed": rollback_failed,
                "entries": sys_plan.entries,
            });
            if let Err(error) = write_audit(&audit_path, &document) {
                emit!(writeln, stderr, "错误：{error}");
            }
            emit!(
                writeln,
                stderr,
                "错误：写入系统失败，已尝试回滚。审计：{}",
                audit_path.display()
            );
            return ExitCode::from(1);
        }
    }
    let document = serde_json::json!({
        "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
        "status": "APPLIED",
        "created_at": created_at,
        "rule_tag": sys_plan.rule_tag,
        "platform": sys_plan.platform,
        "engine": sys_plan.engine,
        "entries": sys_plan.entries,
        "rollback_commands": sys_plan.rollback_commands,
        "zero_residue": true,
    });
    if let Err(error) = write_audit(&audit_path, &document) {
        emit!(writeln, stderr, "错误：{error}");
        return ExitCode::from(1);
    }
    if json {
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    } else {
        emit!(
            writeln,
            stdout,
            "已写入系统阻断：{}（{} 条 IP）。撤销：guard unblock --rule-tag {} --apply",
            sys_plan.rule_tag,
            sys_plan.entries.len(),
            rule_tag
        );
        emit!(writeln, stdout, "审计：{}", audit_path.display());
    }
    ExitCode::SUCCESS
}

fn run_guard_unblock(rule_tag: &str, apply: bool, json: bool) -> ExitCode {
    let Some(platform) = parallaxe_core::sysblock::Platform::current() else {
        emit!(
            writeln,
            stderr,
            "错误：系统阻断仅支持 Windows / Linux；本平台没有可撤销的规则。"
        );
        return ExitCode::from(1);
    };
    let commands = match parallaxe_core::sysblock::unblock_commands(platform, rule_tag) {
        Ok(commands) => commands,
        Err(error) => {
            emit!(writeln, stderr, "错误：{} — {}", error.code, error.detail);
            return ExitCode::from(1);
        }
    };
    if !apply {
        if json {
            emit!(
                writeln,
                stdout,
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
                    "status": "DRY_RUN",
                    "commands": commands,
                }))
                .unwrap_or_default()
            );
        } else {
            for command in &commands {
                emit!(writeln, stdout, "将执行：{command}");
            }
            emit!(writeln, stdout, "加 --apply 才会真正撤销。");
        }
        return ExitCode::SUCCESS;
    }
    let mut status = "REVOKED";
    for command in &commands {
        if let Err(message) = run_system_command(command) {
            if message.contains("No rules match")
                || message.contains("没有与指定条件相匹配的规则")
                || message.contains("No such file or directory")
                || message.contains("does not exist")
            {
                status = "ALREADY_ABSENT";
                continue;
            }
            emit!(writeln, stderr, "错误：撤销失败：{message}");
            return ExitCode::from(1);
        }
    }
    if json {
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": parallaxe_core::sysblock::SYSBLOCK_SCHEMA,
                "status": status,
                "rule_tag": format!("parallaxe-block-{rule_tag}"),
                "zero_residue": true,
            }))
            .unwrap_or_default()
        );
    } else {
        emit!(
            writeln,
            stdout,
            "撤销完成：parallaxe-block-{}（{}，零残留）",
            rule_tag,
            status
        );
    }
    ExitCode::SUCCESS
}

async fn run_link(registry: &Registry, command: LinkCommand) -> ExitCode {
    let LinkCommand::Explain {
        profile,
        group,
        target,
        fixture,
        resolve,
        show_ip,
        json,
    } = command;
    let now = OffsetDateTime::now_utc();
    let generated_at = parallaxe_core::state::format_time(now);
    let mut report = if let Some(case) = &fixture {
        let options = CheckOptions {
            fixture: Some(case.clone()),
            ..Default::default()
        };
        match runtime::run_check(registry, &options, None, false, None).await {
            Ok(output) => linkcheck::analyze(registry, &output.report, generated_at),
            Err(error) => return print_error(&error),
        }
    } else {
        let selection = match registry.select(profile.as_deref(), &group, &target, false) {
            Ok(selection) => selection,
            Err(error) => return print_error(&error),
        };
        linkcheck::analyze_selection(registry, &selection.ids, generated_at)
    };
    if resolve {
        if let Err(error) = linkcheck::probe_resolver(registry, &mut report, show_ip, 4_000).await {
            return print_error(&error);
        }
    }
    if json {
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        emit!(
            writeln,
            stdout,
            "链路体检（只读 / 端口扫描禁止 / QUIC 与 DNS 泄漏未测）"
        );
        emit!(
            writeln,
            stdout,
            "目标\tresolver\t连接边缘\tHTTP 出口\t地址族"
        );
        for row in &report.rows {
            let family = format!(
                "v4={} v6={}{}",
                row.address_family.v4,
                row.address_family.v6,
                row.address_family
                    .diagnostic
                    .as_deref()
                    .map(|text| format!(" / {text}"))
                    .unwrap_or_default()
            );
            emit!(
                writeln,
                stdout,
                "{}\t{}\t{}\t{}\t{}",
                row.target,
                row.resolver.status,
                row.connection_edge.status,
                row.http_egress
                    .observed_country
                    .as_deref()
                    .unwrap_or(row.http_egress.status.as_str()),
                family
            );
        }
        emit!(
            writeln,
            stdout,
            "DNS 查询 {} 次；HTTP 请求 {} 次",
            report.dns_query_count,
            report.http_request_count
        );
    }
    ExitCode::SUCCESS
}

fn run_isolate(command: IsolateCommand) -> ExitCode {
    let (action, json, targets) = match command {
        IsolateCommand::Plan {
            protect,
            defer,
            json,
        } => (
            "plan",
            json,
            protect.into_iter().chain(defer).collect::<Vec<_>>(),
        ),
        IsolateCommand::Explain { target, json } => ("explain", json, vec![target]),
    };
    let document = serde_json::json!({
        "schema": "parallaxe.isolate/v1",
        "action": action,
        "status": "NO_EXISTING_PATH",
        "targets": targets,
        "paths": [],
        "limitations": [
            "只解释已有路径，不创建、切换或验证新代理",
            "独立路径必须由用户自行提供并证明"
        ],
        "selected_by": "USER",
    });
    if json {
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&document).unwrap_or_default()
        );
    } else {
        emit!(
            writeln,
            stdout,
            "隔离{action}：没有可解释的既有独立路径（未创建、未切换）"
        );
        if let Some(limitations) = document["limitations"].as_array() {
            for limitation in limitations {
                emit!(
                    writeln,
                    stdout,
                    "  限制：{}",
                    limitation.as_str().unwrap_or_default()
                );
            }
        }
    }
    ExitCode::SUCCESS
}

async fn run(cli: Cli) -> ExitCode {
    let registry = match load_registry(&cli) {
        Ok(registry) => registry,
        Err(error) => return print_error(&error),
    };
    if cli.selftest {
        return run_selftest(
            &registry,
            SelftestArgs {
                offline: true,
                json: false,
                suite: None,
            },
        )
        .await;
    }
    let Some(command) = cli.command else {
        emit!(
            writeln,
            stderr,
            "{}{}",
            translate(locale(), "cli.error_prefix", &[]),
            translate(locale(), "cli.missing_command", &[])
        );
        return ExitCode::from(1);
    };
    match command {
        Command::Check(args) => run_check(&registry, args).await,
        Command::Explain(args) => run_explain(&registry, args).await,
        Command::Targets { command } => run_targets(&registry, command),
        Command::Selftest(args) => run_selftest(&registry, args).await,
        Command::Guard { command } => run_guard(&registry, command),
        Command::Isolate { command } => run_isolate(command),
        Command::Priorities {
            command: PrioritiesCommand::List { json },
        } => {
            let document = runtime::priorities_list(&registry);
            if json {
                emit!(
                    writeln,
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&document).unwrap_or_default()
                );
            } else {
                emit!(
                    writeln,
                    stdout,
                    "未提供用户显式优先级（示例文件不加载为用户状态）"
                );
            }
            ExitCode::SUCCESS
        }
        Command::Dependencies {
            command: DependenciesCommand::Explain { target, json },
        } => match runtime::explain_dependencies(&registry, &target) {
            Ok(document) => {
                if json {
                    emit!(
                        writeln,
                        stdout,
                        "{}",
                        serde_json::to_string_pretty(&document).unwrap_or_default()
                    );
                } else {
                    emit!(
                        writeln,
                        stdout,
                        "目标 {}（主机 {}）依赖边：",
                        target,
                        document["host"].as_str().unwrap_or("")
                    );
                    if let Some(edges) = document["edges"].as_array() {
                        if edges.is_empty() {
                            emit!(writeln, stdout, "  无显式边；无来源不等于专属性");
                        }
                        for edge in edges {
                            emit!(
                                writeln,
                                stdout,
                                "  · {} → {} [{}]",
                                edge["src"].as_str().unwrap_or(""),
                                edge["dst"].as_str().unwrap_or(""),
                                edge["kind"].as_str().unwrap_or("")
                            );
                        }
                    }
                }
                ExitCode::SUCCESS
            }
            Err(error) => print_error(&error),
        },
        Command::Link { command } => run_link(&registry, command).await,
        Command::Agent { stdio } => {
            if !stdio {
                emit!(
                    writeln,
                    stderr,
                    "{}{}",
                    translate(locale(), "cli.error_prefix", &[]),
                    translate(locale(), "cli.agent_stdio_only", &[])
                );
                return ExitCode::from(1);
            }
            let code = parallaxe_agent::run_stdio(registry, locale()).await;
            ExitCode::from(code)
        }
        Command::Gui => run_gui_command(registry),
    }
}

fn run_gui_command(registry: Registry) -> ExitCode {
    #[cfg(feature = "gui")]
    {
        match parallaxe_gui::run_gui(registry) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                emit!(
                    writeln,
                    stderr,
                    "{}{message}",
                    translate(locale(), "cli.error_prefix", &[])
                );
                ExitCode::from(1)
            }
        }
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = registry;
        emit!(
            writeln,
            stderr,
            "{}{}",
            translate(locale(), "cli.error_prefix", &[]),
            translate(locale(), "cli.gui_unavailable", &[])
        );
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    // 先预扫描 --locale，再构建帮助文本；子命令 help 仍以中文为准（本轮只做机制与主帮助）。
    let hinted = Locale::detect_from_args(std::env::args());
    let command = Cli::command().about(translate(hinted, "cli.about", &[]));
    let matches = match command.try_get_matches() {
        Ok(matches) => matches,
        Err(error) => {
            let code = match error.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    emit!(write, stdout, "{error}");
                    0
                }
                _ => {
                    emit!(write, stderr, "{error}");
                    1
                }
            };
            return ExitCode::from(code);
        }
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => {
            emit!(write, stderr, "{error}");
            return ExitCode::from(1);
        }
    };
    let _ = LOCALE.set(Locale::parse(&cli.locale).unwrap_or_default());
    // GUI 必须在操作系统主线程直接启动，避免嵌套 tokio 运行时或跨平台窗口事件循环冲突。
    if !cli.selftest && matches!(cli.command, Some(Command::Gui)) {
        return match load_registry(&cli) {
            Ok(registry) => run_gui_command(registry),
            Err(error) => print_error(&error),
        };
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            emit!(
                writeln,
                stderr,
                "{}{}",
                translate(locale(), "cli.error_prefix", &[]),
                translate(locale(), "cli.runtime_unavailable", &[])
            );
            return ExitCode::from(1);
        }
    };
    runtime.block_on(run(cli))
}
