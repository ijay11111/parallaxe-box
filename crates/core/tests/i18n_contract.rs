//! i18n 契约：语言包完整、缺失回退、core 错误本地化。

use parallaxe_core::i18n::{has_key, keys, Locale};
use parallaxe_core::targets::{ConfigError, Registry};

#[test]
fn zh_and_en_packs_have_the_same_keys_except_probe() {
    let zh = keys(Locale::ZhCn);
    let en = keys(Locale::EnUs);
    let mut expected_en = zh.clone();
    // 仅用于验证“缺失 key 回退中文”的探针，不应出现在英文包里。
    expected_en.remove("internal.zh_only_probe");
    assert_eq!(en, expected_en, "zh/en 语言包 key 不对齐");
    assert!(zh.len() > 40, "语言包 key 过少: {}", zh.len());
}

#[test]
fn every_eligibility_summary_has_both_languages() {
    let registry = Registry::embedded().expect("registry");
    let keys = registry.eligibility.summary_keys();
    assert!(keys.len() >= 18, "资格说明过少: {}", keys.len());
    for key in keys {
        assert!(has_key(Locale::ZhCn, &key), "zh-CN 缺少 {key}");
        assert!(has_key(Locale::EnUs, &key), "en-US 缺少 {key}");
    }
}

#[test]
fn cli_and_report_keys_are_present_in_both_languages() {
    let keys = [
        "cli.about",
        "cli.error_prefix",
        "report.col.target",
        "report.col.notes",
        "report.coverage",
        "report.eligibility_disclaimer",
        "targets.protected",
        "explain.target",
        "selftest.pass",
        "layer.egress_network",
        "state.ok",
        "state.signal_stale",
        "agent.error.invalid_argument",
    ];
    for key in keys {
        assert!(has_key(Locale::ZhCn, key), "zh-CN 缺少 {key}");
        assert!(has_key(Locale::EnUs, key), "en-US 缺少 {key}");
    }
}

#[test]
fn config_error_is_localized_without_changing_display() {
    let error = ConfigError::field("targets.toml#id", "目标 ID 重复");
    assert_eq!(error.code(), "CONFIG_FIELD");
    let zh = error.localized(Locale::ZhCn);
    assert!(zh.contains("目标 ID 重复"), "{zh}");
    let en = error.localized(Locale::EnUs);
    assert!(en.starts_with("Field targets.toml#id:"), "{en}");

    let parse = ConfigError::Parse("bad toml".to_string());
    assert!(parse
        .localized(Locale::EnUs)
        .starts_with("Configuration parse failed:"));
    assert_eq!(parse.to_string(), "配置解析失败：bad toml");
}

#[test]
fn locale_serializes_to_bcp47_tokens() {
    assert_eq!(serde_json::to_string(&Locale::ZhCn).unwrap(), "\"zh-CN\"");
    assert_eq!(serde_json::to_string(&Locale::EnUs).unwrap(), "\"en-US\"");
}

#[test]
fn every_translate_call_site_literal_key_exists_in_both_locales() {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let crates_dir = root.join("crates");
    let mut files = Vec::new();
    for member in ["core", "cli", "agent", "gui"] {
        collect_rs_files(&crates_dir.join(member).join("src"), &mut files);
    }
    assert!(
        !files.is_empty(),
        "expected Rust source files under crates/"
    );

    let re = regex::Regex::new(r#"translate\s*\([^,]+,\s*"([a-z0-9_.-]+)"\s*,"#).unwrap();
    let mut discovered = BTreeSet::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        // Exclude inline #[cfg(test)] blocks where intentionally missing keys are probed.
        let prod_text = text.split("#[cfg(test)]").next().unwrap_or(&text);
        for cap in re.captures_iter(prod_text) {
            if let Some(m) = cap.get(1) {
                discovered.insert((m.as_str().to_string(), path.display().to_string()));
            }
        }
    }
    assert!(
        discovered.len() >= 100,
        "expected >= 100 translate() literal call sites, found {}",
        discovered.len()
    );

    let mut missing = Vec::new();
    for (key, file) in discovered {
        if !has_key(Locale::ZhCn, &key) {
            missing.push(format!("zh-CN missing '{key}' (referenced in {file})"));
        }
        if !has_key(Locale::EnUs, &key) {
            missing.push(format!("en-US missing '{key}' (referenced in {file})"));
        }
    }
    assert!(
        missing.is_empty(),
        "i18n coverage gaps detected:\n{}",
        missing.join("\n")
    );
}
