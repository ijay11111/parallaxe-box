//! 内置离线案例（合成样本，`mode=FIXTURE`）。
//!
//! 这些不是在线录制，也不携带真实 IP/Cookie；正式二进制不能由此获得联网能力。
//! 文件形式的最小回归集另见 `tests/fixtures/<target-id>/<case>/`。

use crate::contracts::{ObservedFamily, State};
use crate::signals::Sample;

/// 一个合成离线案例。
#[derive(Debug, Clone)]
pub struct FixtureCase {
    pub id: &'static str,
    pub target_id: &'static str,
    pub case: &'static str,
    pub description: &'static str,
    pub sample: Sample,
    pub expected_state: State,
    pub expected_reason: Option<&'static str>,
    pub expected_country: Option<&'static str>,
    pub expected_request_count: u64,
}

fn sample(
    status: u16,
    content_type: &str,
    location: Option<&str>,
    body: &str,
    truncated: bool,
) -> Sample {
    Sample {
        status: Some(status),
        content_type: Some(content_type.to_string()),
        location: location.map(|text| text.to_string()),
        body: body.as_bytes().to_vec(),
        complete: !truncated,
        truncated,
        observed_family: Some(ObservedFamily::V4),
    }
}

/// 内置案例清单（最小回归集）。
#[allow(clippy::vec_init_then_push)]
pub fn cases() -> Vec<FixtureCase> {
    let mut list = Vec::new();
    list.push(FixtureCase {
        id: "gemini-positive",
        target_id: "google.gemini",
        case: "inline3-positive",
        description: "Gemini 页面内联三字母正例：只支持「此页面有此值」",
        sample: sample(
            200,
            "text/html; charset=utf-8",
            None,
            "<html><title>Gemini</title><script>AF_initDataCallback({data:[,2,1,200,\"USA\"]})</script></html>",
            false,
        ),
        expected_state: State::Ok,
        expected_reason: None,
        expected_country: Some("US"),
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "books-missing",
        target_id: "google.books",
        case: "field-missing",
        description: "Books 有公开标题但没有内联字段：不得借用 Gemini 的值",
        sample: sample(
            200,
            "text/html",
            None,
            "<html><title>Google Books</title></html>",
            false,
        ),
        expected_state: State::ReachableOnly,
        expected_reason: Some(crate::contracts::reason::NO_REGION_SIGNAL),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "legacy-region-negative",
        target_id: "google.home",
        case: "legacy-selector-negative",
        description: "旧 name=\"region\" 已失效：即使出现也零证据",
        sample: sample(
            200,
            "text/html",
            None,
            "<html><title>Google</title><input name=\"region\" value=\"CN\"></html>",
            false,
        ),
        expected_state: State::Unknown,
        expected_reason: Some(crate::contracts::reason::FIELD_MISSING),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "drive-login",
        target_id: "google.drive",
        case: "login-handoff",
        description: "Drive 302 到登录域：停在首跳，地区 null",
        sample: sample(
            302,
            "application/binary",
            Some(
                "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fdrive.google.com",
            ),
            "",
            false,
        ),
        expected_state: State::Unknown,
        expected_reason: Some(crate::contracts::reason::REDIRECT_TO_LOGIN),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "news-edition",
        target_id: "google.news",
        case: "edition-redirect",
        description: "News 一跳的三个参数只算一条版次证据，不作出口",
        sample: sample(
            302,
            "application/binary",
            Some("https://news.google.com/?hl=en-US&gl=US&ceid=US:en"),
            "",
            false,
        ),
        expected_state: State::ReachableOnly,
        expected_reason: Some(crate::contracts::reason::EDITION_ONLY),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "empty-204",
        target_id: "google.http204",
        case: "empty-204",
        description: "204 空体只证明连通性，不能计为 HTML 字段缺失",
        sample: sample(204, "text/plain", None, "", false),
        expected_state: State::ReachableOnly,
        expected_reason: Some(crate::contracts::reason::NO_REGION_SIGNAL),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "rate-429",
        target_id: "google.models",
        case: "rate-limited",
        description: "429 只记限流迹象，不重试、不判封国",
        sample: sample(429, "application/json", None, "{}", false),
        expected_state: State::Unknown,
        expected_reason: Some(crate::contracts::reason::RATE_LIMITED),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "truncated-page",
        target_id: "google.books",
        case: "truncated",
        description: "截断页面不提取内容、不计 stale",
        sample: sample(200, "text/html", None, "<html><title>Google Books", true),
        expected_state: State::Unknown,
        expected_reason: Some(crate::contracts::reason::TRUNCATED),
        expected_country: None,
        expected_request_count: 1,
    });
    list.push(FixtureCase {
        id: "conflict-tie",
        target_id: "google.gemini",
        case: "same-layer-conflict",
        description: "同层两个不同三字母：平票无赢家，状态 CONFLICT",
        sample: sample(
            200,
            "text/html",
            None,
            "<html>,2,1,200,\"USA\",2,1,200,\"CHN\"</html>",
            false,
        ),
        expected_state: State::Conflict,
        expected_reason: Some(crate::contracts::reason::CONFLICTING_EVIDENCE),
        expected_country: None,
        expected_request_count: 1,
    });
    list
}

/// 按案例 ID 查找。
pub fn find(id: &str) -> Option<FixtureCase> {
    cases().into_iter().find(|case| case.id == id)
}

/// 案例 ID 列表（用于错误提示）。
pub fn case_ids() -> Vec<&'static str> {
    cases().iter().map(|case| case.id).collect()
}
