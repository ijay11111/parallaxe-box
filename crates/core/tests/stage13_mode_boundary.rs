//! stage13：源码零发包 / 发行包模式边界与 AI F3 真值表的核心准入断言。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parallaxe_core::distribution::DISTRIBUTION_RELEASE_BUILD;
use parallaxe_core::import_bundle::parse_import_text;
use parallaxe_core::targets::Registry;

fn now() -> String {
    parallaxe_core::state::format_time(time::OffsetDateTime::now_utc())
}

fn builtin(registry: &Registry) -> parallaxe_core::targets::Target {
    registry.get("google.gemini").unwrap().clone()
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn source_build_defaults_to_zero_packet() {
    const _: () = assert!(!DISTRIBUTION_RELEASE_BUILD);
    let registry = Registry::embedded().unwrap();
    assert!(!registry.distribution_mode());
    assert!(
        !registry
            .admission(&builtin(&registry), false, &now())
            .admitted
    );
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn source_switch_opens_builtin_gate_and_revoke_restores_it() {
    let mut registry = Registry::embedded().unwrap();
    let target = builtin(&registry);
    assert!(!target.raw.allow_online, "不得改写内置资产基线");

    registry.set_distribution_mode(true);
    assert!(registry.distribution_mode());
    assert!(registry.admission(&target, false, &now()).admitted);
    assert!(
        !target.raw.allow_online,
        "运行时放行不得改写内置 allow_online"
    );

    registry.set_distribution_mode(false);
    assert!(!registry.distribution_mode());
    assert!(!registry.admission(&target, false, &now()).admitted);
}

#[cfg(feature = "distribution-release")]
#[test]
fn release_build_is_immutable_and_opens_builtin_gate() {
    const _: () = assert!(DISTRIBUTION_RELEASE_BUILD);
    let mut registry = Registry::embedded().unwrap();
    let target = builtin(&registry);
    assert!(registry.distribution_mode());
    assert!(registry.admission(&target, false, &now()).admitted);

    registry.set_distribution_mode(false);
    assert!(registry.distribution_mode(), "发行构建不能被运行时关闭");
    assert!(registry.admission(&target, false, &now()).admitted);
}

#[cfg(not(feature = "distribution-release"))]
#[test]
fn zero_packet_source_rejects_builtin_f3_grant() {
    let registry = Registry::embedded().unwrap();
    let err = registry
        .authorize_session_once(&["google.gemini".to_string()], true)
        .unwrap_err();
    assert!(err.to_string().contains("严禁对内置目标"));
}

#[test]
fn distribution_mode_allows_only_exact_builtin_f3_scope() {
    let mut registry = Registry::embedded().unwrap();
    registry.set_distribution_mode(true);
    let selected = builtin(&registry);
    let other = registry.get("google.books").unwrap().clone();

    let grant = registry
        .authorize_session_once(&["google.gemini".to_string()], true)
        .expect("发行包模式下用户逐次 F3 确认内置目标");
    assert!(
        registry
            .check_session_grant_admission(&grant, &selected, false, false)
            .admitted
    );
    assert!(
        !registry
            .check_session_grant_admission(&grant, &other, false, false)
            .admitted,
        "F3 许可不得扩散到未选择目标"
    );
    assert!(registry.take_session_grant().is_some());
    assert!(registry.active_session_grant_snapshot().is_none());
}

#[test]
fn distribution_mode_does_not_silently_authorize_ordinary_imports() {
    let mut registry = Registry::embedded().unwrap();
    registry.set_distribution_mode(true);
    let bundle = parse_import_text(
        "普通导入 | https://example.com/ | rule.http | 普通表面 | ordinary",
        &["rule.http".to_string()],
        false,
    )
    .unwrap();
    let imported = registry.import_bundle(&bundle, true, true).unwrap();
    let id = imported.imported_targets[0].id.clone();
    let target = registry.get(&id).unwrap().clone();
    assert!(!registry.admission(&target, false, &now()).admitted);

    let grant = registry
        .authorize_session_once(std::slice::from_ref(&id), true)
        .expect("普通导入仍可按原 F3 路径授权");
    assert!(
        registry
            .check_session_grant_admission(&grant, &target, false, false)
            .admitted
    );
}

#[test]
fn distribution_mode_excludes_controls_and_implicit_default() {
    let mut registry = Registry::embedded().unwrap();
    registry.set_distribution_mode(true);
    let control = registry
        .entries()
        .iter()
        .find(|target| target.id().starts_with("control."))
        .expect("目录应包含 control 目标");
    assert!(!registry.admission(control, false, &now()).admitted);
    let err = registry
        .authorize_session_once(&[control.id().to_string()], true)
        .unwrap_err();
    assert!(err.to_string().contains("不是可授权目标"));

    let google = builtin(&registry);
    assert!(!registry.admission(&google, true, &now()).admitted);
}

#[test]
fn default_bundle_requires_distribution_mode_before_authorization() {
    let mut registry = Registry::embedded().unwrap();
    let mut bundle = parallaxe_core::import_bundle::bundled_default_rules_bundle().unwrap();
    for entry in &mut bundle.targets {
        entry.replace_existing = true;
    }
    let imported = registry.import_bundle(&bundle, true, true).unwrap();
    let ids: Vec<String> = imported
        .imported_targets
        .iter()
        .map(|meta| meta.id.clone())
        .collect();
    if !DISTRIBUTION_RELEASE_BUILD {
        let err = registry
            .authorize_default_bundle_session(&ids, true)
            .unwrap_err();
        assert!(err.to_string().contains("发行包模式"));
        registry.set_distribution_mode(true);
    }
    assert!(registry
        .authorize_default_bundle_session(&ids, true)
        .is_ok());
}

struct MustNotFetch(Arc<AtomicUsize>);

impl parallaxe_core::probe::Transport for MustNotFetch {
    fn fetch<'a>(
        &'a self,
        _: parallaxe_core::probe::FetchRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = parallaxe_core::probe::FetchOutcome> + Send + 'a>,
    > {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { panic!("预算门禁拒绝前不得调用传输") })
    }
}

#[tokio::test]
async fn distribution_full_scope_still_requires_explicit_budget_confirmation() {
    let mut registry = Registry::embedded().unwrap();
    registry.set_distribution_mode(true);
    let calls = Arc::new(AtomicUsize::new(0));
    let transport: Arc<dyn parallaxe_core::probe::Transport> =
        Arc::new(MustNotFetch(Arc::clone(&calls)));
    let options = parallaxe_core::runtime::CheckOptions {
        profile: Some("google-all".to_string()),
        confirm_budget: false,
        ..Default::default()
    };
    let error =
        parallaxe_core::runtime::run_check(&registry, &options, Some(transport), false, None)
            .await
            .unwrap_err();
    assert!(error.to_string().contains("30"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn google_all_scope_keeps_107_builtin_targets_and_mode_gate() {
    let mut registry = Registry::embedded().unwrap();
    let selection = registry
        .select(Some("google-all"), &[], &[], false)
        .unwrap();
    assert_eq!(selection.ids.len(), 107);

    let source_expected = !DISTRIBUTION_RELEASE_BUILD;
    if source_expected {
        assert!(!registry.distribution_mode());
    }
    for id in &selection.ids {
        let target = registry.get(id).unwrap();
        assert_eq!(
            registry.admission(target, false, &now()).admitted,
            !source_expected,
            "目标 {id} 的模式准入不符合构建边界"
        );
    }

    if source_expected {
        registry.set_distribution_mode(true);
        for id in &selection.ids {
            let target = registry.get(id).unwrap();
            assert!(registry.admission(target, false, &now()).admitted);
        }
    }
}
