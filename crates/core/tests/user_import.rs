//! 自定义网址与检查方式导入契约测试：
//! 覆盖 URL 校验、批量导入、9 种 reader kind 校验、危险 regex (ReDoS) 拒绝、
//! 两段责任声明勾选门槛、未授权零发包、一次性会话许可仅单次有效、撤销后零发包、
//! 禁止提升内置目标、`--full` 不自动包含、以及报告 `origin/consent_version/imported_at/rule_digest` 留痕。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parallaxe_core::contracts::{
    Attempt, AttemptFamily, Confidence, Layer, ObservedFamily, Source, State,
    LIABILITY_CONSENT_VERSION, ORIGIN_USER_IMPORTED, USER_RULE_LIABILITY_NOTICE,
    USER_TARGET_LIABILITY_NOTICE,
};
use parallaxe_core::probe::{BoxFuture, FetchOutcome, FetchRequest, Transport};
use parallaxe_core::reports;
use parallaxe_core::runtime::{self, run_user_import_offline_preview, CheckOptions};
use parallaxe_core::targets::{
    Extractor, InspectionCategory, Registry, Rule, RuleScope, UserTargetImportInput,
};

struct CountingMockTransport {
    calls: AtomicUsize,
}

impl CountingMockTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
        })
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Transport for CountingMockTransport {
    fn fetch<'a>(&'a self, request: FetchRequest) -> BoxFuture<'a, FetchOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            FetchOutcome {
                destination: Default::default(),
                status: Some(200),
                content_type: Some("text/plain; charset=utf-8".to_string()),
                location: None,
                body: format!(
                    "fl=29f1\nh={}\nip=203.0.113.9\ncolo=SJC\nhttp=http/2\nloc=US\ntls=TLSv1.3\n",
                    request.host
                )
                .into_bytes(),
                complete: true,
                truncated: false,
                attempts: vec![Attempt {
                    family: AttemptFamily::V4,
                    phase: "HTTP_RESPONSE".to_string(),
                    connect_ms: Some(2),
                    first_byte_ms: Some(5),
                    http_status: Some(200),
                    fallback: false,
                    request_sent: true,
                }],
                request_count: 1,
                error: None,
                observed_family: Some(ObservedFamily::V4),
            }
        })
    }
}

#[test]
fn liability_notices_and_human_categories_are_complete() {
    assert!(USER_TARGET_LIABILITY_NOTICE.contains("属于用户自身行为"));
    assert!(USER_TARGET_LIABILITY_NOTICE.contains("由用户自行承担，本软件作者与贡献者不承担责任"));
    assert!(USER_RULE_LIABILITY_NOTICE.contains("不是本软件内置、开发、测试或背书的功能"));
    assert!(
        USER_RULE_LIABILITY_NOTICE.contains("由用户自行承担全部责任，本软件作者与贡献者不承担责任")
    );
    assert_eq!(InspectionCategory::ALL.len(), 9);
    for cat in InspectionCategory::ALL {
        assert!(!cat.label().is_empty());
        assert!(!cat.reader_kind().is_empty());
        assert_eq!(
            InspectionCategory::from_reader_kind(cat.reader_kind()),
            Some(cat)
        );
    }
}

#[test]
fn consent_gate_blocks_target_and_rule_import_when_unchecked() {
    let mut reg = Registry::embedded().unwrap();
    let err = reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("user.no-consent".to_string()),
                url: "https://trace.example.com/cdn-cgi/trace".to_string(),
                surface: Some("测试表面".to_string()),
                signals: vec!["rule.trace".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            false,
        )
        .unwrap_err();
    assert!(err.to_string().contains("责任声明"));
    assert_eq!(reg.catalog_size(), 135);

    let rule = Rule {
        id: "rule.user-noconsent".to_string(),
        version: 1,
        enabled: true,
        layer: Layer::ServicePolicy,
        channel: "SERVICE".to_string(),
        credibility: Confidence::Low,
        source_grade: Source::DirectAnonymousObservation,
        notes: "测试".to_string(),
        negative_case: "无".to_string(),
        extract: Extractor {
            kind: "status".to_string(),
            pattern: "status".to_string(),
            capture: "value".to_string(),
        },
        applies_to: Vec::new(),
    };
    let rule_err = reg.import_user_rule(rule, false).unwrap_err();
    assert!(rule_err.to_string().contains("责任声明"));
}

#[test]
fn url_validation_and_batch_import_enforce_security_boundaries() {
    let mut reg = Registry::embedded().unwrap();

    // 1. 拒绝 userinfo、fragment、端口、私有 IP、localhost、未显式允许的 http://
    let bad_urls = [
        "http://example.com/trace",
        "https://user:pass@example.com/trace",
        "https://example.com/trace#frag",
        "https://example.com:8443/trace",
        "https://localhost/trace",
        "https://127.0.0.1/trace",
        "https://192.168.1.1/trace",
        "https://10.0.0.1/trace",
        "https://172.16.0.1/trace",
        "https://[::1]/trace",
        "https://intranet/trace",
        "https://printer.local/trace",
    ];
    for url in bad_urls {
        let res = reg.import_user_target(
            UserTargetImportInput {
                id: None,
                url: url.to_string(),
                surface: None,
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        );
        assert!(res.is_err(), "非法 URL 应当被拒绝: {url}");
    }

    // 2. 显式允许 http 时通过并附带安全警告
    let http_meta = reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("http-demo".to_string()),
                url: "http://plain.example.com/status".to_string(),
                surface: Some("明文测试".to_string()),
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: true,
                user_group: None,
            },
            true,
        )
        .unwrap();
    assert_eq!(http_meta.id, "user.http-demo");
    assert!(http_meta.http_warning.is_some());
    assert_eq!(http_meta.origin, ORIGIN_USER_IMPORTED);
    assert_eq!(http_meta.consent_version, LIABILITY_CONSENT_VERSION);

    // 3. 拒绝冒充 google.* 或 control.* 前缀，拒绝重复 URL / 重复 ID
    assert!(reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("google.fake".to_string()),
                url: "https://fake.example.com/".to_string(),
                surface: None,
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .is_err());
    assert!(reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("user.http-demo".to_string()),
                url: "https://another.example.com/".to_string(),
                surface: None,
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .is_err());
    assert!(reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("user.dup-url".to_string()),
                url: "https://support.google.com/gemini/".to_string(),
                surface: None,
                signals: vec!["rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .is_err());

    // 4. 批量导入成功 + 含坏行时原子回滚
    let count_before = reg.catalog_size();
    let bad_batch = "https://batch1.example.com/a\nhttps://127.0.0.1/private\n";
    assert!(reg
        .import_user_targets_batch(
            bad_batch,
            Some("批量表面".to_string()),
            &["rule.trace".to_string()],
            false,
            true
        )
        .is_err());
    assert_eq!(
        reg.catalog_size(),
        count_before,
        "批量导入失败不得残留部分目标"
    );

    let good_batch = "# 批量导入注释\nuser.b1 | https://batch1.example.com/cdn-cgi/trace\nhttps://batch2.example.com/cdn-cgi/trace\n";
    let imported = reg
        .import_user_targets_batch(
            good_batch,
            Some("批量 Trace".to_string()),
            &["rule.trace".to_string(), "rule.http".to_string()],
            false,
            true,
        )
        .unwrap();
    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].id, "user.b1");
    assert!(imported[1].id.starts_with("user.batch2-example-com-"));
    assert_eq!(reg.catalog_size(), count_before + 2);
}

#[test]
fn rule_kind_validation_and_dangerous_regex_rejection() {
    let mut reg = Registry::embedded().unwrap();

    // 1. 未知 reader kind 拒绝
    let bad_kind_toml = r#"
[[rules]]
id = "rule.user-bad-kind"
version = 1
enabled = true
layer = "SERVICE_POLICY"
channel = "SERVICE"
credibility = "LOW"
source_grade = "DIRECT_ANONYMOUS_OBSERVATION"
notes = "非法 kind"
negative_case = "无"
extract = { kind = "xpath", pattern = "//div", capture = "value" }
applies_to = []
"#;
    assert!(reg.import_user_rules_toml(bad_kind_toml, true).is_err());

    // 2. 危险正则 (ReDoS 嵌套量词、环视、反向引用、无界花括号、legacy region) 全部拒绝
    let dangerous_patterns = [
        "(a+)+",
        "(.*)+",
        "(.+)*",
        "([a-z]+)+",
        "(a|aa)+",
        "(?=foo)bar",
        "(?!foo)bar",
        "([a-z])\\1",
        "a{1,1000}",
        "a{2,}",
        ".*.*country",
        "name=\"region\"",
    ];
    for pat in dangerous_patterns {
        let rule = Rule {
            id: "rule.user-redos".to_string(),
            version: 1,
            enabled: true,
            layer: Layer::ServicePolicy,
            channel: "SERVICE".to_string(),
            credibility: Confidence::Low,
            source_grade: Source::DirectAnonymousObservation,
            notes: "测试".to_string(),
            negative_case: "无".to_string(),
            extract: Extractor {
                kind: "regex".to_string(),
                pattern: pat.to_string(),
                capture: "1".to_string(),
            },
            applies_to: Vec::new(),
        };
        assert!(
            reg.import_user_rule(rule, true).is_err(),
            "危险正则应当被拒绝: {pat}"
        );
    }

    // 3. 含 admission 字段或试图绑定内置目标 google.* 的规则必须拒绝
    let admission_smuggle = r#"
[admission]
grants = []
[[rules]]
id = "rule.user-smuggle"
version = 1
enabled = true
layer = "SERVICE_POLICY"
channel = "SERVICE"
credibility = "LOW"
source_grade = "DIRECT_ANONYMOUS_OBSERVATION"
notes = "夹带 admission"
negative_case = "无"
extract = { kind = "status", pattern = "status", capture = "value" }
applies_to = []
"#;
    assert!(reg.import_user_rules_toml(admission_smuggle, true).is_err());

    let builtin_scope_rule = Rule {
        id: "rule.user-bind-builtin".to_string(),
        version: 1,
        enabled: true,
        layer: Layer::ServicePolicy,
        channel: "SERVICE".to_string(),
        credibility: Confidence::Low,
        source_grade: Source::DirectAnonymousObservation,
        notes: "试图绑定内置目标".to_string(),
        negative_case: "无".to_string(),
        extract: Extractor {
            kind: "status".to_string(),
            pattern: "status".to_string(),
            capture: "value".to_string(),
        },
        applies_to: vec![RuleScope {
            target_id: "google.gemini".to_string(),
            host: "gemini.google.com".to_string(),
            path: "/".to_string(),
            surface: "x".to_string(),
        }],
    };
    assert!(reg.import_user_rule(builtin_scope_rule, true).is_err());

    // 4. 合法的有界正则规则导入成功
    let good_rule_toml = r#"
[[rules]]
id = "rule.user-region-tag"
version = 1
enabled = true
layer = "SERVICE_POLICY"
channel = "SERVICE"
credibility = "LOW"
source_grade = "DIRECT_ANONYMOUS_OBSERVATION"
notes = "提取自定义 region_code"
negative_case = "无该字段时返回缺失"
extract = { kind = "regex", pattern = "data-country=\"([A-Z]{2})\"", capture = "1" }
applies_to = []
"#;
    let rules = reg.import_user_rules_toml(good_rule_toml, true).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "rule.user-region-tag");
    assert_eq!(rules[0].origin, ORIGIN_USER_IMPORTED);
    assert_eq!(rules[0].consent_version, LIABILITY_CONSENT_VERSION);
}

#[tokio::test]
async fn unauthorized_zero_packets_one_time_grant_revocation_and_audit_trail() {
    let mut reg = Registry::embedded().unwrap();

    // 导入一个自定义规则 + 一个自定义目标
    let custom_rule = Rule {
        id: "rule.user-custom-kv".to_string(),
        version: 1,
        enabled: true,
        layer: Layer::EgressNetwork,
        channel: "SERVICE".to_string(),
        credibility: Confidence::Medium,
        source_grade: Source::DirectAnonymousObservation,
        notes: "用户自定义 trace".to_string(),
        negative_case: "非 trace 表面不生效".to_string(),
        extract: Extractor {
            kind: "text_kv".to_string(),
            pattern: "loc,colo,http,tls,ip_present".to_string(),
            capture: "value".to_string(),
        },
        applies_to: Vec::new(),
    };
    reg.import_user_rule(custom_rule, true).unwrap();

    let target_meta = reg
        .import_user_target(
            UserTargetImportInput {
                id: Some("user.my-trace".to_string()),
                url: "https://my-trace.example.org/cdn-cgi/trace".to_string(),
                surface: Some("用户自选 Trace 端点".to_string()),
                signals: vec!["rule.user-custom-kv".to_string(), "rule.http".to_string()],
                allow_insecure_http: false,
                user_group: None,
            },
            true,
        )
        .unwrap();
    assert_eq!(target_meta.id, "user.my-trace");

    // 1. 默认未授权：即使显式选中该目标并提供 Transport，发包数必须为 0！
    let transport = CountingMockTransport::new();
    let opts = CheckOptions {
        targets: vec!["user.my-trace".to_string()],
        run_id: Some("run-unauth".to_string()),
        ..Default::default()
    };
    let unauth_out = runtime::run_check(
        &reg,
        &opts,
        Some(Arc::clone(&transport) as Arc<dyn Transport>),
        true,
        None,
    )
    .await
    .unwrap();
    assert_eq!(transport.count(), 0, "未授权时传输层调用计数必须为 0");
    assert_eq!(unauth_out.report.network_request_count, 0);
    assert!(unauth_out.report.results.is_empty());
    assert_eq!(unauth_out.report.exit_code, 3);

    // 2. `--full` 绝不自动包含用户导入目标
    let full_sel = reg.select(None, &[], &[], true).unwrap();
    assert_eq!(full_sel.ids.len(), 135);
    assert!(!full_sel.ids.contains(&"user.my-trace".to_string()));

    // 3. 严禁对内置目标注入一次性会话许可
    assert!(reg
        .authorize_session_once(&["google.gemini".to_string()], true)
        .is_err());
    assert!(reg
        .authorize_session_once(&["user.my-trace".to_string()], false)
        .is_err());

    // 4. 签发一次性许可但在运行前撤销 -> 发包数必须仍为 0！
    reg.authorize_session_once(&["user.my-trace".to_string()], true)
        .unwrap();
    assert!(reg.active_session_grant_snapshot().is_some());
    reg.revoke_session_grant();
    assert!(reg.active_session_grant_snapshot().is_none());
    let revoked_out = runtime::run_check(
        &reg,
        &opts,
        Some(Arc::clone(&transport) as Arc<dyn Transport>),
        true,
        None,
    )
    .await
    .unwrap();
    assert_eq!(transport.count(), 0, "撤销许可后发包数必须为 0");
    assert_eq!(revoked_out.report.network_request_count, 0);

    // 5. 正式签发一次性许可并执行第一次运行 -> 恰发 1 次请求，且报告带完整审计留痕
    let grant = reg
        .authorize_session_once(&["user.my-trace".to_string()], true)
        .unwrap();
    assert!(grant.prompt_summary.contains("1 个用户导入的网址"));
    let authed_out = runtime::run_check(
        &reg,
        &CheckOptions {
            targets: vec!["user.my-trace".to_string()],
            run_id: Some("run-authed-once".to_string()),
            ..Default::default()
        },
        Some(Arc::clone(&transport) as Arc<dyn Transport>),
        true,
        None,
    )
    .await
    .unwrap();
    assert_eq!(transport.count(), 1, "获一次性授权后应恰发起 1 次请求");
    assert_eq!(authed_out.report.network_request_count, 1);
    assert_eq!(authed_out.report.results.len(), 1);
    let res = &authed_out.report.results[0];
    assert_eq!(res.target, "user.my-trace");
    assert_eq!(res.state, State::Ok);
    assert_eq!(res.service_observed.observed_country.as_deref(), Some("US"));
    assert_eq!(res.origin.as_deref(), Some(ORIGIN_USER_IMPORTED));
    assert_eq!(
        res.consent_version.as_deref(),
        Some(LIABILITY_CONSENT_VERSION)
    );
    assert!(res.imported_at.is_some());
    assert!(res
        .rule_digest
        .as_deref()
        .is_some_and(|d| d.starts_with("sha256:")));
    assert!(res
        .evidence
        .iter()
        .all(|ev| ev.origin == ORIGIN_USER_IMPORTED));

    // 检查 JSON / CSV / HTML 导出留痕
    let json_text = reports::to_json(&authed_out.report);
    assert!(json_text.contains("\"origin\": \"USER_IMPORTED\""));
    assert!(json_text.contains("\"consent_version\": \"liability-v1\""));
    assert!(json_text.contains("\"rule_digest\": \"sha256:"));
    let csv_text = reports::to_csv(&authed_out.report);
    assert!(csv_text.contains("origin=USER_IMPORTED"));
    assert!(csv_text.contains("consent_version=liability-v1"));
    let html_text = reports::to_html(&authed_out.report);
    assert!(html_text.contains("origin=USER_IMPORTED"));
    assert!(html_text.contains("liability-v1"));

    // 6. 一次性许可消费后自动失效：紧接着第二次运行同一目标，不得再发包！
    assert!(reg.active_session_grant_snapshot().is_none());
    let second_out = runtime::run_check(
        &reg,
        &CheckOptions {
            targets: vec!["user.my-trace".to_string()],
            run_id: Some("run-second-unauth".to_string()),
            ..Default::default()
        },
        Some(Arc::clone(&transport) as Arc<dyn Transport>),
        true,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        transport.count(),
        1,
        "一次性许可在第二次运行时必须已失效，总发包数保持为 1"
    );
    assert_eq!(second_out.report.network_request_count, 0);

    // 7. 离线预览模式：无需在线许可即可预览，且网络发包数为 0，仍保留审计留痕
    let preview_out = run_user_import_offline_preview(
        &reg,
        &["user.my-trace".to_string()],
        None,
        Some("run-offline-preview".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(preview_out.report.network_request_count, 0);
    assert_eq!(preview_out.report.simulated_request_count, 1);
    assert_eq!(
        preview_out.report.results[0].origin.as_deref(),
        Some(ORIGIN_USER_IMPORTED)
    );

    // 8. 清除全部导入后，目录恢复至 135 项内置目标
    reg.clear_user_imports();
    assert_eq!(reg.catalog_size(), 135);
    assert!(reg.user_targets().is_empty());
    assert!(reg.user_rules().is_empty());
}
