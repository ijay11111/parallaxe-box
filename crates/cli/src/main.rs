//! `parallaxe` 命令行入口：clap 解析 → 共享 core → 投影输出与退出码。

mod guard_backend;
mod guard_journal;
#[cfg(target_os = "linux")]
mod nft_generation;
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
    /// Inspect and explicitly approve local plugins.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// 只读探测（默认空许可：选中但不发包）
    Check(Box<CheckArgs>),
    /// 解释目标目录或离线样本
    Explain(ExplainArgs),
    /// 目录操作
    Targets {
        #[command(subcommand)]
        command: TargetsCommand,
    },
    /// 输出编译期构建标记（不加载目录覆盖层）
    BuildInfo {
        #[arg(long)]
        json: bool,
    },
    /// 离线自检（不触网）
    Selftest(SelftestArgs),
    /// Guard 预览、手动规则导出与只读审计
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
    /// AI 授权桥（只读状态；不开启、不代用户确认）
    Ai {
        #[command(subcommand)]
        command: AiCommand,
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Subcommand)]
enum PluginCommand {
    List {
        #[arg(long)]
        settings: PathBuf,
        #[arg(long)]
        detailed: bool,
    },
    Verify {
        directory: PathBuf,
    },
    Enable {
        directory: PathBuf,
        #[arg(long)]
        settings: PathBuf,
        #[arg(long)]
        approve_sha256: String,
    },
    Disable {
        id: String,
        #[arg(long)]
        settings: PathBuf,
    },
    Repair {
        #[arg(long)]
        settings: PathBuf,
    },
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_plugin(command: &PluginCommand) -> ExitCode {
    use parallaxe_core::{plugins, settings};
    let result: Result<String, String> = (|| match command {
        PluginCommand::List {
            settings: path,
            detailed,
        } => {
            let config = settings::load(path)?;
            if !*detailed {
                serde_json::to_string_pretty(&config.plugins).map_err(|e| e.to_string())
            } else {
                let entries: Vec<serde_json::Value> = config
                    .plugins
                    .iter()
                    .map(|(id, sha256)| match plugins::read_enabled(&config, id) {
                        Ok((_, manifest)) => serde_json::json!({
                            "id": id,
                            "name": manifest.name,
                            "version": manifest.version,
                            "author": manifest.author,
                            "license": manifest.license,
                            "permissions": manifest.permissions,
                            "sha256": sha256,
                            "status": "verified",
                        }),
                        Err(err) => serde_json::json!({
                            "id": id,
                            "sha256": sha256,
                            "status": "error",
                            "error": err,
                        }),
                    })
                    .collect();
                serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())
            }
        }
        PluginCommand::Verify { directory } => {
            let (bytes, manifest) = plugins::read_directory(directory)?;
            serde_json::to_string_pretty(
                &serde_json::json!({"sha256": plugins::digest(&bytes), "manifest": manifest}),
            )
            .map_err(|e| e.to_string())
        }
        PluginCommand::Enable {
            directory,
            settings: path,
            approve_sha256,
        } => {
            let (bytes, manifest) = plugins::read_directory(directory)?;
            let normalized = plugins::normalize_sha256(approve_sha256)?;
            plugins::verified(&bytes, &normalized)?;
            let directory = std::fs::canonicalize(directory).map_err(|e| e.to_string())?;
            let directory = directory
                .to_str()
                .ok_or("plugin path must be valid UTF-8")?
                .to_string();
            let id = manifest.id;
            settings::update(path, |config| {
                config.plugins.insert(id.clone(), normalized);
                config.plugin_directories.insert(id.clone(), directory);
                Ok(())
            })?;
            Ok(format!("enabled {id}"))
        }
        PluginCommand::Disable { id, settings: path } => {
            settings::update(path, |config| {
                config.plugins.remove(id);
                config.plugin_directories.remove(id);
                Ok(())
            })?;
            Ok(format!("disabled {id}"))
        }
        PluginCommand::Repair { settings: path } => {
            let recovered = settings::repair(path)?;
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "repaired",
                "enabled_plugins": recovered.plugins.len(),
            }))
            .map_err(|e| e.to_string())
        }
    })();
    match result {
        Ok(message) => {
            emit!(writeln, stdout, "{}", message);
            ExitCode::SUCCESS
        }
        Err(error) => {
            emit!(writeln, stderr, "{}", error);
            ExitCode::from(1)
        }
    }
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
    /// 代理模式：auto（默认，自动探测）/ none（强制直连）/ http / socks / pac
    #[arg(long = "proxy-mode", value_parser = ["auto", "none", "http", "socks", "pac"])]
    proxy_mode: Option<String>,
    /// PAC 网址（仅 --proxy-mode pac 使用；获取失败会回退自动选择并注明）
    #[arg(long = "pac-url")]
    pac_url: Option<String>,
    /// 最大检测时长（毫秒）；0 = 不限时（默认按 profile，simple 为不限时）
    #[arg(long)]
    timeout: Option<u64>,
    #[arg(long = "max-concurrency")]
    max_concurrency: Option<u32>,
    /// 同 host / 节点 / 协议的发送间隔（毫秒，0–10000；默认 3500）
    #[arg(long = "min-interval-ms")]
    min_interval_ms: Option<u64>,
    /// 跨目标（不同 host）检验之间的间隔（毫秒，0–10000；默认 0 不额外延迟）
    #[arg(long = "inter-target-interval-ms")]
    inter_target_interval_ms: Option<u64>,
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
    /// 本地 IP 登记库，每行 CIDR,国家码,ASN
    #[arg(long)]
    address_book: Option<PathBuf>,
    /// 与直连对端登记国家比较
    #[arg(long, value_parser = parse_peer_country)]
    expect_peer_country: Option<String>,
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
        /// 请显式指定导出文件夹（不存在的目录会被创建）
        #[arg(long)]
        out_dir: PathBuf,
        /// 整包替换必须显式确认：会覆盖原有分流规则，请先备份
        #[arg(long = "acknowledge-overwrite")]
        acknowledge_overwrite: bool,
        #[arg(long)]
        json: bool,
    },
    /// 系统级阻断：仅 Win/Linux，显式开启、可一键撤销（需管理员/root）
    #[command(
        hide = true,
        override_help = "测试版不含系统级阻断。请用 guard plan 预览、guard status 查看审计。"
    )]
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
    /// 读取本机写前审计和恢复指引（不发包）。
    Status {
        #[arg(long = "rule-tag", default_value = "default")]
        rule_tag: String,
        #[arg(long)]
        json: bool,
    },
    /// 核对并撤销本次运行拥有的规则
    #[command(
        hide = true,
        override_help = "测试版不含系统级阻断。请用 guard plan 预览、guard status 查看审计。"
    )]
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
enum AiCommand {
    /// 打印 GUI 授权桥状态与待确认请求（只读，绝不代替用户确认）
    Status {
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

fn parse_peer_country(value: &str) -> Result<String, String> {
    parallaxe_core::normalize_country(value).ok_or_else(|| "invalid country code".into())
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
    let proxy_mode = match args.proxy_mode.as_deref() {
        None => parallaxe_core::ProxyMode::Auto,
        Some(value) => match parallaxe_core::ProxyMode::parse(value) {
            Some(mode) => mode,
            None => {
                emit!(
                    writeln,
                    stderr,
                    "错误：--proxy-mode 只接受 auto/none/http/socks/pac"
                );
                return ExitCode::from(1);
            }
        },
    };
    if proxy_mode == parallaxe_core::ProxyMode::Socks {
        emit!(
            writeln,
            stderr,
            "错误：当前构建不支持 SOCKS 传输；请改用 --proxy-mode http / pac / none（不会静默直连）"
        );
        return ExitCode::from(1);
    }
    if proxy_mode == parallaxe_core::ProxyMode::Http && args.proxy.is_none() {
        emit!(
            writeln,
            stderr,
            "错误：--proxy-mode http 需要同时提供 --proxy http://host:port"
        );
        return ExitCode::from(1);
    }
    if proxy_mode == parallaxe_core::ProxyMode::Pac && args.pac_url.is_none() {
        emit!(
            writeln,
            stderr,
            "错误：--proxy-mode pac 需要 --pac-url http(s)://…/proxy.pac"
        );
        return ExitCode::from(1);
    }
    if args.fixture.is_some()
        && (proxy_mode != parallaxe_core::ProxyMode::Auto || args.pac_url.is_some())
    {
        emit!(writeln, stderr, "错误：--fixture 与代理模式/PAC 互斥");
        return ExitCode::from(1);
    }
    let mut options = CheckOptions {
        profile: args.profile.clone(),
        groups: args.group.clone(),
        targets: args.target.clone(),
        full: args.full,
        expect: args.expect.clone(),
        fixture: args.fixture.clone(),
        state_dir: args.state_dir.clone(),
        proxy: args.proxy.clone(),
        proxy_mode,
        pac_url: args.pac_url.clone(),
        proxy_path: None,
        proxy_note: None,
        interface: args.interface.clone(),
        ipv4: args.ipv4,
        fast: args.fast,
        timeout_ms: args.timeout,
        max_concurrency: args.max_concurrency,
        min_interval_ms: args.min_interval_ms,
        inter_target_interval_ms: args.inter_target_interval_ms,
        batch_size: args.batch_size,
        batch_cooldown_ms: args.batch_cooldown_ms,
        node_retest: args.node_retest,
        plan: args.plan,
        confirm_budget: args.confirm_budget,
        node_sweep: args.node_sweep,
        qos_control: None,
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
    let book = match &args.address_book {
        Some(path) => match std::fs::read_to_string(path)
            .map_err(|_| "cannot read address book")
            .and_then(|s| parallaxe_core::destination::AddressBook::parse(&s))
        {
            Ok(book) => book,
            Err(message) => return print_error(&ConfigError::field("--address-book", message)),
        },
        None => Default::default(),
    };
    let expected_peer_country = match &args.expect_peer_country {
        Some(raw) => match parallaxe_core::normalize_country(raw) {
            Some(code) => Some(code),
            None => {
                return print_error(&ConfigError::field(
                    "--expect-peer-country",
                    "需要两位或三位国家码（如 US、CN、JPN）",
                ))
            }
        },
        None => None,
    };
    let transport = if options.fixture.is_some() {
        None
    } else {
        // 有界代理探测/模式求值：在产生目标 HTTP 请求前完成；`--plan` 已在上面返回。
        let prep = match runtime::prepare_network(registry, &options).await {
            Ok(prep) => prep,
            Err(error) => return print_error(&error),
        };
        if !prep.note.is_empty() {
            emit!(writeln, stderr, "代理路径：{}", prep.note);
        }
        options.proxy_path = Some(prep.path_label);
        options.proxy_note = Some(prep.note);
        match parallaxe_core::probe::HttpTransport::new(prep.default_proxy, None) {
            Ok(transport) => Some(std::sync::Arc::new(
                transport
                    .with_routes(prep.routes)
                    .with_address_context(book, expected_peer_country),
            )
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
        Ok(mut output) => {
            for result in &mut output.report.results {
                result.destination.set_show_ip(args.show_ip);
            }
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
            if format.as_deref() == Some("json") {
                emit!(writeln, stdout, "{}", reports::to_json(report));
            } else if format.as_deref() == Some("jsonl") {
                emit!(write, stdout, "{}", reports::to_jsonl(&output.events));
            } else if format.as_deref() == Some("html") {
                emit!(write, stdout, "{}", reports::to_html(report));
            } else if format.as_deref() == Some("csv") {
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
    // 发布策略独立于目录是否有候选，优先于 DNS、审计目录和系统后端。
    if !parallaxe_core::sysblock::SYSTEM_BLOCK_ENABLED {
        match &command {
            GuardCommand::Block { json, .. }
            | GuardCommand::Unblock {
                apply: true, json, ..
            } => {
                return guard_output(
                    *json,
                    Err(guard_journal::Failure(
                        "SYSBLOCK_BETA_DISABLED",
                        "测试版不含系统级阻断".into(),
                    )),
                );
            }
            GuardCommand::Unblock { rule_tag, json, .. } => {
                return guard_action(*json, |p, dir| guard_journal::status(dir, rule_tag, p));
            }
            _ => (),
        }
    }
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
            out_dir,
            acknowledge_overwrite,
            json,
        } => run_guard_export(
            registry,
            target,
            source_report,
            &format,
            &mode,
            &out_dir,
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
        GuardCommand::Status { rule_tag, json } => {
            guard_action(json, |p, dir| guard_journal::status(dir, &rule_tag, p))
        }
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
        for code in &bundle.warning_codes {
            emit!(writeln, stdout, "{code}");
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

fn guard_output(json: bool, result: guard_journal::Result<serde_json::Value>) -> ExitCode {
    let (mut doc, code) = match result {
        Ok(doc) => (doc, 0),
        Err(e) => (
            serde_json::json!({"status":"REFUSED","code":e.0,"detail":e.1}),
            3,
        ),
    };
    if doc["code"] == "SYSBLOCK_BETA_DISABLED" {
        doc["applied"] = serde_json::json!(false);
    }
    if doc.get("schema").is_none() {
        doc["schema"] = serde_json::json!(parallaxe_core::sysblock::SYSBLOCK_SCHEMA);
    }
    if json {
        emit!(
            writeln,
            stdout,
            "{}",
            serde_json::to_string_pretty(&doc).unwrap_or_default()
        );
    } else {
        if doc["code"] == "SYSBLOCK_NO_AUDIT" {
            emit!(writeln,stdout,"DRY_RUN · parallaxe-block-{}：缺少本次运行的审计记录。即使加 --apply 也会拒绝删除（UNBLOCK_FOREIGN_RULE）。",doc["rule_tag"].as_str().unwrap_or_default());
        } else {
            emit!(
                writeln,
                stdout,
                "{}",
                serde_json::to_string_pretty(&doc).unwrap_or_default()
            );
        }
    }
    ExitCode::from(code)
}
fn guard_action(
    json: bool,
    op: impl FnOnce(
        parallaxe_core::sysblock::Platform,
        &std::path::Path,
    ) -> guard_journal::Result<serde_json::Value>,
) -> ExitCode {
    let Some(p) = parallaxe_core::sysblock::Platform::current() else {
        return guard_output(
            json,
            Err(guard_journal::Failure(
                "SYSBLOCK_UNSUPPORTED",
                "系统阻断仅支持 Windows / Linux".into(),
            )),
        );
    };
    guard_output(json, guard_journal::directory().and_then(|dir| op(p, &dir)))
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
    let dir = match guard_journal::directory() {
        Ok(dir) => dir,
        Err(e) => return guard_output(json, Err(e)),
    };
    match guard_journal::pending(&dir, rule_tag, platform) {
        Ok(None) => (),
        Ok(Some(doc)) => {
            guard_output(json, Ok(doc));
            return ExitCode::from(3);
        }
        Err(e) => return guard_output(json, Err(e)),
    }
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
    let now = parallaxe_core::state::format_time(OffsetDateTime::now_utc());
    let plan = &preview.plan;
    let result = if resolve {
        parallaxe_core::sysblock::resolve_plan(registry, plan, platform, now, rule_tag)
    } else {
        parallaxe_core::sysblock::candidate_hosts(registry, plan, rule_tag).and_then(|_| {
            if apply {
                return Err(parallaxe_core::sysblock::SysBlockError {
                    code: "SYSBLOCK_RESOLUTION_REQUIRED",
                    detail: "实际写入需要 --resolve，手工 IP 仅供预览。".into(),
                });
            }
            parallaxe_core::sysblock::build_plan(registry, plan, &manual, platform, now, rule_tag)
        })
    };
    let sys_plan = match result {
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

    guard_output(
        json,
        guard_journal::block(
            &dir,
            platform,
            rule_tag,
            &sys_plan,
            audit_out.as_deref(),
            &guard_backend::System(platform),
        ),
    )
}

fn run_guard_unblock(tag: &str, apply: bool, json: bool) -> ExitCode {
    guard_action(json, |p, dir| {
        guard_journal::unblock(dir, p, tag, apply, &guard_backend::System(p))
    })
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
            Ok(output) => {
                linkcheck::analyze_with_ip(registry, &output.report, generated_at, show_ip)
            }
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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if !cli.selftest {
        if let Some(Command::Plugin { command }) = &cli.command {
            return run_plugin(command);
        }
    }
    if let Some(Command::BuildInfo { json }) = &cli.command {
        let info = parallaxe_core::distribution::build_info_json();
        if *json {
            let pretty = serde_json::to_string_pretty(
                &serde_json::from_str::<serde_json::Value>(&info).unwrap_or_default(),
            )
            .unwrap_or(info);
            emit!(writeln, stdout, "{pretty}");
        } else {
            emit!(
                writeln,
                stdout,
                "{}",
                parallaxe_core::distribution::BUILD_MARKER
            );
        }
        return ExitCode::SUCCESS;
    }
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
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        Command::Plugin { command } => run_plugin(&command),
        Command::Check(args) => run_check(&registry, *args).await,
        Command::Explain(args) => run_explain(&registry, args).await,
        Command::Targets { command } => run_targets(&registry, command),
        Command::BuildInfo { .. } => unreachable!("build-info is handled before registry loading"),
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
        Command::Ai { command } => run_ai(command),
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

fn run_ai(command: AiCommand) -> ExitCode {
    use parallaxe_core::ai_bridge::BridgePaths;
    let paths = BridgePaths::resolve();
    let status = paths.read_status();
    let requests = paths.read_requests();
    let live = status.as_ref().is_some_and(|status| {
        time::OffsetDateTime::parse(
            &status.updated_at,
            &time::format_description::well_known::Rfc3339,
        )
        .is_ok_and(|at| time::OffsetDateTime::now_utc() - at < time::Duration::seconds(120))
    });
    match command {
        AiCommand::Status { json } => {
            if json {
                let doc = serde_json::json!({
                    "schema": "parallaxe.ai.bridge.status/v1",
                    "available": live,
                    "host": status.as_ref().map(|s| s.host.clone()),
                    "autonomous_enabled": status
                        .as_ref()
                        .is_some_and(|s| s.autonomous_enabled && live),
                    "session_only": true,
                    "imported_targets": status
                        .as_ref()
                        .map(|s| s.imported_targets.clone())
                        .unwrap_or_default(),
                    "requests": requests.iter().map(|request| serde_json::json!({
                        "request_id": request.request_id,
                        "scope": request.scope.label(),
                        "min_interval_ms": request.min_interval_ms,
                        "batch_size": request.batch_size,
                        "expires_at": request.expires_at,
                    })).collect::<Vec<_>>(),
                });
                emit!(writeln, stdout, "{}", doc);
            } else if !live {
                emit!(
                    writeln,
                    stdout,
                    "AI 授权桥：未检测到 GUI（默认零发包）。请先打开 Parallaxe box 桌面程序。"
                );
            } else {
                let host = status.as_ref().map(|s| s.host.as_str()).unwrap_or("-");
                let autonomous = status.as_ref().is_some_and(|s| s.autonomous_enabled);
                let imported = status
                    .as_ref()
                    .map(|s| s.imported_targets.join(", "))
                    .unwrap_or_default();
                emit!(writeln, stdout, "AI 授权桥：已连接（host={host}，会话级）");
                emit!(
                    writeln,
                    stdout,
                    "自主检测：{}（仅用户在 GUI 设置中打开；关闭不落盘）",
                    if autonomous {
                        "已开启"
                    } else {
                        "关闭（默认）"
                    }
                );
                emit!(
                    writeln,
                    stdout,
                    "可真实发包的导入目标：{}",
                    if imported.is_empty() {
                        "无（内置目录默认零发包）"
                    } else {
                        &imported
                    }
                );
                emit!(writeln, stdout, "待确认请求：{}", requests.len());
                for request in &requests {
                    emit!(
                        writeln,
                        stdout,
                        "  {} · {} · 间隔 {}ms · 批次 {} · 过期 {}",
                        request.request_id,
                        request.scope.label(),
                        request.min_interval_ms,
                        request.batch_size,
                        request.expires_at
                    );
                }
            }
            ExitCode::SUCCESS
        }
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
                    if error.kind() == clap::error::ErrorKind::MissingRequiredArgument
                        && error
                            .get(clap::error::ContextKind::InvalidArg)
                            .is_some_and(|arg| arg.to_string().starts_with("--out-dir "))
                    {
                        emit!(writeln, stderr, "请显式指定导出文件夹：--out-dir <DIR>");
                    }
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
