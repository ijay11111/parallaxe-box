//! 目录/规则/风格数据的硬约束：数量、许可、选择器与外部覆盖不可提权。

use std::fs;
use std::path::PathBuf;

use parallaxe_core::contracts::reason;
use parallaxe_core::targets::{LoadPaths, Registry};

fn registry() -> Registry {
    Registry::embedded().expect("内嵌资产应通过校验")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "parallaxe-test-{name}-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn catalog_counts_and_permissions() {
    let registry = registry();
    assert_eq!(registry.catalog_size(), 135);
    assert_eq!(
        registry
            .entries()
            .iter()
            .filter(|entry| entry.raw.id.starts_with("google."))
            .count(),
        107
    );
    assert_eq!(
        registry
            .entries()
            .iter()
            .filter(|entry| entry.raw.id.starts_with("control."))
            .count(),
        28
    );
    let google_families: std::collections::HashSet<&str> = registry
        .entries()
        .iter()
        .filter(|entry| entry.raw.id.starts_with("google."))
        .map(|entry| entry.raw.family.as_str())
        .collect();
    assert_eq!(google_families.len(), 12, "Google 目录应覆盖 12 个 family");
    assert!(registry
        .entries()
        .iter()
        .all(|entry| !entry.raw.allow_online && !entry.raw.safe_default));
    let protected = registry
        .entries()
        .iter()
        .filter(|entry| entry.raw.protected)
        .count();
    assert_eq!(protected, 110, "82 Google + 28 controls 全部受保护");
}

#[test]
fn adminission_is_denied_for_all_without_grants() {
    let registry = registry();
    let now = "2026-11-21T00:00:00Z";
    for entry in registry.entries() {
        let admission = registry.admission(entry, false, now);
        assert!(!admission.admitted, "空许可下不得准入 {}", entry.id());
        assert_eq!(admission.reason, Some(reason::ALLOW_ONLINE_FALSE));
    }
}

#[test]
fn selection_sizes_match_profiles() {
    let registry = registry();
    let cases = [("simple", 16), ("google-all", 107), ("full", 135)];
    for (profile, expected) in cases {
        let selection = registry
            .select(Some(profile), &[], &[], false)
            .expect("profile");
        assert_eq!(selection.ids.len(), expected, "profile {profile}");
    }
    let group = registry
        .select(None, &["google.ai".to_string()], &[], false)
        .expect("group");
    assert!(!group.ids.is_empty());
    assert!(group
        .ids
        .iter()
        .all(|id| registry.get(id).map(|entry| entry.raw.group.as_str()) == Some("google.ai")));
}

#[test]
fn selection_errors_are_usage_errors() {
    let registry = registry();
    assert!(registry.select(Some("nope"), &[], &[], false).is_err());
    assert!(registry
        .select(None, &["nope.group".to_string()], &[], false)
        .is_err());
    assert!(registry
        .select(None, &[], &["nope.target".to_string()], false)
        .is_err());
}

#[test]
fn signal_rules_bind_both_ways_and_negative_is_unreferenced() {
    let registry = registry();
    let rules = registry.rules();
    assert_eq!(rules.len(), 15);
    assert_eq!(rules.iter().filter(|rule| rule.enabled).count(), 14);
    let legacy = registry.rule("rule.legacy-region").expect("negative rule");
    assert!(!legacy.enabled);
    assert!(registry.entries().iter().all(|entry| !entry
        .raw
        .signals
        .iter()
        .any(|id| id == "rule.legacy-region")));
    for entry in registry.entries() {
        for rule in registry.rules_for(entry) {
            assert!(
                entry.raw.signals.contains(&rule.id),
                "作用域与目标引用必须双向一致：{} {}",
                entry.id(),
                rule.id
            );
        }
    }
}

#[test]
fn external_targets_layer_cannot_carry_admission() {
    let dir = temp_dir("admission");
    let file = dir.join("override.toml");
    fs::write(
        &file,
        r#"
schema_version = 1
[defaults]
method = "GET"
precondition = "ANONYMOUS_ONLY"
request_budget = 1
max_body_bytes = 16777216
max_redirects = 0
stale_misses = 3
source_type = "DIRECT_ANONYMOUS_OBSERVATION"
verified_at = 2026-11-21
[admission]
grants = [
  { target_id = "google.home", method = "GET", exact_url = "https://support.google.com/websearch/", surface = "x", purpose = "x", policy_ref = "x", reviewed_by = "x", reviewed_at = 2026-01-01, valid_until = 2030-01-01 }
]
"#,
    )
    .expect("write override");
    let error = Registry::load(&LoadPaths {
        targets_dir: Some(dir.clone()),
        ..Default::default()
    })
    .expect_err("外部许可必须被拒绝");
    assert!(error.to_string().contains("admission"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn external_profile_cannot_raise_budget() {
    let dir = temp_dir("profile");
    let file = dir.join("profile.toml");
    fs::write(
        &file,
        r#"
schema_version = 1
[limits]
max_concurrency = 6
per_host = 2
logical_gets_per_target = 1
extra_attempts_per_target = 1
max_redirects = 0
max_body_bytes = 16777216
v6_fallback_after_ms = 8000
[[profiles]]
id = "simple"
run_timeout_ms = 20000
request_timeout_ms = 10000
max_concurrency = 6
per_host = 2
"#,
    )
    .expect("write profile");
    let error = Registry::load(&LoadPaths {
        profiles_dir: Some(dir.clone()),
        ..Default::default()
    })
    .expect_err("放宽预算必须被拒绝");
    assert!(error.to_string().contains("放宽"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn valid_external_target_addition_is_registered_without_permission() {
    let dir = temp_dir("add");
    let file = dir.join("extra.toml");
    fs::write(
        &file,
        r#"
schema_version = 1
[defaults]
method = "GET"
precondition = "ANONYMOUS_ONLY"
request_budget = 1
max_body_bytes = 16777216
max_redirects = 0
stale_misses = 3
source_type = "DIRECT_ANONYMOUS_OBSERVATION"
verified_at = 2026-11-21
[[targets]]
id = "google.extra-test"
family = "content"
group = "google.content"
matrix_group = "B"
url = "https://extra.example.invalid/"
surface = "测试表面"
signals = ["rule.http"]
confidence = "NONE"
confidence_basis = "测试"
failure = "测试"
protected = true
verification = "NO_HTTP"
allow_online = false
safe_default = false
"#,
    )
    .expect("write extra");
    // 规则表不引用该目标 → 双向一致性校验必须拒绝（避免只加目标就能扩权/漂移）。
    assert!(Registry::load(&LoadPaths {
        targets_dir: Some(dir.clone()),
        ..Default::default()
    })
    .is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn nine_states_are_exactly_defined() {
    let names: Vec<&str> = parallaxe_core::State::ALL
        .iter()
        .map(|state| state.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "OK",
            "RESTRICTED",
            "BLOCKED",
            "REACHABLE_ONLY",
            "LOCAL_PATH_ERROR",
            "CONFLICT",
            "UNKNOWN",
            "SIGNAL_STALE",
            "UNSUPPORTED"
        ]
    );
}

#[test]
fn duplicate_overlay_target_is_rejected_before_merge() {
    let mut file: toml::Value =
        toml::from_str(include_str!("../../../assets/targets.toml")).unwrap();
    let row = file["targets"][0].clone();
    file["targets"] = toml::Value::Array(vec![row.clone(), row]);
    file.as_table_mut().unwrap().remove("controls");
    let dir = temp_dir("duplicate");
    fs::write(dir.join("duplicate.toml"), toml::to_string(&file).unwrap()).unwrap();
    let err = Registry::load(&LoadPaths {
        targets_dir: Some(dir.clone()),
        ..Default::default()
    })
    .unwrap_err();
    assert!(err.to_string().contains("同层目标 ID 重复"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn non_file_overlay_is_not_silently_skipped() {
    let dir = temp_dir("bad-type");
    fs::create_dir(dir.join("not-a-file.toml")).unwrap();
    assert!(Registry::load(&LoadPaths {
        targets_dir: Some(dir.clone()),
        ..Default::default()
    })
    .is_err());
    fs::remove_dir_all(dir).unwrap();
}
