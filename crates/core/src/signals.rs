//! `signals`：对限定表面执行白名单 reader，提取最小字段。
//!
//! 规则只来自 `signal-rules.toml`；本模块不跨服务借值、不保存正文、不补抓。

use serde_json::Value;

use crate::contracts::{reason, Confidence, Evidence, Freshness, LayerRef, Source};
use crate::probe::FetchOutcome;
use crate::targets::{Detection, Extractor, Registry, Rule, Target};

/// 一次响应样本（脱敏后只保留解析所需字段）。
#[derive(Debug, Clone)]
pub struct Sample {
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub body: Vec<u8>,
    pub complete: bool,
    pub truncated: bool,
    pub observed_family: Option<crate::contracts::ObservedFamily>,
}

impl Sample {
    /// 从传输结果构造样本。
    pub fn from_outcome(outcome: &FetchOutcome) -> Self {
        Sample {
            status: outcome.status,
            content_type: outcome.content_type.clone(),
            location: outcome.location.clone(),
            body: outcome.body.clone(),
            complete: outcome.complete,
            truncated: outcome.truncated,
            observed_family: outcome.observed_family,
        }
    }

    /// 去掉 MIME 参数后的小写类型。
    pub fn mime(&self) -> Option<String> {
        self.content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase())
    }

    /// 正文按 UTF-8 有损转换（不保存，只在解析期使用）。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 停步原因（安全策略，不是国家证据）。
#[derive(Debug, Clone)]
pub struct Stop {
    pub reason: &'static str,
    pub markers: Vec<&'static str>,
}

/// 提取结果。
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub evidence: Vec<Evidence>,
    pub stop: Option<Stop>,
    /// 是否允许做正文内容提取（完整、未截断、类型与长度合格）。
    pub content_allowed: bool,
    pub surface: String,
}

/// 解析 Location 的主机与路径。
pub fn split_location(location: &str) -> Option<(String, String)> {
    let url = url::Url::parse(location).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some((
        url.host_str()?.trim_end_matches('.').to_string(),
        url.path().to_string(),
    ))
}

// 白名单键也不能承载任意长文本、查询串、地址或认证信息。
fn edition_param(key: &str, value: &str) -> bool {
    let lang =
        |s: &str| !s.is_empty() && s.len() <= 8 && s.bytes().all(|c| c.is_ascii_alphabetic());
    let region = |s: &str| s.len() == 2 && lang(s);
    match key {
        "gl" => region(value),
        "hl" => value.len() <= 16 && value.split('-').all(lang),
        "ceid" => value
            .split_once(':')
            .is_some_and(|(cc, l)| region(cc) && lang(l)),
        _ => false,
    }
}

fn safe_scalar(key: &str, value: &str) -> bool {
    match key {
        "country" | "loc" => value.len() == 2 && value.bytes().all(|b| b.is_ascii_alphabetic()),
        "colo" => value.len() == 3 && value.bytes().all(|b| b.is_ascii_alphabetic()),
        "asn" => {
            !value.is_empty() && value.len() <= 10 && value.bytes().all(|b| b.is_ascii_digit())
        }
        "http" => matches!(value, "http/1.0" | "http/1.1" | "http/2" | "http/3"),
        "tls" => matches!(value, "TLSv1" | "TLSv1.1" | "TLSv1.2" | "TLSv1.3"),
        _ => false,
    }
}

/// 依据 detection 判定是否必须停步（先传输、后要求，再解析）。
fn detect(sample: &Sample, detection: &Detection, location: Option<&url::Url>) -> Option<Stop> {
    let status = sample.status?;
    if detection.rate_limit_status.contains(&status) {
        return Some(Stop {
            reason: reason::RATE_LIMITED,
            markers: vec!["RATE_LIMITED"],
        });
    }
    if detection.auth_or_denied_status.contains(&status) {
        return Some(Stop {
            reason: reason::DENIED_OR_AUTH,
            markers: vec!["AUTH_OR_DENIED"],
        });
    }
    if let Some(location) = location {
        if let Some((host, path)) = split_location(location.as_str()) {
            let login_host = detection.login_hosts.contains(&host);
            let context_host = detection.login_context_hosts.contains(&host);
            let login_path = detection
                .login_path_prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix.as_str()));
            if login_host || (context_host && login_path) {
                return Some(Stop {
                    reason: reason::REDIRECT_TO_LOGIN,
                    markers: vec!["LOGIN_HANDOFF"],
                });
            }
            if detection.consent_hosts.contains(&host) {
                return Some(Stop {
                    reason: reason::CONSENT_REQUIRED,
                    markers: vec!["CONSENT_WALL"],
                });
            }
            if detection
                .captcha_path_prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix.as_str()))
            {
                return Some(Stop {
                    reason: reason::CAPTCHA_CHALLENGE,
                    markers: vec!["CAPTCHA"],
                });
            }
        }
    }
    let mime = sample.mime().unwrap_or_default();
    let looks_textual = mime.contains("html") || mime.contains("json") || mime.starts_with("text/");
    if looks_textual && !sample.truncated {
        let text = sample.text();
        if detection
            .consent_markers
            .iter()
            .any(|marker| text.contains(marker))
        {
            return Some(Stop {
                reason: reason::CONSENT_REQUIRED,
                markers: vec!["CONSENT_WALL"],
            });
        }
        if detection
            .challenge_markers
            .iter()
            .any(|marker| text.contains(marker))
        {
            return Some(Stop {
                reason: reason::CHALLENGE_PAGE,
                markers: vec!["CHALLENGE"],
            });
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn evidence(
    target: &Target,
    rule: &Rule,
    sample_index: u64,
    path_id: &str,
    observed_at: &str,
    fixture: bool,
    claim: &str,
    value: Value,
    reason_text: Option<&str>,
    confidence: Confidence,
) -> Evidence {
    Evidence {
        id: format!("ev-{}-{}-{}", target.raw.id, rule.id, sample_index),
        target_id: target.raw.id.clone(),
        sample_id: format!("smp-{}-1", target.raw.id),
        rule_id: rule.id.clone(),
        rule_version: rule.version,
        observed_at: observed_at.to_string(),
        layer: if rule.channel == "TRANSPORT" {
            LayerRef::Transport(crate::contracts::TransportLayer::Transport)
        } else {
            LayerRef::Layer(rule.layer)
        },
        claim: claim.to_string(),
        value,
        source_type: rule.source_grade,
        source_ref: format!("assets/signal-rules.toml#{}", rule.id),
        confidence,
        path_id: path_id.to_string(),
        origin: if fixture {
            "fixture".to_string()
        } else {
            "live".to_string()
        },
        reason: reason_text.map(|text| text.to_string()),
        fixture,
        freshness: Freshness::not_tracked(),
    }
}

fn content_ok(sample: &Sample, detection: &Detection) -> bool {
    if !matches!(sample.status, Some(200..=299)) || !sample.complete || sample.truncated {
        return false;
    }
    if sample.body.len() as u64 > detection.max_body_bytes {
        return false;
    }
    let Some(mime) = sample.mime() else {
        return false;
    };
    let allowed = detection
        .allowed_html_types
        .iter()
        .chain(detection.allowed_json_types.iter())
        .any(|candidate| *candidate == mime)
        || mime == "text/plain";
    if !allowed {
        return false;
    }
    sample.body.len() as u64 >= detection.min_text_bytes
}

/// 枚举某目标启用的规则，逐条提取。
pub fn evaluate(
    registry: &Registry,
    target: &Target,
    sample: &Sample,
    path_id: &str,
    observed_at: &str,
    fixture: bool,
) -> Evaluation {
    let detection = &registry.rules.detection;
    // 相对 Location 相对于本目标解析；不跟跳，不采集用户信息或片段。
    let location = sample
        .location
        .as_deref()
        .and_then(|loc| url::Url::parse(&target.raw.url).ok()?.join(loc).ok());
    let stop = detect(sample, detection, location.as_ref());
    let content_allowed = stop.is_none() && content_ok(sample, detection);
    let mut out = Vec::new();
    let rules = registry.rules_for(target);
    for rule in &rules {
        let kind = rule.extract.kind.as_str();
        if sample.status.is_none() || (rule.channel == "SERVICE" && stop.is_some()) {
            continue;
        }
        match kind {
            "status" => {
                if let Some(status) = sample.status {
                    out.push(evidence(
                        target,
                        rule,
                        out.len() as u64 + 1,
                        path_id,
                        observed_at,
                        fixture,
                        "http_status",
                        Value::from(status as u64),
                        None,
                        rule.credibility,
                    ));
                }
            }
            "header" => {
                let value = match rule.extract.pattern.to_ascii_lowercase().as_str() {
                    "content-type" => sample.mime().filter(|v| {
                        v.len() <= 127
                            && v.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"/+.-".contains(&b))
                    }),
                    _ => None,
                };
                if let Some(value) = value {
                    out.push(evidence(
                        target,
                        rule,
                        out.len() as u64 + 1,
                        path_id,
                        observed_at,
                        fixture,
                        "header_value",
                        Value::String(value),
                        None,
                        rule.credibility,
                    ));
                }
            }
            "redirect_host" => {
                if let Some(location) = location
                    .as_ref()
                    .filter(|_| matches!(sample.status, Some(300..=399)))
                {
                    if let Some((host, _)) = split_location(location.as_str()) {
                        out.push(evidence(
                            target,
                            rule,
                            out.len() as u64 + 1,
                            path_id,
                            observed_at,
                            fixture,
                            "redirect_host",
                            Value::String(host),
                            None,
                            rule.credibility,
                        ));
                    }
                }
            }
            "redirect_param" => {
                if let Some(location) = location.as_ref().filter(|url| {
                    matches!(sample.status, Some(300..=399))
                        && split_location(url.as_str()).is_some()
                }) {
                    for key in rule.extract.pattern.split(',').map(str::trim) {
                        if let Some((_, value)) = location
                            .query_pairs()
                            .find(|(name, value)| name == key && edition_param(key, value))
                        {
                            out.push(evidence(
                                target,
                                rule,
                                out.len() as u64 + 1,
                                path_id,
                                observed_at,
                                fixture,
                                &format!("edition.{key}"),
                                Value::String(value.into_owned()),
                                Some(reason::PUBLIC_EDITION),
                                rule.credibility,
                            ));
                        }
                    }
                }
            }
            "body_length" if sample.complete && !sample.truncated => {
                out.push(evidence(
                    target,
                    rule,
                    out.len() as u64 + 1,
                    path_id,
                    observed_at,
                    fixture,
                    "body_bytes",
                    Value::from(sample.body.len() as u64),
                    None,
                    rule.credibility,
                ));
            }
            "regex" | "text_label" | "text_kv" | "json" => {
                let mime = sample.mime().unwrap_or_default();
                let compatible = match kind {
                    "json" => detection.allowed_json_types.contains(&mime),
                    "text_kv" => mime == "text/plain",
                    _ => detection.allowed_html_types.contains(&mime),
                };
                if !content_allowed || !compatible {
                    continue;
                }
                let text = String::from_utf8_lossy(&sample.body);
                let hit = match kind {
                    "regex" => {
                        let claim = match rule.id.as_str() {
                            "rule.inline3" => "inline3_country",
                            "rule.yt-gl" => "edition.gl",
                            "rule.flag" => "store_region_flag",
                            _ => "regex_value",
                        };
                        let confidence = rule.credibility;
                        let mut hits = 0usize;
                        if let Ok(regex) = regex::Regex::new(&rule.extract.pattern) {
                            let mut seen = Vec::new();
                            for captures in regex.captures_iter(&text) {
                                let value = match rule.extract.capture.as_str() {
                                    "1" => captures.get(1).map(|item| item.as_str().to_string()),
                                    _ => captures.get(0).map(|item| item.as_str().to_string()),
                                };
                                let Some(value) = value else { continue };
                                if seen.contains(&value) {
                                    continue;
                                }
                                seen.push(value.clone());
                                hits += 1;
                                out.push(evidence(
                                    target,
                                    rule,
                                    out.len() as u64 + 1,
                                    path_id,
                                    observed_at,
                                    fixture,
                                    claim,
                                    Value::String(value),
                                    None,
                                    confidence,
                                ));
                                if hits == 4 {
                                    break;
                                }
                            }
                        }
                        hits > 0
                    }
                    "text_label" => {
                        if text.contains(&rule.extract.pattern) {
                            out.push(evidence(
                                target,
                                rule,
                                out.len() as u64 + 1,
                                path_id,
                                observed_at,
                                fixture,
                                "search_source_label",
                                Value::String(rule.extract.pattern.clone()),
                                None,
                                rule.credibility,
                            ));
                            true
                        } else {
                            false
                        }
                    }
                    "text_kv" => {
                        let keys: Vec<&str> =
                            rule.extract.pattern.split(',').map(str::trim).collect();
                        let mut hits = 0usize;
                        for line in text.lines().take(4096) {
                            let Some((key, value)) = line.split_once('=') else {
                                continue;
                            };
                            let key = match key.trim() {
                                "ip" => "ip_present",
                                "ip_present" => continue,
                                other => other,
                            };
                            if !keys.contains(&key) {
                                continue;
                            }
                            let value = value.trim();
                            if key != "ip_present" && !safe_scalar(key, value) {
                                continue;
                            }
                            hits += 1;
                            let (claim, value, confidence) = if key == "ip_present" {
                                (
                                    "trace.ip_present".to_string(),
                                    Value::Bool(!value.is_empty()),
                                    rule.credibility,
                                )
                            } else {
                                (
                                    format!("trace.{key}"),
                                    Value::String(value.to_string()),
                                    rule.credibility,
                                )
                            };
                            out.push(evidence(
                                target,
                                rule,
                                out.len() as u64 + 1,
                                path_id,
                                observed_at,
                                fixture,
                                &claim,
                                value,
                                None,
                                confidence,
                            ));
                        }
                        hits > 0
                    }
                    "json" => {
                        let mut hits = 0usize;
                        let parsed = serde_json::from_slice::<Value>(&sample.body);
                        let Ok(Value::Object(map)) = parsed else {
                            continue;
                        };
                        for key in rule.extract.pattern.split(',') {
                            let key = key.trim();
                            if let Some(field) = map.get(key) {
                                let normalized = match field {
                                    Value::String(text) if safe_scalar(key, text) => {
                                        Value::String(text.clone())
                                    }
                                    Value::Number(number) if key == "asn" && number.is_u64() => {
                                        Value::String(number.to_string())
                                    }
                                    _ => continue,
                                };
                                hits += 1;
                                out.push(evidence(
                                    target,
                                    rule,
                                    out.len() as u64 + 1,
                                    path_id,
                                    observed_at,
                                    fixture,
                                    &format!("meta.{key}"),
                                    normalized,
                                    None,
                                    rule.credibility,
                                ));
                            }
                        }
                        hits > 0
                    }
                    _ => false,
                };
                if !hit && stop.is_none() {
                    // 合格样本上的字段缺失产生诊断证据（value=null）。
                    out.push(evidence(
                        target,
                        rule,
                        out.len() as u64 + 1,
                        path_id,
                        observed_at,
                        fixture,
                        &format!("{}.missing", rule.id),
                        Value::Null,
                        Some(reason::FIELD_MISSING),
                        Confidence::None,
                    ));
                }
            }
            _ => {}
        }
    }
    Evaluation {
        evidence: out,
        stop,
        content_allowed,
        surface: target.raw.surface.clone(),
    }
}

/// 供离线 fixture 解析 `response.http` 文本（空行后为正文，按字节切分）。
pub fn parse_http_text(text: &str) -> Option<Sample> {
    let (head, body) = if let Some(index) = text.find("\r\n\r\n") {
        (&text[..index], &text[index + 4..])
    } else if let Some(index) = text.find("\n\n") {
        (&text[..index], &text[index + 2..])
    } else {
        (text, "")
    };
    let mut lines = head.lines();
    let status_line = lines.next()?;
    let status = status_line.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    let mut content_type = None;
    let mut location = None;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "content-type" => content_type = Some(value.trim().to_string()),
            "location" => location = Some(value.trim().to_string()),
            _ => {}
        }
    }
    Some(Sample {
        status: Some(status),
        content_type,
        location,
        body: body.as_bytes().to_vec(),
        complete: true,
        truncated: false,
        observed_family: None,
    })
}

/// 便捷：判断某条证据是否携带可投票的国家值。
pub fn country_claim(evidence: &Evidence) -> Option<String> {
    if evidence.value.is_null() {
        return None;
    }
    if !matches!(
        evidence.claim.as_str(),
        "inline3_country" | "trace.loc" | "meta.country"
    ) {
        return None;
    }
    let raw = match &evidence.value {
        Value::String(text) => text.clone(),
        _ => return None,
    };
    crate::contracts::normalize_country(&raw)
}

/// 校验抽取器字段（供配置测试）。
pub fn extractor_kind_known(extractor: &Extractor) -> bool {
    matches!(
        extractor.kind.as_str(),
        "status"
            | "header"
            | "redirect_host"
            | "redirect_param"
            | "regex"
            | "text_label"
            | "body_length"
            | "text_kv"
            | "json"
    )
}

/// 导出 Source 便于测试断言。
pub fn source_of(rule: &Rule) -> Source {
    rule.source_grade
}
