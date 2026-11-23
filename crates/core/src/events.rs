//! `events`：单写者、单调序号、不可变事件序列。

use serde_json::json;
use time::OffsetDateTime;

use crate::contracts::{reason, Coverage, Event, EventName, Evidence, GuardPlan, Path, Report};

/// 一次运行的事件记录器：提交后不可改序号。
#[derive(Debug)]
pub struct EventLog {
    run_id: String,
    is_fixture: bool,
    seq: u64,
    ts: String,
    events: Vec<Event>,
    sink: Option<tokio::sync::mpsc::UnboundedSender<Event>>,
}

impl EventLog {
    /// 新建记录器。
    pub fn new(run_id: impl Into<String>, is_fixture: bool, now: OffsetDateTime) -> Self {
        Self::with_sink(run_id, is_fixture, now, None)
    }

    /// 新建记录器，并在每个事件提交时向外部通道推送一份副本。
    ///
    /// 通道只承载已经提交的事件；调用方断开接收端不会影响运行（事件仍保留在本记录器）。
    pub fn with_sink(
        run_id: impl Into<String>,
        is_fixture: bool,
        now: OffsetDateTime,
        sink: Option<tokio::sync::mpsc::UnboundedSender<Event>>,
    ) -> Self {
        EventLog {
            run_id: run_id.into(),
            is_fixture,
            seq: 0,
            ts: crate::state::format_time(now),
            events: Vec::new(),
            sink,
        }
    }

    fn push(&mut self, name: EventName, payload: serde_json::Value) {
        self.seq += 1;
        let event = Event::new(
            self.run_id.clone(),
            self.seq,
            self.ts.clone(),
            self.is_fixture,
            name,
            payload,
        );
        if let Some(sink) = &self.sink {
            // 接收端关闭视为“消费者已离开”，不影响本运行的事件序列。
            let _ = sink.send(event.clone());
        }
        self.events.push(event);
    }

    /// RunStarted：运行首事件。
    pub fn run_started(
        &mut self,
        mode: &str,
        profile: Option<&str>,
        catalog_revision: &str,
        selected_ids: &[String],
    ) {
        self.push(
            EventName::RunStarted,
            json!({
                "mode": mode,
                "profile": profile,
                "catalog_revision": catalog_revision,
                "selected_ids": selected_ids,
            }),
        );
    }

    /// PreflightReady：无准入也必须报预检。
    pub fn preflight_ready(
        &mut self,
        admitted_ids: &[String],
        skipped: &[(String, String)],
        budget: serde_json::Value,
        path: &Path,
    ) {
        let skipped: Vec<serde_json::Value> = skipped
            .iter()
            .map(|(target, reason)| json!({"target": target, "reason": reason}))
            .collect();
        self.push(
            EventName::PreflightReady,
            json!({
                "admitted_ids": admitted_ids,
                "skipped": skipped,
                "budget": budget,
                "path": path,
            }),
        );
    }

    /// TargetStarted。
    pub fn target_started(&mut self, target: &str, path_id: &str, logical_attempt: u64) {
        self.push(
            EventName::TargetStarted,
            json!({"target": target, "path_id": path_id, "logical_attempt": logical_attempt}),
        );
    }

    /// EvidenceObserved。
    pub fn evidence_observed(&mut self, target: &str, evidence: &Evidence) {
        self.push(
            EventName::EvidenceObserved,
            json!({"target": target, "evidence": evidence}),
        );
    }

    /// TargetFinished。
    pub fn target_finished(&mut self, result: &crate::contracts::ResultRecord) {
        self.push(EventName::TargetFinished, json!({"result": result}));
    }

    /// CoverageUpdated：完整快照，不是增量。
    pub fn coverage_updated(&mut self, coverage: &Coverage) {
        self.push(EventName::CoverageUpdated, json!({"coverage": coverage}));
    }

    /// GuardPlanReady：只因显式预览请求产生。
    pub fn guard_plan_ready(&mut self, plan: &GuardPlan) {
        self.push(EventName::GuardPlanReady, json!({"plan": plan}));
    }

    /// Error：作用域内错误，不终止其他目标。
    pub fn error(
        &mut self,
        scope: &str,
        code: &str,
        message_safe: &str,
        recoverable: bool,
        target: Option<&str>,
    ) {
        let mut payload = json!({
            "scope": scope,
            "code": code,
            "message_safe": message_safe,
            "recoverable": recoverable,
        });
        if let Some(target) = target {
            payload["target"] = json!(target);
        }
        self.push(EventName::Error, payload);
    }

    /// RunFinished：含报告终态。
    pub fn run_finished(&mut self, report: &Report) {
        self.push(EventName::RunFinished, json!({"report": report}));
    }

    /// RunCancelled：与 RunFinished 互斥。
    pub fn run_cancelled(&mut self, report: &Report) {
        self.push(EventName::RunCancelled, json!({"report": report}));
    }

    /// 已完成事件序列。
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// 消费为事件列表。
    pub fn into_events(self) -> Vec<Event> {
        self.events
    }
}

/// 取消路径使用的标准原因文本。
pub fn cancelled_reason() -> &'static str {
    reason::CANCELLED
}
