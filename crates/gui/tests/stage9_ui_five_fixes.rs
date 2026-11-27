//! Stage 9 UI 五问修复 + 补充 A / C 自动化回归测试：
//! - 问题 1：检查目标/范围选择直接位于检测首页（桌面 & 移动），中文人话标签 + 实时生效；
//! - 问题 2：用户自定义分组（独立于内置 google / control 分组），预览/清单/检测首页联动；
//! - 问题 3：默认视角隐藏全部底层机器码与英文等式（除白名单外），「显示技术细节」开关可按需还原；
//! - 问题 4：检测节奏直接位于检测首页，与设置页共享同一 `SchedulePrefs`，< 3000ms 保持非模态警示；
//! - 问题 5：移动端导入页三入口位于输入区正上方且水平单行排布（360×640 / 390×844 / 412×732 × 双语），
//!   输入框全宽 > 150px、桌面端原生文件选择器可测；
//! - 补充 A / C：目标分组独立限高滚动、移动端不盖住主按钮与底栏、单屏唯一主 CTA。

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::sync::Arc;

use egui::{Pos2, Rect};
use parallaxe_core::contracts::{USER_RULE_LIABILITY_NOTICE, USER_TARGET_LIABILITY_NOTICE};
use parallaxe_core::i18n::Locale;
use parallaxe_core::import_bundle::parse_import_text_with_default_group;
use parallaxe_core::targets::Registry;
use parallaxe_gui::app::ParallaxeGuiApp;
use parallaxe_gui::commands::{build_offline_scenario_events, OfflineScenario};
use parallaxe_gui::folder_picker::ImportFileFilter;
use parallaxe_gui::layout::ActiveTab;
use parallaxe_gui::theme::{DensityMode, ThemeMode};
use parallaxe_gui::views::import_panel::ImportEntryTab;
use parallaxe_gui::views::settings_page::SettingsPage;

#[derive(Debug, Clone)]
struct TextBox {
    text: String,
    rect: Rect,
    clip: Rect,
}

fn render_boxes(
    app: &mut ParallaxeGuiApp,
    ctx: &egui::Context,
    w: f32,
    h: f32,
    events: Vec<egui::Event>,
) -> Vec<TextBox> {
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            events,
            ..Default::default()
        },
        |c| app.render_ui(c),
    );
    out.shapes
        .into_iter()
        .filter_map(|clipped| {
            if let egui::Shape::Text(t) = clipped.shape {
                let rect = t.visual_bounding_rect();
                let center = rect.center();
                clipped.clip_rect.contains(center).then(|| TextBox {
                    text: t.galley.text().to_string(),
                    rect,
                    clip: clipped.clip_rect,
                })
            } else {
                None
            }
        })
        .collect()
}

fn click_at(pos: Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn find_box<'a>(boxes: &'a [TextBox], needle: &str) -> &'a TextBox {
    boxes.iter().find(|b| b.text == needle).unwrap_or_else(|| {
        let dump: Vec<String> = boxes
            .iter()
            .map(|b| format!("'{}'@{:.0}..{:.0}", b.text, b.rect.top(), b.rect.bottom()))
            .collect();
        panic!("未找到可见文案: {needle}\n可见列表: {}", dump.join(" | "))
    })
}

fn extract_forbidden_machine_codes(text: &str) -> Vec<String> {
    // 允许白名单（契约规定保留的观测边界、导出安全码与第三方文件名、正常英文缩写/专有名词）
    const ALLOWLIST: &[&str] = &[
        "ANDROID_SIMULATED_USER",
        "EXPORT_WARNING_NO_SAFE_BLOCK",
        "THIRD_PARTY_PLUGINS",
        "THIRD_PARTY_NOTICES",
        "HTTPS",
        "GITHUB",
        "README",
        "DOMAIN",
        "CLASH",
        "PROXY",
        "GOOGLE",
        "BOOKS",
        "DRIVE",
        "GMAIL",
        "YOUTUBE",
        "GEMINI",
        "START",
        "CHECK",
        "QUICK",
        "SCOPE",
        "BATCH",
        "GROUP",
        "ABOUT",
        "LIGHT",
        "DARK",
        "IMPORT",
        "MANUAL",
        "VALID",
        "READY",
        "CLEAR",
        "CLOSE",
        "RESET",
        "GUARD",
        "LOCAL",
        "FIXED",
        "ERROR",
        "EMPTY",
        "TITLE",
    ];
    let mut hits = Vec::new();
    let mut cur = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_uppercase() || ch == '_' || (ch.is_ascii_digit() && !cur.is_empty()) {
            cur.push(ch);
        } else {
            if cur.len() >= 5 {
                let has_alpha = cur.chars().any(|c| c.is_ascii_uppercase());
                let is_all_upper_code = has_alpha
                    && (cur.contains('_')
                        || matches!(
                            cur.as_str(),
                            "MOBILE"
                                | "CAMPUS"
                                | "UNKNOWN"
                                | "BLOCKED"
                                | "RESTRICTED"
                                | "CONFLICT"
                                | "FIXTURE"
                                | "ONLINE"
                                | "COMPLETE"
                                | "CANCELLED"
                                | "FAILED"
                                | "PARTIAL"
                                | "VERIFIED"
                                | "UNSUPPORTED"
                                | "MATCH"
                                | "MISMATCH"
                                | "STALE"
                                | "CURRENT"
                                | "MISSING"
                                | "REJECT"
                                | "INTENT"
                                | "REVOKED"
                                | "TRANSPORT"
                                | "INFERENCE"
                                | "MEDIUM"
                        ));
                if is_all_upper_code && !ALLOWLIST.contains(&cur.as_str()) {
                    hits.push(cur.clone());
                }
            }
            cur.clear();
        }
    }
    hits
}

#[test]
fn problem1_check_page_scope_selector_visible_and_syncs_desktop_and_mobile() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Overview);

    // 1. 桌面端首页直接可见检查范围（简单检查 / 完整检查）与唯一主 CTA
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    find_box(&boxes, "简单检查（16 项）");
    let full_chip = find_box(&boxes, "完整检查（107 项）");
    let cta_count = boxes.iter().filter(|b| b.text == "开始检测").count();
    assert_eq!(cta_count, 1, "检测首页只能有唯一一个「开始检测」主按钮");

    // 点击「完整检查（107 项）」立即生效
    let _ = render_boxes(
        &mut app,
        &ctx,
        1280.0,
        900.0,
        click_at(full_chip.rect.center()),
    );
    assert_eq!(app.selected_profile(), "google-all");

    // 2. 英文界面同样显示可读名称
    app.set_locale(Locale::EnUs);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let en_boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    find_box(&en_boxes, "Quick Check (16)");
    find_box(&en_boxes, "Full Check (107)");
    app.set_locale(Locale::ZhCn);

    // 3. 移动端（390×844）检查范围位于「开始检测」正上方，且同样只有唯一主 CTA
    let _ = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let mob = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let mob_scope = find_box(&mob, "简单检查（16 项）");
    let mob_cta = find_box(&mob, "开始检测");
    assert!(
        mob_scope.rect.bottom() < mob_cta.rect.top(),
        "移动端检查范围选择器应位于「开始检测」主按钮上方"
    );
    assert_eq!(
        mob.iter().filter(|b| b.text == "开始检测").count(),
        1,
        "移动端检测首页同样只应有一个「开始检测」主按钮"
    );
}

#[test]
fn problem2_user_custom_groups_create_preview_inventory_and_check_page() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());

    // 1. 新建自定义分组成功，拒绝与内置 google / control 命名空间冲突
    assert!(app
        .import_view_mut()
        .create_and_select_group("google")
        .is_err());
    assert!(app
        .import_view_mut()
        .create_and_select_group("control.cdn")
        .is_err());
    let created = app
        .import_view_mut()
        .create_and_select_group("办公专线")
        .expect("create custom group");
    assert_eq!(created, "办公专线");

    // 2. 通过文本导入两个自定义分组并原子导入目录
    let sample = "[办公专线]\n\
https://office-a.example.com/status | rule.http | office-a | 办公内网探测\n\
[媒体自测]\n\
https://media-b.example.com/trace | rule.trace,rule.http | media-b | 媒体出口自测\n";
    let bundle = parse_import_text_with_default_group(sample, &[], false, Some("我的导入"))
        .expect("parse grouped bundle");
    app.registry_mut()
        .import_bundle(&bundle, true, false)
        .expect("import grouped targets");
    assert_eq!(
        app.registry().user_groups(),
        vec!["办公专线".to_string(), "媒体自测".to_string()]
    );

    // 3. 导入页清单按自定义分组展示统计与分组条目
    app.navigate_to(ActiveTab::Import);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 1200.0, vec![]);
    let imp = render_boxes(&mut app, &ctx, 1280.0, 1200.0, vec![]);
    assert!(imp.iter().any(|b| b.text.contains("办公专线 · 1")));
    assert!(imp.iter().any(|b| b.text.contains("媒体自测 · 1")));

    // 4. 检测首页可滚动分组列表置顶展示用户自定义分组，并保持与内置 Google / 对照分组独立
    app.navigate_to(ActiveTab::Overview);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    let ov = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    assert!(ov.iter().any(|b| b.text == "办公专线 · 1"));
    assert!(ov.iter().any(|b| b.text == "媒体自测 · 1"));
    assert!(ov.iter().any(|b| b.text == "自定义分组"));
    assert!(ov.iter().any(|b| b.text == "Google 服务 · 10"));
}

#[tokio::test]
async fn problem3_default_view_hides_machine_codes_and_toggle_reveals_them() {
    let dir = std::env::temp_dir().join(format!(
        "parallaxe_stage9_problem3_audit_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("sysblock-pending.json"),
        serde_json::json!({
            "schema": "parallaxe.sysblock.audit/v2",
            "status": "INTENT",
            "rule_tag": "pending",
            "code": "SYSBLOCK_RECOVERY_REQUIRED",
            "detail": "上次系统写入状态未知，请先核对规则。",
            "recovery": "parallaxe guard unblock --rule-tag pending"
        })
        .to_string(),
    )
    .unwrap();

    let ctx = egui::Context::default();
    let reg = Registry::embedded().unwrap();
    let events = build_offline_scenario_events(
        &reg,
        OfflineScenario::BlockedAndConflictMix,
        "run-stage9-p3",
    )
    .await
    .unwrap();
    let mut app = ParallaxeGuiApp::new(reg, ctx.clone());
    for ev in &events {
        app.apply_event(ev);
    }
    app.guard_view_mut().audit_dir_override = Some(dir.clone());
    app.guard_view_mut().refresh_pending_audits();
    assert!(
        !app.show_technical_details(),
        "「显示技术细节」默认必须为关闭"
    );

    // 扫描全部主视图 + 导入三入口 + 关于弹窗（中英双语），断言默认模式零机器码泄露
    let mut leaked = Vec::new();
    let tabs = [
        ActiveTab::Overview,
        ActiveTab::Services,
        ActiveTab::Coverage,
        ActiveTab::Guard,
        ActiveTab::Timeline,
        ActiveTab::Import,
        ActiveTab::Settings,
    ];
    for locale in Locale::ALL {
        app.set_locale(locale);
        for tab in tabs {
            app.navigate_to(tab);
            if tab == ActiveTab::Import {
                for entry in ImportEntryTab::ALL {
                    app.import_view_mut().entry_tab = entry;
                    let _ = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
                    let boxes = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
                    for b in &boxes {
                        for code in extract_forbidden_machine_codes(&b.text) {
                            leaked.push(format!(
                                "{locale:?}/{tab:?}/{entry:?}: {code} in '{}'",
                                b.text
                            ));
                        }
                    }
                    // 责任声明在英文界面下仍必须保持核定中文原文一字不改
                    if entry == ImportEntryTab::Manual {
                        assert!(
                            boxes
                                .iter()
                                .any(|b| b.text.contains(USER_TARGET_LIABILITY_NOTICE)),
                            "责任声明正文必须在 {locale:?} 下原样保留"
                        );
                    }
                }
            } else {
                let _ = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
                let boxes = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
                for b in &boxes {
                    for code in extract_forbidden_machine_codes(&b.text) {
                        leaked.push(format!("{locale:?}/{tab:?}: {code} in '{}'", b.text));
                    }
                }
            }
        }

        // 关于弹窗默认模式也不能直接裸露 account_mode=ANONYMOUS_ONLY / session_mode=STATELESS
        app.navigate_to(ActiveTab::Overview);
        app.set_about_modal_open(true);
        let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
        let about_boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
        for b in &about_boxes {
            for code in extract_forbidden_machine_codes(&b.text) {
                leaked.push(format!("{locale:?}/AboutModal: {code} in '{}'", b.text));
            }
        }
        app.set_about_modal_open(false);
    }

    assert!(
        leaked.is_empty(),
        "默认模式（未开启技术细节）检测到机器码泄露：\n{}",
        leaked.join("\n")
    );

    // 验证规则责任声明原文也在 ZIP 示例包导入页英文模式下完整保留
    app.set_locale(Locale::EnUs);
    app.navigate_to(ActiveTab::Import);
    app.import_view_mut().entry_tab = ImportEntryTab::ZipBundle;
    app.import_view_mut().load_sample_zip_bundle();
    let _ = render_boxes(&mut app, &ctx, 1280.0, 1200.0, vec![]);
    let zip_en = render_boxes(&mut app, &ctx, 1280.0, 1200.0, vec![]);
    assert!(zip_en
        .iter()
        .any(|b| b.text.contains(USER_RULE_LIABILITY_NOTICE)));

    // 开启「显示技术细节」总开关后，技术字段/机器码清晰可见，便于排障
    app.set_locale(Locale::ZhCn);
    app.set_show_technical_details(true);
    app.set_about_modal_open(true);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let about_tech = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    assert!(about_tech.iter().any(|b| b.text.contains("ANONYMOUS_ONLY")));
    app.set_about_modal_open(false);

    app.navigate_to(ActiveTab::Guard);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
    let guard_tech = render_boxes(&mut app, &ctx, 1280.0, 1100.0, vec![]);
    assert!(guard_tech
        .iter()
        .any(|b| b.text.contains("SYSBLOCK_RECOVERY_REQUIRED")));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn problem4_check_pace_on_check_page_syncs_with_settings_and_warns_low_interval() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Overview);

    // 默认间隔符合 3500ms（3–5 秒规范）
    assert_eq!(app.schedule_prefs().effective().min_interval_ms, 3_500);
    assert!(!app.schedule_prefs().is_low_interval());

    // 首页展开「检测节奏」面板，显示安全间隔说明提示
    app.set_pace_panel_open(true);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    let ov_boxes = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    assert!(ov_boxes.iter().any(|b| b
        .text
        .contains("间隔越大越安全，默认 3500ms 已符合 3–5 秒规范")));

    // 调低间隔至 2000ms (< 3000ms) 与每批 3 项：首页立即出现非模态风控警示，且设置页同步一致
    app.schedule_prefs_mut().set_interval_ms(2_000);
    app.schedule_prefs_mut().set_batch_size(3);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    let low_ov = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    assert!(low_ov.iter().any(|b| b.text.contains("可能触发风控")));
    assert!(low_ov
        .iter()
        .any(|b| b.text.contains("间隔 2000 ms · 每批 3 项")));

    // 收起节奏面板后仍不静默跳过低间隔警示
    app.set_pace_panel_open(false);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    let collapsed_low = render_boxes(&mut app, &ctx, 1280.0, 960.0, vec![]);
    assert!(collapsed_low
        .iter()
        .any(|b| b.text.contains("可能触发风控")));

    // 移动端检测节奏条位于「开始检测」主按钮上方
    let _ = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let mob = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let pace_lbl = find_box(&mob, "检测节奏");
    let mob_cta = find_box(&mob, "开始检测");
    assert!(pace_lbl.rect.bottom() < mob_cta.rect.top());

    // 切到设置页「高级网络」二级页：展示与首页完全同步的参数与警示
    app.navigate_to(ActiveTab::Settings);
    app.open_settings_page(SettingsPage::Advanced);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 1400.0, vec![]);
    let set_boxes = render_boxes(&mut app, &ctx, 1280.0, 1400.0, vec![]);
    assert!(set_boxes.iter().any(|b| b.text.contains("可能触发风控")));
    assert!(set_boxes.iter().any(|b| b.text.contains("间隔 2000 ms")));
}

#[test]
fn problem5_mobile_import_tabs_horizontal_above_inputs_across_viewports_and_picker() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Import);

    let viewports = [
        ("360x640", 360.0_f32, 640.0_f32),
        ("390x844", 390.0_f32, 844.0_f32),
        ("412x732", 412.0_f32, 732.0_f32),
    ];

    for locale in Locale::ALL {
        app.set_locale(locale);
        let tab_labels = match locale {
            Locale::ZhCn => ["手动配置", "文本导入", "ZIP 导入包"],
            Locale::EnUs => ["Manual", "Text Import", "ZIP Bundle"],
        };
        for (vp_name, w, h) in viewports {
            for entry in ImportEntryTab::ALL {
                app.import_view_mut().entry_tab = entry;
                let _ = render_boxes(&mut app, &ctx, w, h, vec![]);
                let boxes = render_boxes(&mut app, &ctx, w, h, vec![]);

                let t0 = find_box(&boxes, tab_labels[0]);
                let t1 = find_box(&boxes, tab_labels[1]);
                let t2 = find_box(&boxes, tab_labels[2]);

                // 1. 三个入口 Tab 严格水平一行排布（y 对齐、从左到右递增、单行不逐字竖排）
                assert!(
                    (t0.rect.center().y - t1.rect.center().y).abs() < 8.0
                        && (t1.rect.center().y - t2.rect.center().y).abs() < 8.0,
                    "{locale:?} {vp_name} {entry:?}: 三入口 Tab 未在同一水平行: y=({:.1}, {:.1}, {:.1})",
                    t0.rect.center().y,
                    t1.rect.center().y,
                    t2.rect.center().y
                );
                assert!(
                    t0.rect.center().x < t1.rect.center().x
                        && t1.rect.center().x < t2.rect.center().x,
                    "{locale:?} {vp_name} {entry:?}: 三入口 Tab 顺序异常"
                );
                for tb in [t0, t1, t2] {
                    assert!(
                        tb.rect.height() < 26.0,
                        "{locale:?} {vp_name} {entry:?}: Tab '{}' 发生异常多行竖排 (h={:.1})",
                        tb.text,
                        tb.rect.height()
                    );
                }
                assert!(
                    (t1.rect.center().x - t0.rect.center().x) >= 96.0
                        && (t2.rect.center().x - t1.rect.center().x) >= 96.0,
                    "{locale:?} {vp_name} {entry:?}: Tab 槽宽被挤压过窄: dx=({:.1}, {:.1})",
                    t1.rect.center().x - t0.rect.center().x,
                    t2.rect.center().x - t1.rect.center().x
                );

                // 2. 输入区核心控件位于三入口 Tab 正下方且不被挤出视口
                let expected_input_anchor = match entry {
                    ImportEntryTab::Manual => "目标网址（默认 https://）",
                    ImportEntryTab::TextFile => match locale {
                        Locale::ZhCn => "选择本地文件...",
                        Locale::EnUs => "Choose Local File...",
                    },
                    ImportEntryTab::ZipBundle => match locale {
                        Locale::ZhCn => "选择本地 ZIP 包...",
                        Locale::EnUs => "Choose Local ZIP...",
                    },
                };
                let anchor = find_box(&boxes, expected_input_anchor);
                assert!(
                    anchor.rect.top() > t0.rect.bottom(),
                    "{locale:?} {vp_name} {entry:?}: 输入区未位于三入口 Tab 下方"
                );

                // 3. 零水平溢出
                for b in &boxes {
                    assert!(
                        b.rect.right().min(b.clip.right()) <= w + 2.0 && b.rect.left() >= -2.0,
                        "{locale:?} {vp_name} {entry:?}: 文案 '{}' 溢出视口 [{:.1}..{:.1}] (w={w})",
                        b.text,
                        b.rect.left(),
                        b.rect.right()
                    );
                }
            }
        }
    }

    // 4. 验证桌面端本地文件选择器回调（取消不改状态；选中文件立即解析并支持自定义分组）
    app.set_locale(Locale::ZhCn);
    app.import_view_mut().entry_tab = ImportEntryTab::TextFile;
    let tmp_file = std::env::temp_dir().join(format!(
        "parallaxe_stage9_pick_test_{}.txt",
        std::process::id()
    ));
    std::fs::write(
        &tmp_file,
        "[文件分组]\nhttps://picked.example.com/status | rule.http | picked-a | 本地选取测试\n",
    )
    .unwrap();

    // 取消选择路径
    app.import_view_mut().file_picker_override = Some(Arc::new(|_, _| Ok(None)));
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let pick_btn = find_box(&boxes, "选择本地文件...");
    let _ = render_boxes(
        &mut app,
        &ctx,
        1280.0,
        900.0,
        click_at(pick_btn.rect.center()),
    );
    assert!(matches!(
        app.import_view_mut().feedback,
        Some(Ok(ref msg)) if msg.contains("已取消")
    ));

    // 选中本地文件并解析分组
    let tmp_clone = tmp_file.clone();
    app.import_view_mut().file_picker_override = Some(Arc::new(move |_, filter| {
        assert_eq!(filter, ImportFileFilter::TextBundle);
        Ok(Some(tmp_clone.clone()))
    }));
    let boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let pick_btn = find_box(&boxes, "选择本地文件...");
    let _ = render_boxes(
        &mut app,
        &ctx,
        1280.0,
        900.0,
        click_at(pick_btn.rect.center()),
    );
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let after = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    assert!(
        after.iter().any(|b| b.text == "文件分组 · 1 项"),
        "选中本地文本文件后预览区应按文件内自定义分组渲染"
    );
    let _ = std::fs::remove_file(&tmp_file);
}

#[test]
fn supplement_a_scrollable_target_groups_bounded_and_above_mobile_cta_and_nav() {
    let ctx = egui::Context::default();
    let mut app = ParallaxeGuiApp::new(Registry::embedded().unwrap(), ctx.clone());
    app.navigate_to(ActiveTab::Overview);
    app.set_view_modes(DensityMode::Normal, ThemeMode::Dark);

    // 1. 桌面端：默认展开当前主组（Google 服务 · 10）并在限高滚动区内裁剪长列表，折叠后即可见独立对照服务 · 6
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let common_boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let google_hdr = find_box(&common_boxes, "Google 服务 · 10");
    find_box(&common_boxes, "Google 搜索");
    let _ = render_boxes(
        &mut app,
        &ctx,
        1280.0,
        900.0,
        click_at(google_hdr.rect.center()),
    );
    let collapsed_boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    find_box(&collapsed_boxes, "独立对照服务 · 6");

    app.set_show_all_groups(true);
    let _ = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let all_boxes = render_boxes(&mut app, &ctx, 1280.0, 900.0, vec![]);
    let grp_header = find_box(&all_boxes, "检测目标分组");
    let kpi_header = find_box(&all_boxes, "明确异常");
    // 分组区域（标题 + 搜索框 + 限高滚动容器）高度严格受限，绝不撑满整屏把 KPI 推到不可见深渊
    let groups_section_h = kpi_header.rect.top() - grp_header.rect.top();
    assert!(
        groups_section_h <= 420.0,
        "桌面目标分组区过高: {groups_section_h:.1}px（应 <= 420px）"
    );

    // 2. 移动端（390×844）：可滚动目标分组严格位于「开始检测」主按钮之上、主按钮严格位于底部导航之上
    app.set_show_all_groups(false);
    let _ = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let mob = render_boxes(&mut app, &ctx, 390.0, 844.0, vec![]);
    let mob_grp_title = find_box(&mob, "检测目标分组");
    let mob_cta = find_box(&mob, "开始检测");
    let mob_nav_services = find_box(&mob, "服务");
    let mob_groups_h = mob_cta.rect.top() - mob_grp_title.rect.top();
    assert!(
        mob_groups_h <= 844.0 * 0.45,
        "移动端目标分组区过高 ({mob_groups_h:.1}px)，可能挤压底部主按钮"
    );
    assert!(
        mob_cta.rect.bottom() < mob_nav_services.rect.top(),
        "移动端「开始检测」主按钮不得与底部导航重叠"
    );
}
