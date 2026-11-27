//! 仓库级契约：`plugins/samples/` 下的示例插件必须能被真实加载器读取、
//! 与 `THIRD_PARTY_PLUGINS.md` 的声明逐字节一致，并保持“轻/重”两种形态的区分。
//!
//! 这里刻意复用 `plugins::read_directory`（宿主实际使用的入口），而不是在测试里
//! 另写一套解析，避免测试与生产路径漂移。

#![cfg(not(any(target_os = "android", target_os = "ios")))]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use parallaxe_core::plugins::{self, Component};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn collect_manifests(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_manifests(&path, found);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("manifest.json") {
            found.push(path);
        }
    }
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().trim_matches('`').trim().to_string())
        .collect()
}

struct NoticeRow {
    id: String,
    permissions: String,
    sha256: String,
}

fn notice_rows(path: &Path) -> BTreeMap<String, NoticeRow> {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut columns: Option<Vec<String>> = None;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        match &columns {
            None => {
                if trimmed.starts_with('|') {
                    let header = cells(trimmed);
                    if header.first().map(String::as_str) == Some("插件 ID") {
                        columns = Some(header);
                    }
                }
            }
            Some(names) => {
                if !trimmed.starts_with('|') {
                    break;
                }
                let row = cells(trimmed);
                if row
                    .iter()
                    .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
                {
                    continue;
                }
                assert_eq!(
                    row.len(),
                    names.len(),
                    "table row width differs from the header: {trimmed}"
                );
                let at = |name: &str| {
                    let index = names.iter().position(|column| column == name);
                    index.unwrap_or_else(|| panic!("missing table column {name}"))
                };
                let key = row[at("manifest 路径")].clone();
                assert!(
                    rows.insert(
                        key.clone(),
                        NoticeRow {
                            id: row[at("插件 ID")].clone(),
                            permissions: row[at("权限")].clone(),
                            sha256: row[at("sha256")].clone(),
                        }
                    )
                    .is_none(),
                    "duplicate notice row for {key}"
                );
            }
        }
    }
    assert!(columns.is_some(), "plugin table header missing");
    assert!(!rows.is_empty(), "plugin table has no entries");
    rows
}

fn normalize_permissions(cell: &str) -> String {
    let compact: String = cell
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '，')
        .collect();
    match compact.as_str() {
        "" | "无" | "none" | "-" => String::new(),
        other => other.to_string(),
    }
}

#[test]
fn samples_load_and_match_public_notices() {
    let root = repo_root();
    let mut manifests = Vec::new();
    collect_manifests(&root.join("plugins"), &mut manifests);
    assert!(!manifests.is_empty(), "no plugin manifests under plugins/");

    let rows = notice_rows(&root.join("THIRD_PARTY_PLUGINS.md"));
    let mut declared = BTreeSet::new();

    for path in &manifests {
        let directory = path.parent().expect("manifest parent");
        let (bytes, manifest) = plugins::read_directory(directory)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let key = path
            .strip_prefix(&root)
            .expect("manifest under repo root")
            .to_string_lossy()
            .replace('\\', "/");

        let row = rows
            .get(&key)
            .unwrap_or_else(|| panic!("{key} is not declared in THIRD_PARTY_PLUGINS.md"));
        declared.insert(key.clone());

        assert_eq!(row.id, manifest.id, "{key}: declared id differs");
        assert_eq!(
            row.sha256.to_lowercase(),
            plugins::digest(&bytes),
            "{key}: declared sha256 differs from the exact manifest bytes"
        );
        let permissions: String = manifest
            .permissions
            .iter()
            .map(|permission| match permission {
                plugins::Permission::ReportCounts => "report_counts",
            })
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            normalize_permissions(&row.permissions),
            permissions,
            "{key}: declared permissions differ"
        );
        assert!(
            manifest.pages.len() <= 16 && manifest.operations.len() <= 64,
            "{key}: sample exceeds protocol budgets"
        );
    }

    let orphans: Vec<&String> = rows.keys().filter(|key| !declared.contains(*key)).collect();
    assert!(
        orphans.is_empty(),
        "notice rows without a manifest on disk: {orphans:?}"
    );
}

#[test]
fn light_sample_stays_light_and_heavy_sample_keeps_a_page() {
    let root = repo_root();
    let samples = root.join("plugins").join("samples");
    let (_, light) = plugins::read_directory(&samples.join("light-report-badge")).expect("light");
    let (_, heavy) = plugins::read_directory(&samples.join("heavy-report-guide")).expect("heavy");

    assert!(light.pages.is_empty(), "a light plugin must not add a page");
    assert!(
        !light.operations.is_empty(),
        "a light plugin must change at least one display operation"
    );

    let page = heavy.pages.first().expect("a heavy plugin must add a page");
    assert!(
        page.components.len() >= 5,
        "the heavy sample must demonstrate several allowed components"
    );
    let kinds: Vec<&str> = page
        .components
        .iter()
        .map(|component| match component {
            Component::Heading { .. } => "heading",
            Component::Badge { .. } => "badge",
            Component::Callout { .. } => "callout",
            Component::Markdown { .. } => "markdown",
            Component::Code { .. } => "code",
            Component::Progress { .. } => "progress",
            Component::Table { .. } => "table",
            Component::Form { .. } => "form",
            Component::Button { .. } => "button",
        })
        .collect();
    for expected in [
        "heading", "badge", "callout", "markdown", "code", "progress", "table", "form", "button",
    ] {
        assert!(
            kinds.contains(&expected),
            "the heavy sample must demonstrate a {expected} block"
        );
    }
}

#[test]
fn missing_or_unknown_provenance_fails_closed() {
    let path = repo_root()
        .join("plugins")
        .join("samples")
        .join("light-report-badge")
        .join("manifest.json");
    let raw = std::fs::read(&path).expect("sample manifest");
    let value: serde_json::Value = serde_json::from_slice(&raw).expect("json");
    assert!(
        plugins::Manifest::parse(&serde_json::to_vec(&value).expect("encode")).is_ok(),
        "the shipped sample must itself be valid"
    );
    for field in ["author", "license", "source"] {
        let mut broken = value.clone();
        broken
            .as_object_mut()
            .expect("object")
            .remove(field)
            .unwrap_or_else(|| panic!("sample must declare {field}"));
        let _ = plugins::Manifest::parse(&serde_json::to_vec(&broken).expect("encode"))
            .expect_err("a manifest without provenance must be rejected");
    }
    let mut anonymous = value.clone();
    anonymous["source"] = serde_json::Value::String("https://user:secret@example.com/".into());
    assert!(plugins::Manifest::parse(&serde_json::to_vec(&anonymous).expect("encode")).is_err());
    let mut unknown = value.clone();
    unknown["network"] = serde_json::Value::Bool(true);
    assert!(plugins::Manifest::parse(&serde_json::to_vec(&unknown).expect("encode")).is_err());
}
