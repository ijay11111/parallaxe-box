//! 离线自检套件：不触网、不读文件、不写系统。
//!
//! 覆盖 fixture 期望、负例、stale 开关、退出码优先级、v6 三态与 schema 契约。

use std::collections::HashMap;
use std::sync::Arc;

use time::OffsetDateTime;

use crate::contracts::{
    reason, Attempt, AttemptFamily, Confidence, Coverage, Event, Expectation, ExpectationStatus,
    LocalPolicy, Mode, ObservedFamily, Path, Report, RequestedFamily, ResultRecord, RiskSignal,
    ServiceObserved, State, Termination, TransportLayerResult,
};
use crate::fixtures::{self, FixtureCase};
use crate::network::{self, PathDiagnostic};
use crate::reports;
use crate::runtime::{self, CheckOptions};
use crate::signals::{self, Sample};
use crate::state::{FreshnessStore, MemoryStore, NullStore, StaleKeyParts};
use crate::targets::{ConfigError, Registry};
use crate::verdict::{self, VerdictInput};

/// 单个用例结果。
#[derive(Debug, Clone)]
pub struct SelftestCase {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl SelftestCase {
    fn pass(name: impl Into<String>, detail: impl Into<String>) -> Self {
        SelftestCase {
            name: name.into(),
            ok: true,
            detail: detail.into(),
        }
    }

    fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        SelftestCase {
            name: name.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

/// 套件结果。
#[derive(Debug, Clone)]
pub struct SelftestOutcome {
    pub suite: String,
    pub passed: u32,
    pub failed: u32,
    pub cases: Vec<SelftestCase>,
    pub report: Report,
    pub events: Vec<Event>,
}

impl SelftestOutcome {
    /// 全部通过 0，否则 1（与站点异常/不确定无关）。
    pub fn exit_code(&self) -> u8 {
        if self.failed == 0 {
            0
        } else {
            1
        }
    }

    /// 序列化为 `parallaxe.selftest/v1`。
    pub fn to_json(&self) -> String {
        let cases: Vec<serde_json::Value> = self
            .cases
            .iter()
            .map(|case| {
                serde_json::json!({
                    "name": case.name,
                    "ok": case.ok,
                    "detail": case.detail,
                })
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "parallaxe.selftest/v1",
            "suite": self.suite,
            "passed": self.passed,
            "failed": self.failed,
            "cases": cases,
            "report": self.report,
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn fixture_result(
    registry: &Registry,
    case: &FixtureCase,
    store: &dyn FreshnessStore,
    fixture: bool,
) -> Result<ResultRecord, ConfigError> {
    let target = registry
        .get(case.target_id)
        .ok_or_else(|| ConfigError::field("selftest", format!("未知目标 {}", case.target_id)))?;
    let path = Path::system("path-selftest");
    let evaluation = signals::evaluate(
        registry,
        target,
        &case.sample,
        &path.path_id,
        &crate::state::format_time(now()),
        fixture,
    );
    let attempts = vec![Attempt {
        family: AttemptFamily::V4,
        phase: "FIXTURE".to_string(),
        connect_ms: Some(1),
        first_byte_ms: Some(2),
        http_status: case.sample.status,
        fallback: false,
        request_sent: case.sample.status.is_some(),
    }];
    let outcome = crate::probe::FetchOutcome {
        destination: Default::default(),
        status: case.sample.status,
        content_type: case.sample.content_type.clone(),
        location: case.sample.location.clone(),
        body: case.sample.body.clone(),
        complete: case.sample.complete,
        truncated: case.sample.truncated,
        attempts,
        request_count: case.expected_request_count,
        error: None,
        observed_family: case.sample.observed_family,
    };
    Ok(verdict::decide(VerdictInput {
        registry,
        target,
        evaluation: &evaluation,
        outcome: &outcome,
        path: &path,
        path_diagnostic: PathDiagnostic::System,
        fixture,
        now: now(),
        store,
        expectation: None,
        capability_unsupported: None,
        cancelled: false,
        global_timeout: false,
    }))
}

fn fixture_checks(registry: &Registry) -> Vec<SelftestCase> {
    let mut cases = Vec::new();
    for case in fixtures::cases() {
        let store = NullStore::default();
        let name = format!("fixture:{}", case.id);
        match fixture_result(registry, &case, &store, true) {
            Ok(result) => {
                let state_ok = result.state == case.expected_state;
                let reason_ok = case
                    .expected_reason
                    .map(|expected| result.reason.as_deref() == Some(expected))
                    .unwrap_or(true);
                let country_ok = match case.expected_country {
                    Some(expected) => {
                        result.service_observed.observed_country.as_deref() == Some(expected)
                    }
                    None => true,
                };
                let layers_ok = result.region_layers.len() == 7;
                if state_ok && reason_ok && country_ok && layers_ok {
                    cases.push(SelftestCase::pass(
                        name,
                        format!(
                            "{} / {} / country={:?}",
                            result.state.as_str(),
                            result.reason.clone().unwrap_or_default(),
                            result.service_observed.observed_country
                        ),
                    ));
                } else {
                    cases.push(SelftestCase::fail(
                        name,
                        format!(
                            "期望 {:?}/{:?}/{:?}，实际 {:?}/{:?}/{:?}",
                            case.expected_state,
                            case.expected_reason,
                            case.expected_country,
                            result.state,
                            result.reason,
                            result.service_observed.observed_country
                        ),
                    ));
                }
            }
            Err(error) => cases.push(SelftestCase::fail(name, error.to_string())),
        }
    }
    cases
}

fn legacy_negative_check(registry: &Registry) -> SelftestCase {
    let Some(case) = fixtures::find("legacy-region-negative") else {
        return SelftestCase::fail("legacy-region-negative", "内置案例或目标缺失");
    };
    let store = NullStore::default();
    match fixture_result(registry, &case, &store, true) {
        Ok(result) => {
            let leaked = result
                .evidence
                .iter()
                .any(|item| item.rule_id == "rule.legacy-region");
            if leaked {
                SelftestCase::fail(
                    "rules:legacy-selector-negative",
                    "禁用规则 rule.legacy-region 产生了证据",
                )
            } else {
                SelftestCase::pass(
                    "rules:legacy-selector-negative",
                    "旧 name=\"region\" 出现但零证据",
                )
            }
        }
        Err(error) => SelftestCase::fail("rules:legacy-selector-negative", error.to_string()),
    }
}

fn default_no_stale_check(registry: &Registry) -> SelftestCase {
    let Some(case) = fixtures::find("books-missing") else {
        return SelftestCase::fail("books-missing", "内置案例或目标缺失");
    };
    let store = NullStore::default();
    let mut states = Vec::new();
    for _ in 0..3 {
        match fixture_result(registry, &case, &store, false) {
            Ok(result) => states.push(result.state),
            Err(error) => return SelftestCase::fail("stale:default-off", error.to_string()),
        }
    }
    if states.contains(&State::SignalStale) {
        SelftestCase::fail("stale:default-off", "默认 N=1 却报告了 SIGNAL_STALE")
    } else {
        SelftestCase::pass(
            "stale:default-off",
            format!("三次缺失未报 stale：{:?}", states),
        )
    }
}

fn enabled_stale_check(registry: &Registry) -> SelftestCase {
    let store = MemoryStore::default();
    let Some(gemini) = fixtures::find("gemini-positive") else {
        return SelftestCase::fail("gemini-positive", "内置案例或目标缺失");
    };
    let Some(missing) = fixtures::find("books-missing") else {
        return SelftestCase::fail("books-missing", "内置案例或目标缺失");
    };
    // 先让同目标同规则有合格命中。
    let hit = fixture_result(registry, &gemini, &store, false);
    if let Err(error) = hit {
        return SelftestCase::fail("stale:enabled", error.to_string());
    }
    // 同目标同规则连续三次缺失（用 Books 的缺字段样本，但目标仍是 Gemini）。
    let Some(gemini_target) = registry.get("google.gemini") else {
        return SelftestCase::fail("google.gemini", "内置案例或目标缺失");
    };
    let mut sample: Sample = missing.sample.clone();
    sample.status = Some(200);
    sample.content_type = Some("text/html".to_string());
    let mut last_state = None;
    for index in 0..3 {
        let path = Path::system("path-selftest");
        let evaluation = signals::evaluate(
            registry,
            gemini_target,
            &sample,
            &path.path_id,
            &crate::state::format_time(now()),
            false,
        );
        let outcome = crate::probe::FetchOutcome {
            destination: Default::default(),
            status: Some(200),
            content_type: Some("text/html".to_string()),
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
        let result = verdict::decide(VerdictInput {
            registry,
            target: gemini_target,
            evaluation: &evaluation,
            outcome: &outcome,
            path: &path,
            path_diagnostic: PathDiagnostic::System,
            fixture: false,
            now: now(),
            store: &store,
            expectation: None,
            capability_unsupported: None,
            cancelled: false,
            global_timeout: false,
        });
        last_state = Some((index + 1, result.state, result.reason));
    }
    match last_state {
        Some((3, State::SignalStale, _)) => SelftestCase::pass(
            "stale:enabled-n3",
            "显式虚拟状态目录下第三次缺失报 SIGNAL_STALE",
        ),
        other => SelftestCase::fail(
            "stale:enabled-n3",
            format!("期望第三次 SIGNAL_STALE，实际 {other:?}"),
        ),
    }
}

fn truncated_no_stale_check(registry: &Registry) -> SelftestCase {
    let store = MemoryStore::default();
    let Some(books) = registry.get("google.books") else {
        return SelftestCase::fail("google.books", "内置案例或目标缺失");
    };
    let parts = StaleKeyParts {
        target_id: "google.books",
        url: &books.raw.url,
        surface: &books.raw.surface,
        rule_id: "rule.inline3",
        rule_version: 1,
        family: "V4",
        path_id: "path-selftest",
        shape: "ANONYMOUS_EMPTY_COOKIE",
    };
    // 先制造一次合格命中，再累计两次缺失。
    store.hit(&parts, now());
    store.miss(&parts, now());
    store.miss(&parts, now());
    let Some(case) = fixtures::find("truncated-page") else {
        return SelftestCase::fail("truncated-page", "内置案例或目标缺失");
    };
    match fixture_result(registry, &case, &store, false) {
        Ok(result) => {
            if result.state == State::SignalStale {
                SelftestCase::fail("stale:truncated-not-counted", "截断样本被计入 stale")
            } else if result.state == State::Unknown
                && result.reason.as_deref() == Some(reason::TRUNCATED)
            {
                SelftestCase::pass(
                    "stale:truncated-not-counted",
                    "截断样本只记 UNKNOWN/TRUNCATED，未递增 stale",
                )
            } else {
                SelftestCase::fail(
                    "stale:truncated-not-counted",
                    format!("期望 UNKNOWN/TRUNCATED，实际 {}", result.state.as_str()),
                )
            }
        }
        Err(error) => SelftestCase::fail("stale:truncated-not-counted", error.to_string()),
    }
}

fn synthetic_result(target: &str, state: State) -> ResultRecord {
    ResultRecord {
        destination: Default::default(),
        target: target.to_string(),
        state,
        reason: None,
        confidence: Confidence::High,
        source: None,
        path: Path::system("path-selftest"),
        duration_ms: 0,
        request_count: 0,
        evidence: Vec::new(),
        region_layers: verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: Some(PathDiagnostic::System.as_str().to_string()),
        },
        service_observed: ServiceObserved {
            surface: "selftest".to_string(),
            observed_country: None,
            evidence_ids: Vec::new(),
            reason: None,
        },
        risk_signal: RiskSignal {
            markers: Vec::new(),
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy: LocalPolicy {
            protected: false,
            guard_eligible: false,
            plan_id: None,
            reason: None,
        },
        expectation: None,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: Vec::new(),
    }
}

fn report_with(states: &[(State, Option<Expectation>)], uncovered_selected: usize) -> Report {
    let mut report = Report::new(
        "selftest",
        crate::state::format_time(now()),
        "run-selftest",
        Mode::Fixture,
    );
    report.results = states
        .iter()
        .enumerate()
        .map(|(index, (state, expectation))| {
            let mut result = synthetic_result(&format!("selftest.{index}"), *state);
            result.expectation = expectation.clone();
            result
        })
        .collect();
    report.coverage = Coverage {
        catalog_size: 1,
        selected_count: 1,
        covered: Vec::new(),
        uncovered: (0..uncovered_selected)
            .map(|index| crate::contracts::Uncovered {
                target: format!("selftest.unc.{index}"),
                selected: true,
                reason: reason::NO_ADMISSION.to_string(),
            })
            .collect(),
    };
    report.exit_code = reports::aggregate_report(&report);
    report
}

fn exit_code_checks() -> Vec<SelftestCase> {
    let mut cases = Vec::new();
    let known_difference = report_with(
        &[(
            State::Ok,
            Some(Expectation {
                key: "service_country".to_string(),
                expected: "CN".to_string(),
                actual: Some("US".to_string()),
                status: ExpectationStatus::Mismatch,
            }),
        )],
        0,
    );
    if known_difference.exit_code == 0 {
        cases.push(SelftestCase::pass(
            "exit:expectation-known-difference",
            "--expect 已知不同只作提示，不单独升为 2/3",
        ));
    } else {
        cases.push(SelftestCase::fail(
            "exit:expectation-known-difference",
            format!("期望 0，实际 {}", known_difference.exit_code),
        ));
    }
    let blocked = report_with(
        &[
            (State::Blocked, None),
            (State::Unknown, None),
            (State::Conflict, None),
        ],
        0,
    );
    if blocked.exit_code == 2 {
        cases.push(SelftestCase::pass(
            "exit:blocked-unknown-conflict",
            "BLOCKED+UNKNOWN+CONFLICT → 2（不取 max(2,3)）",
        ));
    } else {
        cases.push(SelftestCase::fail(
            "exit:blocked-unknown-conflict",
            format!("期望 2，实际 {}", blocked.exit_code),
        ));
    }
    let local = report_with(&[(State::LocalPathError, None), (State::Unknown, None)], 0);
    cases.push(if local.exit_code == 2 {
        SelftestCase::pass("exit:local-path-error", "LOCAL_PATH_ERROR+UNKNOWN → 2")
    } else {
        SelftestCase::fail(
            "exit:local-path-error",
            format!("期望 2，实际 {}", local.exit_code),
        )
    });
    let unknown = report_with(&[(State::Unknown, None)], 0);
    cases.push(if unknown.exit_code == 3 {
        SelftestCase::pass("exit:unknown", "UNKNOWN → 3")
    } else {
        SelftestCase::fail(
            "exit:unknown",
            format!("期望 3，实际 {}", unknown.exit_code),
        )
    });
    let ok = report_with(&[(State::ReachableOnly, None)], 0);
    cases.push(if ok.exit_code == 0 {
        SelftestCase::pass("exit:reachable-only", "REACHABLE_ONLY → 0")
    } else {
        SelftestCase::fail(
            "exit:reachable-only",
            format!("期望 0，实际 {}", ok.exit_code),
        )
    });
    let expectation_mismatch = report_with(
        &[(
            State::ReachableOnly,
            Some(Expectation {
                key: "service_country".to_string(),
                expected: "US".to_string(),
                actual: None,
                status: ExpectationStatus::NotObservable,
            }),
        )],
        0,
    );
    if expectation_mismatch.exit_code == 3 {
        cases.push(SelftestCase::pass(
            "exit:expectation-unsettled",
            "--expect 所求量纲不可观察 → 3",
        ));
    } else {
        cases.push(SelftestCase::fail(
            "exit:expectation-unsettled",
            format!("期望 3，实际 {}", expectation_mismatch.exit_code),
        ));
    }
    let uncovered = report_with(&[], 1);
    cases.push(if uncovered.exit_code == 3 {
        SelftestCase::pass("exit:no-admission", "选中但未获准 → 3")
    } else {
        SelftestCase::fail(
            "exit:no-admission",
            format!("期望 3，实际 {}", uncovered.exit_code),
        )
    });
    cases
}

fn v6_checks() -> Vec<SelftestCase> {
    fn attempt(family: AttemptFamily, status: Option<u16>, first_byte: Option<u64>) -> Attempt {
        Attempt {
            family,
            phase: "SELFTEST".to_string(),
            connect_ms: None,
            first_byte_ms: first_byte,
            http_status: status,
            fallback: false,
            request_sent: status.is_some(),
        }
    }
    let slow =
        network::diagnose_attempts(&[attempt(AttemptFamily::V6, Some(200), Some(9_500))], 8_000);
    let broken = network::diagnose_attempts(
        &[
            attempt(AttemptFamily::V6, None, None),
            attempt(AttemptFamily::V4, Some(200), Some(120)),
        ],
        8_000,
    );
    let only_v6 = network::diagnose_attempts(
        &[
            attempt(AttemptFamily::V4, None, None),
            attempt(AttemptFamily::V6, Some(200), Some(300)),
        ],
        8_000,
    );
    vec![
        if slow == PathDiagnostic::Ipv6Slow {
            SelftestCase::pass("v6:slow", "v6 慢但成功 → IPv6_SLOW")
        } else {
            SelftestCase::fail("v6:slow", format!("实际 {slow:?}"))
        },
        if broken == PathDiagnostic::Ipv6Broken {
            SelftestCase::pass("v6:broken", "v6 失败、v4 成功 → IPv6_BROKEN")
        } else {
            SelftestCase::fail("v6:broken", format!("实际 {broken:?}"))
        },
        if only_v6 == PathDiagnostic::Ipv6OnlyEgress {
            SelftestCase::pass("v6:only-egress", "v4 失败、v6 成功 → IPV6_ONLY_EGRESS")
        } else {
            SelftestCase::fail("v6:only-egress", format!("实际 {only_v6:?}"))
        },
    ]
}

fn schema_check(result: &ResultRecord) -> SelftestCase {
    match serde_json::to_value(result) {
        Ok(value) => {
            let required = [
                "target",
                "state",
                "reason",
                "confidence",
                "source",
                "path",
                "duration_ms",
                "request_count",
                "evidence",
                "region_layers",
                "observed",
                "conflicts",
                "agreement",
                "transport",
                "service_observed",
                "risk_signal",
                "local_policy",
            ];
            let missing: Vec<&str> = required
                .iter()
                .filter(|key| value.get(**key).is_none())
                .copied()
                .collect();
            let layers = value
                .get("region_layers")
                .and_then(|layers| layers.as_array())
                .map(|layers| layers.len())
                .unwrap_or(0);
            let path_ok = value
                .get("path")
                .map(|path| {
                    ["path_id", "requested_family", "proxy_mode", "bind_status"]
                        .iter()
                        .all(|key| path.get(*key).is_some())
                })
                .unwrap_or(false);
            if missing.is_empty() && layers == 7 && path_ok {
                SelftestCase::pass("schema:result-v1", "Result 必填字段与七层齐全")
            } else {
                SelftestCase::fail(
                    "schema:result-v1",
                    format!("缺字段 {missing:?} / layers={layers}"),
                )
            }
        }
        Err(error) => SelftestCase::fail("schema:result-v1", error.to_string()),
    }
}

async fn admission_check(registry: &Registry) -> Vec<SelftestCase> {
    let options = CheckOptions {
        profile: Some("google-all".to_string()),
        run_id: Some("run-selftest-admission".to_string()),
        ..Default::default()
    };
    if registry.distribution_mode() {
        let now = crate::state::format_time(time::OffsetDateTime::now_utc());
        let selected = registry.select(Some("google-all"), &[], &[], false);
        let plan = runtime::network_plan(registry, &options);
        let admitted = selected
            .as_ref()
            .map(|selection| {
                selection
                    .ids
                    .iter()
                    .filter(|id| {
                        registry
                            .get(id)
                            .is_some_and(|target| registry.admission(target, false, &now).admitted)
                    })
                    .count()
            })
            .unwrap_or(0);
        let control_admitted = registry
            .entries()
            .iter()
            .filter(|target| target.id().starts_with("control."))
            .filter(|target| registry.admission(target, false, &now).admitted)
            .count();
        let plan_ok = plan
            .as_ref()
            .map(|plan| plan.target_count == 107 && plan.zero_packet)
            .unwrap_or(false);
        return vec![
            if selected
                .as_ref()
                .is_ok_and(|selection| selection.ids.len() == 107)
                && admitted == 107
                && control_admitted == 0
            {
                SelftestCase::pass(
                    "admission:distribution-google",
                    "发行包模式 Google 107 项获准、control 目标仍零发包",
                )
            } else {
                SelftestCase::fail(
                    "admission:distribution-google",
                    format!("selected/admitted={admitted}/107 control_admitted={control_admitted}"),
                )
            },
            if plan_ok {
                SelftestCase::pass(
                    "admission:distribution-plan-offline",
                    "发行包风险预览选中 107、zero_packet=true",
                )
            } else {
                SelftestCase::fail(
                    "admission:distribution-plan-offline",
                    format!("计划校验失败：{plan:?}"),
                )
            },
        ];
    }
    match runtime::run_check(registry, &options, None, false, None).await {
        Ok(output) => {
            let selected_ok = output.report.coverage.selected_count == 107;
            let zero_requests = output.report.network_request_count == 0;
            let exit_ok = output.report.exit_code == 3;
            let all_uncovered = output
                .report
                .coverage
                .uncovered
                .iter()
                .filter(|item| item.selected)
                .all(|item| {
                    item.reason == reason::NO_ADMISSION
                        || item.reason == reason::ALLOW_ONLINE_FALSE
                        || item.reason == reason::SAFE_DEFAULT_FALSE
                });
            vec![
                if selected_ok && zero_requests && exit_ok && all_uncovered {
                    SelftestCase::pass(
                        "admission:empty-grants",
                        "google-all 选中 107、发包 0、退出 3、全部 NO_ADMISSION",
                    )
                } else {
                    SelftestCase::fail(
                        "admission:empty-grants",
                        format!(
                            "selected={} requests={} exit={}",
                            output.report.coverage.selected_count,
                            output.report.network_request_count,
                            output.report.exit_code
                        ),
                    )
                },
                if output.report.results.is_empty() {
                    SelftestCase::pass("admission:no-fake-results", "未发请求时没有伪造 Result")
                } else {
                    SelftestCase::fail(
                        "admission:no-fake-results",
                        format!("出现 {} 条结果", output.report.results.len()),
                    )
                },
            ]
        }
        Err(error) => vec![SelftestCase::fail(
            "admission:empty-grants",
            error.to_string(),
        )],
    }
}

async fn fixture_mode_check(registry: &Registry) -> Vec<SelftestCase> {
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        run_id: Some("run-selftest-fixture".to_string()),
        ..Default::default()
    };
    match runtime::run_check(registry, &options, None, false, None).await {
        Ok(output) => {
            let report = &output.report;
            let offline = report.network_request_count == 0
                && report.simulated_request_count == 1
                && report.mode == Mode::Fixture;
            vec![
                if offline {
                    SelftestCase::pass("fixture:offline", "FIXTURE 模式发包 0、模拟 1")
                } else {
                    SelftestCase::fail(
                        "fixture:offline",
                        format!(
                            "network={} simulated={} mode={:?}",
                            report.network_request_count,
                            report.simulated_request_count,
                            report.mode
                        ),
                    )
                },
                if output.events.iter().all(|event| event.is_fixture) {
                    SelftestCase::pass("fixture:events-flagged", "事件均带 is_fixture=true")
                } else {
                    SelftestCase::fail("fixture:events-flagged", "存在未标记 fixture 的事件")
                },
            ]
        }
        Err(error) => vec![SelftestCase::fail("fixture:offline", error.to_string())],
    }
}

async fn build_report(
    cases: &[SelftestCase],
    registry: &Registry,
    run_id: &str,
) -> (Report, Vec<Event>) {
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        run_id: Some(run_id.to_string()),
        ..Default::default()
    };
    match runtime::run_check(registry, &options, None, false, None).await {
        Ok(output) => (output.report, output.events),
        Err(_) => {
            let mut report = Report::new(
                crate::VERSION,
                crate::state::format_time(now()),
                run_id,
                Mode::Fixture,
            );
            report.termination = Termination::Failed;
            report.exit_code = 1;
            report.results = cases
                .iter()
                .enumerate()
                .map(|(index, case)| {
                    synthetic_result(
                        &format!("selftest.{index}"),
                        if case.ok {
                            State::ReachableOnly
                        } else {
                            State::Unknown
                        },
                    )
                })
                .collect();
            report.coverage = Coverage {
                catalog_size: 1,
                selected_count: cases.len() as u64,
                covered: Vec::new(),
                uncovered: Vec::new(),
            };
            (report, Vec::new())
        }
    }
}

/// 运行离线套件。
pub async fn run(
    registry: &Registry,
    suite: Option<&str>,
    run_id: Option<String>,
) -> Result<SelftestOutcome, ConfigError> {
    let suite_name = suite.unwrap_or("default");
    if suite_name != "default" {
        return Err(ConfigError::field(
            format!("--suite {suite_name}"),
            "只允许内置离线套件 default",
        ));
    }
    let mut cases = fixture_checks(registry);
    cases.push(legacy_negative_check(registry));
    cases.push(default_no_stale_check(registry));
    cases.push(enabled_stale_check(registry));
    cases.push(truncated_no_stale_check(registry));
    cases.extend(exit_code_checks());
    cases.extend(v6_checks());
    let sample = fixtures::find("gemini-positive")
        .ok_or_else(|| ConfigError::field("selftest", "内置案例缺失"))?;
    let store = NullStore::default();
    if let Ok(result) = fixture_result(registry, &sample, &store, true) {
        cases.push(schema_check(&result));
    } else {
        cases.push(SelftestCase::fail("schema:result-v1", "构造样本失败"));
    }
    cases.extend(admission_check(registry).await);
    cases.extend(fixture_mode_check(registry).await);

    let passed = cases.iter().filter(|case| case.ok).count() as u32;
    let failed = cases.iter().filter(|case| !case.ok).count() as u32;
    let run_id = run_id.unwrap_or_else(|| format!("selftest-{}", now().unix_timestamp_nanos()));
    let (mut report, mut events) = build_report(&cases, registry, &run_id).await;
    if failed > 0 {
        report.termination = Termination::Failed;
        report.exit_code = 1;
        if let Some(event) = events.last_mut() {
            event.payload["report"] = serde_json::json!(&report);
        }
    }
    Ok(SelftestOutcome {
        suite: suite_name.to_string(),
        passed,
        failed,
        cases,
        report,
        events,
    })
}

/// 供 CLI 汇总的用例计数文本。
pub fn summary(outcome: &SelftestOutcome) -> String {
    format!(
        "离线自检 {}：通过 {} / 失败 {}",
        outcome.suite, outcome.passed, outcome.failed
    )
}

/// 供测试复用：构造一个已覆盖项（避免测试重复实现）。
pub fn sample_covered(target: &str) -> crate::contracts::Covered {
    crate::contracts::Covered {
        target: target.to_string(),
        request_count: 1,
        http_received: true,
    }
}

/// 供测试复用：空映射。
pub fn empty_map() -> HashMap<String, String> {
    HashMap::new()
}

/// 供测试复用：Arc 传输。
pub fn arc_transport<T: crate::probe::Transport + 'static>(
    transport: T,
) -> Arc<dyn crate::probe::Transport> {
    Arc::new(transport)
}

/// 供测试复用：请求族文本。
pub fn family_text(family: RequestedFamily) -> &'static str {
    match family {
        RequestedFamily::Auto => "AUTO",
        RequestedFamily::V4 => "V4",
        RequestedFamily::V6 => "V6",
    }
}
