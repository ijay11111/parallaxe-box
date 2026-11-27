//! 阶段 12：发行包默认规则包（重复网址更换 + 仅本次/永久授权）。
#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::collections::BTreeSet;

use parallaxe_core::import_bundle::{
    bundled_default_rules_bundle, parse_import_text, ImportBundle, ImportSource,
};
use parallaxe_core::targets::{Registry, UserTargetImportInput};

fn temp_state_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "parallaxe_stage12_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ))
}

fn replacement_bundle() -> ImportBundle {
    let mut bundle = bundled_default_rules_bundle().expect("发行包默认规则包必须可解析");
    for entry in &mut bundle.targets {
        entry.replace_existing = true;
    }
    bundle
}

#[test]
fn bundled_rules_are_embedded_and_parse_to_107_replaceable_targets() {
    let bundle = bundled_default_rules_bundle().expect("嵌入规则包解析");
    assert_eq!(bundle.schema, "parallaxe.import/v1");
    assert_eq!(bundle.source, ImportSource::ZipBundle);
    assert_eq!(bundle.targets.len(), 107);
    assert_eq!(bundle.rules.len(), 0);

    let registry = Registry::embedded().unwrap();
    let preview = registry.preview_import_bundle(&bundle);
    assert!(preview.all_valid, "预览不应因重复网址而失效");
    let duplicates: Vec<_> = preview
        .targets
        .iter()
        .filter(|row| row.duplicate_of.is_some())
        .collect();
    assert_eq!(duplicates.len(), 107, "107 条必须全部命中内置重复");
    let protected = preview
        .targets
        .iter()
        .filter(|row| row.duplicate_protected)
        .count();
    assert_eq!(protected, 82, "受保护目标数量应为 82");
    assert!(preview
        .targets
        .iter()
        .all(|row| !row.valid || row.error.is_none()));
}

#[test]
fn duplicate_import_is_rejected_without_explicit_replace() {
    let registry = Registry::embedded().unwrap();
    let bundle = bundled_default_rules_bundle().unwrap();
    let one = bundle.targets[0].clone();
    let mut no_replace = ImportBundle::new(ImportSource::ZipBundle);
    no_replace.targets.push(one.clone());
    let mut reg = registry.clone();
    let err = reg.import_bundle(&no_replace, true, true).unwrap_err();
    assert!(
        err.to_string().contains("已存在"),
        "未确认更换时必须保持硬拒绝：{err}"
    );

    // 手工单条导入同样默认拒绝重复。
    let err = reg
        .import_user_target(
            UserTargetImportInput {
                id: one.id.clone(),
                url: one.url.clone(),
                surface: one.surface.clone(),
                signals: one.signals.clone(),
                allow_insecure_http: false,
                user_group: Some("identity".to_string()),
            },
            true,
        )
        .unwrap_err();
    assert!(err.to_string().contains("已存在"));
}

#[test]
fn batch_replace_imports_107_and_session_authorization_is_one_shot() {
    let mut registry = Registry::embedded().unwrap();
    let report = registry
        .import_bundle(&replacement_bundle(), true, true)
        .expect("批量更换导入");
    assert_eq!(report.imported_targets.len(), 107);
    assert!(report
        .imported_targets
        .iter()
        .all(|meta| meta.replaced_builtin_id.is_some()));
    assert_eq!(
        report
            .imported_targets
            .iter()
            .filter(|meta| meta.replaces_protected)
            .count(),
        82
    );

    let ids: Vec<String> = report
        .imported_targets
        .iter()
        .map(|meta| meta.id.clone())
        .collect();
    let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
    registry.set_distribution_mode(true);

    // 授权前：授权版目标仍零发包（发行模式只放行内置 Google，不静默放行 user.*）。
    let first = registry.get(&ids[0]).unwrap().clone();
    assert!(!registry.admission(&first, false, &now).admitted);
    assert!(!first.raw.allow_online);

    // 仅本次：一次性会话许可直接放行，但不改持久 allow_online。
    let grant = registry
        .authorize_default_bundle_session(&ids, true)
        .expect("会话授权");
    assert_eq!(grant.targets.len(), 107);
    let first = registry.get(&ids[0]).unwrap().clone();
    assert!(!first.raw.allow_online);
    assert!(
        registry
            .check_session_grant_admission(&grant, &first, false, false)
            .admitted
    );
    // 原子消费后失效。
    assert!(registry.take_session_grant().is_some());
    assert!(registry.active_session_grant_snapshot().is_none());
}

#[test]
fn permanent_authorization_persists_restores_and_revokes() {
    let dir = temp_state_dir("perm");
    let _ = std::fs::remove_dir_all(&dir);
    let mut registry = Registry::embedded().unwrap();
    let report = registry
        .import_bundle(&replacement_bundle(), true, true)
        .expect("批量更换导入");
    let ids: Vec<String> = report
        .imported_targets
        .iter()
        .map(|meta| meta.id.clone())
        .collect();
    let now = parallaxe_core::state::format_time(time::OffsetDateTime::now_utc());
    registry.set_distribution_mode(true);

    let auths = registry
        .authorize_default_bundle_permanent(&ids, &now)
        .expect("永久授权");
    assert_eq!(auths.len(), 107);
    let first = registry.get(&ids[0]).unwrap().clone();
    assert!(first.raw.allow_online);
    assert!(registry.admission(&first, false, &now).admitted);

    let path = registry.save_default_bundle_consent(&dir).expect("落盘");
    assert!(path.exists());

    // 重启：全新 Registry 从用户状态目录恢复 107 项并保持放行。
    let mut restored = Registry::embedded().unwrap();
    restored.set_distribution_mode(true);
    let restore = restored
        .restore_default_bundle_state(&dir)
        .expect("恢复授权");
    assert_eq!(restore.restored_targets, 107);
    assert_eq!(restore.authorized_targets, 107);
    assert_eq!(restored.authorized_user_target_ids().len(), 107);
    let first = restored.get(&ids[0]).unwrap().clone();
    assert!(first.raw.allow_online);
    assert!(restored.admission(&first, false, &now).admitted);

    // 撤销：授权文件删除、基线恢复零发包。
    restored.revoke_default_bundle_state(&dir).expect("撤销");
    assert!(!path.exists());
    let first = restored.get(&ids[0]).unwrap().clone();
    assert!(!first.raw.allow_online);
    assert!(!restored.admission(&first, false, &now).admitted);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn authorization_scope_cannot_spread_to_ordinary_imports() {
    let mut registry = Registry::embedded().unwrap();
    let bundle = parse_import_text(
        "普通导入 | https://example.com/ | rule.http | 普通表面 | ordinary",
        &["rule.http".to_string()],
        false,
    )
    .unwrap();
    let imported = registry.import_bundle(&bundle, true, true).unwrap();
    let id = imported.imported_targets[0].id.clone();
    registry.set_distribution_mode(true);
    let err = registry
        .authorize_default_bundle_permanent(std::slice::from_ref(&id), "2026-11-26T00:00:00Z")
        .unwrap_err();
    assert!(err.to_string().contains("更换内置目标"));

    let mut ids = BTreeSet::new();
    ids.insert(id);
    assert_eq!(ids.len(), 1);
}
