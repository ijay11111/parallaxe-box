//! 只读处置清单：帮助用户把选定的处置方案导出为本机备忘 JSON，不是自动生效的 Guard 规则，也不序列化原始证据。
//! 支持为任意服务（包含当前访问正常的服务，如主动屏蔽 Google One）生成处置备忘。

use crate::{
    state::RunView,
    theme::{
        btn_filled, btn_outlined, card_frame, render_pill, subtle_well_frame, Palette, FONT_BODY,
        FONT_META, FONT_TITLE, SPACE_1, SPACE_2, SPACE_3,
    },
    views::service_matrix::service_name,
};
use egui::{RichText, Ui};
use parallaxe_core::contracts::{ResultRecord, State};
use std::{fs::OpenOptions, io::Write, path::Path};

/// 四类常用处置思路：(方案名, 干什么 / 解决什么问题, 注意事项与风险边界)
const OPTIONS: [(&str, &str, &str); 4] = [
    (
        "主动屏蔽该服务网址（即使当前访问正常）",
        "解决什么问题：你想主动停用或在本机屏蔽某个服务（例如 Google Books 受限、Google One 正常，但你仍希望主动屏蔽 Google One 网址），减少该服务的本机访问暴露。",
        "注意：需由你在代理客户端或本机防火墙手动添加该域名的拒绝规则；屏蔽后将无法访问该服务，若与登录/商店共享域名可能产生连带影响；本软件不自动写入规则。",
    ),
    (
        "评估本机隔离规则（适用于已受限/异常服务）",
        "解决什么问题：当某服务已明确受限或被阻断时，评估是否在本机对该独占域名做拦截隔离，避免客户端反复误连异常出口。",
        "注意：须先确认该域名不与登录、Play 商店或公共 CDN 共用；当前版本仅生成备忘清单，绝不自动修改系统或代理配置。",
    ),
    (
        "保持监控，不改规则",
        "解决什么问题：当检测结果证据不足、存在冲突，或你暂不希望改动当前网络规则时，先保留记录以便后续复核。",
        "注意：不会消除已有的访问限制，建议稍后或网络变更后重新检测。",
    ),
    (
        "手动切换线路后复测",
        "解决什么问题：当当前出口地区不符合预期或服务受限时，在你的代理客户端手动切换至另一条已有线路并重新体检。",
        "注意：切换线路可能中断当前连接；仅切换网络出口不能保证远端账号或付款地区改变。",
    ),
];

#[derive(Default)]
pub struct PlanDraft {
    pub target: String,
    pub target_host: String,
    pub run_id: Option<String>,
    pub is_fixture: bool,
    pub observed_state: Option<State>,
    pub observed_country: Option<String>,
    selected: [bool; 4],
    path: String,
    status: Option<Result<String, String>>,
    pub closed: bool,
}

impl PlanDraft {
    pub fn new(run: &RunView, target: &str) -> Self {
        let r = run.result_for(target);
        Self {
            run_id: Some(run.run_id.clone()),
            target: target.to_owned(),
            target_host: String::new(),
            is_fixture: run.is_fixture,
            observed_state: r.map(|x| x.state),
            observed_country: r.and_then(|x| x.service_observed.observed_country.clone()),
            path: "parallaxe-plan.json".into(),
            ..Default::default()
        }
    }

    /// 供任意目录服务（含当前正常服务或尚未检测的服务，如主动屏蔽 Google One）创建处置清单。
    pub fn for_service(
        target_id: &str,
        target_host: String,
        run: Option<&RunView>,
        preselect_proactive_block: bool,
    ) -> Self {
        let r = run.and_then(|rv| rv.result_for(target_id));
        let mut selected = [false; 4];
        if preselect_proactive_block {
            selected[0] = true;
        }
        Self {
            run_id: run.map(|rv| rv.run_id.clone()),
            target: target_id.to_owned(),
            target_host,
            is_fixture: run.is_some_and(|rv| rv.is_fixture),
            observed_state: r.map(|x| x.state),
            observed_country: r.and_then(|x| x.service_observed.observed_country.clone()),
            selected,
            path: "parallaxe-plan.json".into(),
            status: None,
            closed: false,
        }
    }

    fn save(
        &self,
        path: &Path,
        run: Option<&RunView>,
        r: Option<&ResultRecord>,
    ) -> Result<String, String> {
        if let Some(rv) = run {
            if self.run_id.as_deref() == Some(rv.run_id.as_str()) {
                if let Some(rec) = r {
                    if self.target != rec.target {
                        return Err("当前结果或选择已改变，请重新选择方案。".into());
                    }
                }
            }
        }
        if !self.selected.contains(&true) {
            return Err("请至少勾选一项准备考虑的处置方案。".into());
        }
        if path.to_string_lossy().replace('\\', "/").starts_with("//") {
            return Err("请选择本机路径，不支持网络共享或设备路径。".into());
        }
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            return Err("请输入以 .json 结尾的本地文件名。".into());
        }
        let choices: Vec<_> = OPTIONS
            .iter()
            .zip(self.selected)
            .filter(|(_, on)| *on)
            .map(|((name, pre, risk), _)| {
                serde_json::json!({"方案": name, "适用前提": pre, "风险": risk})
            })
            .collect();
        let state_val = r.map(|x| x.state).or(self.observed_state);
        let country_val = r
            .and_then(|x| x.service_observed.observed_country.clone())
            .or_else(|| self.observed_country.clone());
        let doc = serde_json::json!({
            "schema": "parallaxe.plan-preview/v1",
            "operation": "PREVIEW",
            "applied": false,
            "is_fixture": run.map(|rv| rv.is_fixture).unwrap_or(self.is_fixture),
            "run_id": run.map(|rv| rv.run_id.as_str()).or(self.run_id.as_deref()).unwrap_or("manual-selection"),
            "target": self.target,
            "target_host": self.target_host,
            "state": state_val,
            "observed_country": country_val,
            "choices": choices,
            "effect_scope": "LOCAL_TRAFFIC_ONLY",
            "remote_account_impact": "NOT_PROVABLE",
            "shared_egress_guarantee": "UNPROVEN"
        });
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "同名文件已存在，未覆盖。请换一个文件名。".into()
                } else {
                    format!("无法创建文件：{e}")
                }
            })?;
        f.write_all(
            serde_json::to_string_pretty(&doc)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )
        .and_then(|_| f.flush())
        .map_err(|e| format!("写入未完成：{e}"))?;
        Ok(format!("已导出处置清单：{}", path.display()))
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        run: Option<&RunView>,
        r: Option<&ResultRecord>,
    ) {
        card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("{} · 只读处置清单", service_name(&self.target)))
                        .size(FONT_TITLE)
                        .strong()
                        .color(p.text_primary),
                );
                if !self.target_host.is_empty() {
                    render_pill(
                        ui,
                        &self.target_host,
                        p.text_secondary,
                        p.card_alt,
                        p.border,
                    );
                }
                let state_tag = match r.map(|x| x.state).or(self.observed_state) {
                    Some(State::Ok) => ("当前正常（仍可主动屏蔽）", p.ok_fg, p.ok_bg),
                    Some(State::Restricted | State::Blocked) => {
                        ("当前异常 / 受限", p.warning_fg, p.warning_bg)
                    }
                    Some(_) => ("当前待复核", p.info_fg, p.info_bg),
                    None => ("主动自选目标（未测也可制定清单）", p.info_fg, p.info_bg),
                };
                render_pill(ui, state_tag.0, state_tag.1, state_tag.2, state_tag.1);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if btn_outlined(ui, p, "收起清单", true).clicked() {
                        self.closed = true;
                    }
                });
            });
            ui.add_space(SPACE_1);
            ui.label(
                RichText::new(
                    "勾选你准备采用的处置方式并导出为本地 JSON 备忘；即使该服务目前正常，也可以主动选择屏蔽。本操作绝不自动修改系统或代理规则。",
                )
                .size(FONT_BODY)
                .color(p.text_secondary),
            );
            ui.add_space(SPACE_3);

            for (i, (name, pre, risk)) in OPTIONS.iter().enumerate() {
                if i > 0 {
                    ui.add_space(SPACE_2);
                }
                subtle_well_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if ui
                        .checkbox(
                            &mut self.selected[i],
                            RichText::new(*name).size(FONT_BODY).strong(),
                        )
                        .changed()
                    {
                        self.status = None;
                    }
                    ui.add_space(SPACE_1);
                    ui.label(RichText::new(*pre).size(FONT_META).color(p.text_primary));
                    ui.label(RichText::new(*risk).size(FONT_META).color(p.text_secondary));
                });
            }

            ui.add_space(SPACE_3);
            ui.label(
                RichText::new(
                    "边界说明：所有处置仅涉及本机流量；不能证明远端账号归属变化，也不保证共享出口表现改变。",
                )
                .size(FONT_META)
                .color(p.text_secondary),
            );
            ui.collapsing("查看审计边界技术字段", |ui| {
                ui.monospace(
                    "effect_scope=LOCAL_TRAFFIC_ONLY\nremote_account_impact=NOT_PROVABLE\nshared_egress_guarantee=UNPROVEN",
                );
            });
            ui.add_space(SPACE_3);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_2;
                ui.label(
                    RichText::new("保存位置（新建 JSON 文件，不覆盖）：")
                        .size(FONT_META)
                        .color(p.text_secondary),
                );
                if ui
                    .add(egui::TextEdit::singleline(&mut self.path).desired_width(240.0))
                    .on_hover_ui(|ui| {
                        if let Ok(dir) = std::env::current_dir() {
                            ui.label(format!("相对路径的起点：{}", dir.display()));
                        }
                    })
                    .changed()
                {
                    self.status = None;
                }
                if btn_filled(ui, p, "导出处置清单", self.selected.contains(&true)).clicked() {
                    self.status = Some(self.save(Path::new(self.path.trim()), run, r));
                }
            });
            if let Some(outcome) = &self.status {
                ui.add_space(SPACE_2);
                match outcome {
                    Ok(s) => ui.colored_label(p.ok_fg, s),
                    Err(e) => ui.colored_label(p.danger_fg, e),
                };
            }
            ui.add_space(SPACE_1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{build_offline_scenario_events, OfflineScenario},
        state::AppState,
    };
    use parallaxe_core::targets::Registry;

    #[tokio::test]
    async fn checklist_never_embeds_evidence_or_replaces_files() {
        let events = build_offline_scenario_events(
            &Registry::embedded().unwrap(),
            OfflineScenario::BooksAndSharedCdnGuard,
            "run-offline-plan",
        )
        .await
        .unwrap();
        let state = AppState::from_events(&events);
        let run = state.active_run().unwrap();
        let mut r = run.result_for("google.books").unwrap().clone();
        r.path.http_egress_ref = Some("本地敏感占位符".into());
        let mut draft = PlanDraft::new(run, &r.target);
        let path = std::env::temp_dir().join(format!(
            "px-preview-{}-{}.json",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        assert!(draft.save(&path, Some(run), Some(&r)).is_err());
        assert!(!path.exists());
        draft.selected[2] = true;
        assert!(draft
            .save(
                Path::new("//example.invalid/share/preview.json"),
                Some(run),
                Some(&r)
            )
            .is_err());
        assert!(draft
            .save(&path.with_extension("cmd"), Some(run), Some(&r))
            .is_err());
        draft.save(&path, Some(run), Some(&r)).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&original).unwrap();
        assert!(!original.contains("敏感占位符"));
        assert_eq!(doc["applied"], false);
        assert_eq!(doc["is_fixture"], true);
        assert_eq!(doc["state"], "RESTRICTED");
        assert_eq!(doc["choices"].as_array().unwrap().len(), 1);
        draft.selected[3] = true;
        assert!(draft.save(&path, Some(run), Some(&r)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn proactive_block_checklist_works_for_healthy_or_untested_google_one() {
        let draft = PlanDraft::for_service("google.one", "one.google.com".into(), None, true);
        let path = std::env::temp_dir().join(format!(
            "px-proactive-one-{}-{}.json",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        draft.save(&path, None, None).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["target"], "google.one");
        assert_eq!(doc["target_host"], "one.google.com");
        assert_eq!(doc["applied"], false);
        assert!(doc["choices"][0]["方案"]
            .as_str()
            .unwrap()
            .contains("主动屏蔽该服务网址"));
        std::fs::remove_file(path).unwrap();
    }
}
