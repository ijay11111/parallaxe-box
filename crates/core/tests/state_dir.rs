//! `--state-dir` 边界：默认不写、显式才启用 N=3，文件只存摘要。

use std::fs;
use std::path::PathBuf;

use parallaxe_core::contracts::FreshnessStatus;
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::state::{DirStore, FreshnessStore, NullStore, StaleKeyParts};
use parallaxe_core::targets::Registry;
use time::OffsetDateTime;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "parallaxe-state-{name}-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    dir
}

fn parts<'a>() -> StaleKeyParts<'a> {
    StaleKeyParts {
        target_id: "google.gemini",
        url: "https://gemini.google.com/",
        surface: "surface",
        rule_id: "rule.inline3",
        rule_version: 1,
        family: "AUTO",
        path_id: "path-system",
        shape: "ANONYMOUS_EMPTY_COOKIE",
    }
}

#[test]
fn null_store_never_reports_stale() {
    let store = NullStore::default();
    assert!(!store.enabled());
    for _ in 0..5 {
        let freshness = store.miss(&parts(), OffsetDateTime::now_utc());
        assert_ne!(freshness.status, FreshnessStatus::Stale);
    }
}

#[test]
fn dir_store_counts_and_saturates() {
    let dir = temp_dir("dir");
    let store = DirStore::open(&dir).expect("open store");
    let now = OffsetDateTime::now_utc();
    let hit = store.hit(&parts(), now);
    assert_eq!(hit.status, FreshnessStatus::Current);
    let first = store.miss(&parts(), now);
    assert_eq!(first.comparable_misses, 1);
    let second = store.miss(&parts(), now);
    assert_eq!(second.comparable_misses, 2);
    let third = store.miss(&parts(), now);
    assert_eq!(third.status, FreshnessStatus::Stale);
    assert_eq!(third.comparable_misses, 3);
    let fourth = store.miss(&parts(), now);
    assert_eq!(fourth.comparable_misses, 3);

    let text = fs::read_to_string(dir.join("freshness-v1.json")).expect("state file");
    assert!(text.contains("parallaxe.freshness/v1"));
    assert!(
        !text.contains("https://gemini.google.com/"),
        "不得保存原 URL"
    );
    assert!(!text.contains("path-system"), "不得保存路径标识");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dir_store_rejects_non_directory() {
    let path = std::env::temp_dir().join(format!(
        "parallaxe-state-file-{}-{}.txt",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    fs::write(&path, "x").expect("write file");
    assert!(DirStore::open(&path).is_err());
    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn fixture_mode_rejects_real_state_dir() {
    let registry = Registry::embedded().expect("registry");
    let dir = temp_dir("fixture-reject");
    let options = CheckOptions {
        fixture: Some("gemini-positive".to_string()),
        state_dir: Some(dir.clone()),
        ..Default::default()
    };
    let error = runtime::run_check(&registry, &options, None, false, None)
        .await
        .expect_err("fixture 不得使用真实状态目录");
    assert!(error.to_string().contains("状态目录"));
    assert!(!dir.exists(), "拒绝时不得创建目录");
}

#[tokio::test]
async fn default_check_does_not_create_state() {
    let registry = Registry::embedded().expect("registry");
    let dir = temp_dir("no-state");
    let options = CheckOptions {
        profile: Some("simple".to_string()),
        state_dir: None,
        ..Default::default()
    };
    let output = runtime::run_check(&registry, &options, None, false, None)
        .await
        .expect("run");
    assert_eq!(output.report.network_request_count, 0);
    assert!(!dir.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn exclusive_session_reopens_with_committed_state() {
    let dir = temp_dir("exclusive");
    let store = DirStore::open(&dir).unwrap();
    let now = OffsetDateTime::now_utc();
    store.hit(&parts(), now);
    assert!(DirStore::open(&dir).is_err());
    drop(store);
    let next = DirStore::open(&dir).unwrap();
    assert_eq!(next.miss(&parts(), now).comparable_misses, 1);
    drop(next);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reserved_temp_file_is_never_truncated() {
    let dir = temp_dir("reserved");
    let store = DirStore::open(&dir).unwrap();
    let temp = dir.join("freshness-v1.json.tmp");
    fs::write(&temp, "sentinel").unwrap();
    store.hit(&parts(), OffsetDateTime::now_utc());
    assert!(store.error().is_some());
    assert_eq!(fs::read_to_string(temp).unwrap(), "sentinel");
    assert!(!dir.join("freshness-v1.json").exists());
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn clock_rollback_and_day_eight_invalidate_misses() {
    let now = OffsetDateTime::now_utc();
    let store = parallaxe_core::state::MemoryStore::default();
    for delta in [-1, 8] {
        store.hit(&parts(), now);
        assert_eq!(
            store
                .miss(&parts(), now + time::Duration::days(delta))
                .comparable_misses,
            0
        );
    }
    store.hit(&parts(), now);
    assert_eq!(
        store
            .miss(&parts(), now + time::Duration::days(7))
            .comparable_misses,
        1
    );
}
