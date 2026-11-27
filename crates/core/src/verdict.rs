//! `verdict`：汇总独立证据、冲突、stale 与四层结论。
//!
//! 只消费 `signals` 给出的证据与路径事实；不接触原始传输，不生成阻断规则。

use std::collections::BTreeMap;

use time::OffsetDateTime;

use crate::contracts::{
    normalize_country, reason, Agreement, Confidence, Conflict, Covered, Expectation,
    ExpectationStatus, Freshness, FreshnessStatus, Layer, LayerValue, LocalPolicy, ObservedFamily,
    Path, ResultRecord, RiskSignal, ServiceObserved, State, TransportLayerResult, Vote,
};
use crate::network::PathDiagnostic;
use crate::probe::FetchOutcome;
use crate::signals::{country_claim, Evaluation};
use crate::state::{FreshnessStore, StaleKeyParts};
use crate::targets::{ExpectationSpec, Registry, Target};

/// 判定输入。
pub struct VerdictInput<'a> {
    pub registry: &'a Registry,
    pub target: &'a Target,
    pub evaluation: &'a Evaluation,
    pub outcome: &'a FetchOutcome,
    pub path: &'a Path,
    pub path_diagnostic: PathDiagnostic,
    pub fixture: bool,
    pub now: OffsetDateTime,
    pub store: &'a dyn FreshnessStore,
    pub expectation: Option<&'a ExpectationSpec>,
    pub capability_unsupported: Option<&'static str>,
    pub cancelled: bool,
    pub global_timeout: bool,
}

/// 由输入生成单目标结果。
pub fn decide(input: VerdictInput<'_>) -> ResultRecord {
    let VerdictInput {
        registry,
        target,
        evaluation,
        outcome,
        path,
        path_diagnostic,
        fixture,
        now,
        store,
        expectation,
        capability_unsupported,
        cancelled,
        global_timeout,
        ..
    } = input;

    let mut evidence = evaluation.evidence.clone();
    apply_freshness(
        registry,
        target,
        &mut evidence,
        store,
        now,
        fixture,
        outcome.observed_family,
        path,
    );

    let stale = evidence
        .iter()
        .any(|item| item.freshness.status == FreshnessStatus::Stale);

    // 分桶投票：服务/表面/层/地址族/路径；同响应同源值不重复投票。
    let mut buckets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, item) in evidence.iter().enumerate() {
        if country_claim(item).is_none() {
            continue;
        }
        let layer = item
            .layer
            .region_layer()
            .map(layer_name)
            .unwrap_or("TRANSPORT");
        let family = outcome
            .observed_family
            .map(observed_family_name)
            .unwrap_or("UNKNOWN");
        let bucket = format!(
            "{}|{}|{}|{}|{}",
            target.raw.id, target.raw.surface, layer, family, path.path_id
        );
        buckets.entry(bucket).or_default().push(index);
    }

    let mut agreement = Vec::new();
    let mut conflicts = Vec::new();
    for (bucket, indices) in &buckets {
        let mut votes: Vec<Vote> = Vec::new();
        for index in indices {
            let item = &evidence[*index];
            let Some(value) = country_claim(item) else {
                continue;
            };
            if let Some(vote) = votes.iter_mut().find(|vote| vote.value == value) {
                if !vote.sample_ids.contains(&item.sample_id) {
                    vote.sample_ids.push(item.sample_id.clone());
                }
            } else {
                votes.push(Vote {
                    value,
                    sample_ids: vec![item.sample_id.clone()],
                });
            }
        }
        let Some(first) = indices.first().and_then(|i| evidence.get(*i)) else {
            continue;
        };
        let Some(max_votes) = votes.iter().map(|vote| vote.sample_ids.len()).max() else {
            continue;
        };
        let mut leaders = votes
            .iter()
            .filter(|vote| vote.sample_ids.len() == max_votes);
        let winner = leaders.next().map(|vote| vote.value.clone());
        let tie = leaders.next().is_some();
        let winner = if tie { None } else { winner };
        agreement.push(Agreement {
            bucket: bucket.clone(),
            votes: votes.clone(),
            winner: winner.clone(),
            tie,
        });
        if votes.len() > 1 {
            conflicts.push(Conflict {
                claim: first.claim.clone(),
                evidence_ids: indices
                    .iter()
                    .map(|index| evidence[*index].id.clone())
                    .collect(),
            });
        }
    }

    let observed_country = if conflicts.is_empty() {
        region_country(&evidence, &[Layer::EgressNetwork, Layer::ServicePolicy])
    } else {
        None
    };
    let edition_value = preference_value(&evidence);

    let policy = crate::signals::policy_evidence(&evidence);
    let state;
    let mut reason_text: Option<String> = None;

    if cancelled {
        state = State::Unknown;
        reason_text = Some(reason::CANCELLED.to_string());
    } else if global_timeout {
        state = State::Unknown;
        reason_text = Some(reason::GLOBAL_TIMEOUT.to_string());
    } else if let Some(capability) = capability_unsupported {
        state = State::Unsupported;
        reason_text = Some(capability.to_string());
    } else if let Some(stop) = &evaluation.stop {
        state = State::Unknown;
        reason_text = Some(stop.reason.to_string());
    } else if outcome.status.is_none() {
        let error = outcome.error.clone();
        match error.map(|error| error.kind) {
            Some(crate::probe::TransportErrorKind::WasmOffline) => {
                state = State::Unsupported;
                reason_text = Some(reason::SCOPE_WASM_OFFLINE.to_string());
            }
            Some(crate::probe::TransportErrorKind::Tls) => {
                state = State::Unknown;
                reason_text = Some(reason::TRANSPORT_TLS.to_string());
            }
            Some(kind) => {
                state = State::LocalPathError;
                reason_text = Some(kind.reason().to_string());
            }
            None => {
                state = State::Unknown;
                reason_text = Some(reason::TRANSPORT_OTHER.to_string());
            }
        }
    } else if !conflicts.is_empty() {
        state = State::Conflict;
        reason_text = Some(reason::CONFLICTING_EVIDENCE.to_string());
    } else if stale {
        state = State::SignalStale;
        reason_text = Some(reason::STALE_AFTER_MISSES.to_string());
    } else if outcome.truncated {
        state = State::Unknown;
        reason_text = Some(reason::TRUNCATED.to_string());
    } else if let Some(decision) = policy {
        state = if decision.value == "REGION_RESTRICTED" {
            State::Restricted
        } else {
            State::Blocked
        };
        reason_text = decision.value.as_str().map(str::to_owned);
    } else if observed_country.is_some() {
        state = State::Ok;
    } else if edition_value.is_some() {
        state = State::ReachableOnly;
        reason_text = Some(reason::EDITION_ONLY.to_string());
    } else if target.raw.confidence != Confidence::None {
        state = State::Unknown;
        reason_text = Some(reason::FIELD_MISSING.to_string());
    } else {
        state = State::ReachableOnly;
        reason_text = Some(reason::NO_REGION_SIGNAL.to_string());
    }

    // 取消/超时覆盖不了已观察到的明确异常优先级，但本阶段没有在线运行；
    // 这里保持 UNKNOWN，由运行归约统一按 3 处理。

    let country_evidence: Vec<_> = evidence
        .iter()
        .filter(|e| observed_country.is_some() && country_claim(e) == observed_country)
        .collect();
    // 传输事实的高可信度不能抬高页面国家信号的低可信度。
    let supporting = if matches!(state, State::Restricted | State::Blocked) {
        policy
    } else {
        country_evidence
            .iter()
            .copied()
            .max_by_key(|e| e.confidence)
    };
    let confidence = supporting.map_or_else(
        || result_confidence(&evidence, state, outcome),
        |e| e.confidence,
    );
    let source = supporting
        .map(|e| e.source_type)
        .or_else(|| {
            evidence
                .iter()
                .find(|e| !e.value.is_null())
                .map(|e| e.source_type)
        })
        .or_else(|| {
            outcome
                .http_received()
                .then_some(target.defaults.source_type)
        });
    let country_ids = country_evidence.into_iter().map(|e| e.id.clone()).collect();

    let markers = evaluation
        .stop
        .as_ref()
        .map(|stop| {
            stop.markers
                .iter()
                .map(|marker| marker.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let markers = {
        let mut markers = markers;
        markers.extend(outcome.destination.markers().map(str::to_owned));
        if outcome.truncated && !markers.contains(&"TRUNCATED".to_string()) {
            markers.push("TRUNCATED".to_string());
        }
        markers
    };

    let local_policy = LocalPolicy {
        protected: target.raw.protected,
        guard_eligible: matches!(state, State::Restricted | State::Blocked)
            && !target.raw.protected
            && confidence.meets_guard_floor(),
        plan_id: None,
        reason: Some(if target.raw.protected {
            reason::PROTECTED_HOST.to_string()
        } else if matches!(state, State::Restricted | State::Blocked)
            && confidence.meets_guard_floor()
        {
            reason::GUARD_STAGE4_ONLY.to_string()
        } else {
            reason::NOT_GUARD_ELIGIBLE.to_string()
        }),
    };

    let expectation =
        expectation.map(|spec| build_expectation(spec, state, &observed_country, outcome));

    ResultRecord {
        destination: outcome.destination.clone(),
        target: target.raw.id.clone(),
        state,
        reason: reason_text,
        confidence,
        source,
        path: Path {
            observed_family: outcome.observed_family,
            ..path.clone()
        },
        duration_ms: outcome
            .attempts
            .iter()
            .filter_map(|attempt| attempt.first_byte_ms)
            .max()
            .unwrap_or(0),
        request_count: outcome.request_count,
        observed: evidence
            .iter()
            .filter(|item| !item.value.is_null())
            .map(|item| item.id.clone())
            .collect(),
        region_layers: region_layers(
            &evidence,
            observed_country.as_deref(),
            edition_value.as_deref(),
        ),
        evidence,
        conflicts,
        agreement,
        transport: TransportLayerResult {
            attempts: outcome.attempts.clone(),
            path_diagnostic: Some(path_diagnostic.as_str().to_string()),
        },
        service_observed: ServiceObserved {
            surface: target.raw.surface.clone(),
            observed_country,
            evidence_ids: country_ids,
            reason: if edition_value.is_some() {
                Some(reason::PUBLIC_EDITION.to_string())
            } else {
                None
            },
        },
        risk_signal: RiskSignal {
            markers,
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy,
        expectation,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: registry.eligibility_notes_for(
            crate::forensics::policy_product(&target.raw.url).unwrap_or(target.id()),
        ),
    }
}

fn observed_family_name(family: ObservedFamily) -> &'static str {
    match family {
        ObservedFamily::V4 => "V4",
        ObservedFamily::V6 => "V6",
    }
}

fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::EgressNetwork => "EGRESS_NETWORK",
        Layer::ServicePolicy => "SERVICE_POLICY",
        Layer::Account => "ACCOUNT",
        Layer::Payments => "PAYMENTS",
        Layer::Preference => "PREFERENCE",
        Layer::Device => "DEVICE",
        Layer::Cloud => "CLOUD",
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_freshness(
    registry: &Registry,
    target: &Target,
    evidence: &mut [crate::contracts::Evidence],
    store: &dyn FreshnessStore,
    now: OffsetDateTime,
    fixture: bool,
    family: Option<ObservedFamily>,
    path: &Path,
) {
    if fixture {
        // fixture 只带样本时间，不进入线上计数；新鲜度标为 CURRENT/MISSING。
        for item in evidence.iter_mut() {
            item.freshness = if item.value.is_null() {
                Freshness {
                    status: FreshnessStatus::Missing,
                    last_hit_at: None,
                    comparable_misses: 0,
                }
            } else {
                Freshness {
                    status: FreshnessStatus::Current,
                    last_hit_at: Some(crate::state::format_time(now)),
                    comparable_misses: 0,
                }
            };
        }
        return;
    }
    // 只将已观察的同族、同路径样本用于持久历史。代理后端族不可验证。
    if store.enabled() && (family.is_none() || path.proxy_ref.is_some()) {
        return;
    }
    let rules = registry.rules_for(target);
    for rule in rules {
        if rule.channel != "SERVICE" {
            continue;
        }
        let indices: Vec<usize> = evidence
            .iter()
            .enumerate()
            .filter(|(_, item)| item.rule_id == rule.id)
            .map(|(index, _)| index)
            .collect();
        if indices.is_empty() {
            continue;
        }
        let hit = indices
            .iter()
            .any(|index| !evidence[*index].value.is_null());
        let parts = StaleKeyParts {
            target_id: &target.raw.id,
            url: &target.raw.url,
            surface: &target.raw.surface,
            rule_id: &rule.id,
            rule_version: rule.version,
            family: family.map(observed_family_name).unwrap_or("UNKNOWN"),
            path_id: &path.path_id,
            shape: "ANONYMOUS_EMPTY_COOKIE",
        };
        let freshness = if hit {
            store.hit(&parts, now)
        } else {
            store.miss(&parts, now)
        };
        for index in indices {
            evidence[index].freshness = freshness.clone();
        }
    }
}

fn region_country(evidence: &[crate::contracts::Evidence], layers: &[Layer]) -> Option<String> {
    for layer in layers {
        let mut claims = evidence
            .iter()
            .filter(|item| item.layer.region_layer() == Some(*layer))
            .filter_map(country_claim);
        if let Some(first) = claims.next() {
            return claims.all(|country| country == first).then_some(first);
        }
    }
    None
}

fn preference_value(evidence: &[crate::contracts::Evidence]) -> Option<String> {
    // gl 是地区版次；没有它时才展示语言。不从 hl 推国家，不选多个值中的首项。
    for claim in [
        "edition.gl",
        "edition.gl.param",
        "store_region_flag",
        "edition.hl",
    ] {
        let mut values = evidence
            .iter()
            .filter(|item| {
                item.layer.region_layer() == Some(Layer::Preference) && item.claim == claim
            })
            .filter_map(|item| item.value.as_str());
        if let Some(first) = values.next() {
            return values.all(|other| other == first).then(|| {
                if claim == "edition.hl" {
                    first.to_string()
                } else {
                    normalize_country(first).unwrap_or_else(|| first.to_string())
                }
            });
        }
    }
    None
}

fn region_layers(
    evidence: &[crate::contracts::Evidence],
    _observed_country: Option<&str>,
    edition: Option<&str>,
) -> Vec<LayerValue> {
    Layer::ORDER
        .iter()
        .map(|layer| {
            let value = match layer {
                Layer::EgressNetwork => region_country(evidence, &[Layer::EgressNetwork]),
                Layer::ServicePolicy => region_country(evidence, &[Layer::ServicePolicy]),
                Layer::Preference => edition.map(|text| text.to_string()),
                Layer::Account | Layer::Payments | Layer::Cloud => None,
                Layer::Device => None,
            };
            let reason = match layer {
                Layer::Account | Layer::Payments | Layer::Cloud => {
                    Some(reason::NOT_OBSERVABLE.to_string())
                }
                Layer::Device => Some(reason::NOT_COLLECTED.to_string()),
                Layer::Preference if value.is_some() => Some(reason::PUBLIC_EDITION.to_string()),
                Layer::EgressNetwork | Layer::ServicePolicy if value.is_some() => None,
                Layer::EgressNetwork | Layer::ServicePolicy
                    if evidence.iter().any(|item| {
                        item.layer.region_layer() == Some(*layer) && country_claim(item).is_some()
                    }) =>
                {
                    Some(reason::CONFLICTING_EVIDENCE.to_string())
                }
                _ => Some(reason::NO_REGION_SIGNAL.to_string()),
            };
            LayerValue {
                layer: *layer,
                value,
                reason,
            }
        })
        .collect()
}

fn result_confidence(
    evidence: &[crate::contracts::Evidence],
    state: State,
    outcome: &FetchOutcome,
) -> Confidence {
    let mut best = Confidence::None;
    for item in evidence {
        if item.value.is_null() {
            continue;
        }
        if item.confidence > best {
            best = item.confidence;
        }
    }
    if best != Confidence::None {
        return best;
    }
    match state {
        State::ReachableOnly => {
            if outcome.http_received() {
                Confidence::Low
            } else {
                Confidence::None
            }
        }
        _ => Confidence::None,
    }
}

fn build_expectation(
    spec: &ExpectationSpec,
    state: State,
    observed_country: &Option<String>,
    outcome: &FetchOutcome,
) -> Expectation {
    match spec {
        ExpectationSpec::State(expected) => {
            let actual = state.as_str().to_string();
            Expectation {
                key: "state".to_string(),
                expected: expected.clone(),
                status: if actual.eq_ignore_ascii_case(expected) {
                    ExpectationStatus::Match
                } else {
                    ExpectationStatus::Mismatch
                },
                actual: Some(actual),
            }
        }
        ExpectationSpec::ServiceCountry(expected) => {
            let actual = observed_country.clone();
            let status = match &actual {
                None => ExpectationStatus::NotObservable,
                Some(value) if value.eq_ignore_ascii_case(expected) => ExpectationStatus::Match,
                Some(_) => ExpectationStatus::Mismatch,
            };
            Expectation {
                key: "service_country".to_string(),
                expected: expected.to_string(),
                actual,
                status,
            }
        }
        ExpectationSpec::Family(expected) => {
            let actual = outcome.observed_family.map(|family| match family {
                ObservedFamily::V4 => "V4".to_string(),
                ObservedFamily::V6 => "V6".to_string(),
            });
            let status = match &actual {
                None => ExpectationStatus::NotObservable,
                Some(value) if value.eq_ignore_ascii_case(expected) => ExpectationStatus::Match,
                Some(_) => ExpectationStatus::Mismatch,
            };
            Expectation {
                key: "family".to_string(),
                expected: expected.to_string(),
                actual,
                status,
            }
        }
    }
}

/// 七层全空的合法结果（未启动/不支持时使用）。
pub fn all_layers_null() -> Vec<LayerValue> {
    Layer::ORDER
        .iter()
        .map(|layer| LayerValue {
            layer: *layer,
            value: None,
            reason: Some(match layer {
                Layer::Account | Layer::Payments | Layer::Cloud => {
                    reason::NOT_OBSERVABLE.to_string()
                }
                Layer::Device => reason::NOT_COLLECTED.to_string(),
                _ => reason::NO_REGION_SIGNAL.to_string(),
            }),
        })
        .collect()
}

/// 未观察任何量纲时的 `--expect` 比较结果。
pub fn expectation_for_empty(spec: &ExpectationSpec) -> Expectation {
    match spec {
        ExpectationSpec::State(expected) => Expectation {
            key: "state".to_string(),
            expected: expected.clone(),
            actual: Some("UNKNOWN".to_string()),
            status: if expected.eq_ignore_ascii_case("UNKNOWN") {
                ExpectationStatus::Match
            } else {
                ExpectationStatus::Mismatch
            },
        },
        ExpectationSpec::ServiceCountry(expected) => Expectation {
            key: "service_country".to_string(),
            expected: expected.clone(),
            actual: None,
            status: ExpectationStatus::NotObservable,
        },
        ExpectationSpec::Family(expected) => Expectation {
            key: "family".to_string(),
            expected: expected.clone(),
            actual: None,
            status: ExpectationStatus::NotObservable,
        },
    }
}

/// 覆盖集合中的已覆盖项。
pub fn covered(result: &ResultRecord, http_received: bool) -> Covered {
    Covered {
        target: result.target.clone(),
        request_count: result.request_count,
        http_received,
    }
}
