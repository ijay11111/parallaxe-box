//! 任务 A 的 GUI 侧授权桥支撑（T-10-00）：
//! 把「AI 一次性检测请求 → 用户 F3 一键确认 → 真实发包 → 脱敏报告回传」串成闭环。
//!
//! 严格边界：
//! - 只在当前用户临时目录的固定子目录读写**短生命周期 JSON 文件**，不监听任何端口；
//! - 自主检测开关只存在于 `AiDebugState` 会话内存，本模块只把开关状态**只读投影**给
//!   `status.json`，绝不写入任何持久化设置；
//! - 请求只接受目录 ID（不接受 URL），范围与 QoS 强制下限由核心校验；
//! - 回传报告一律脱敏（`set_show_ip(false)`），不写任意磁盘路径。

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use parallaxe_core::ai_bridge::{
    BridgeControl, BridgeDecision, BridgePaths, BridgeRequest, BridgeResult, BridgeStatus,
    ControlAction,
};
use parallaxe_core::contracts::Report;

/// 状态投影刷新间隔（宿主轮询节流）。
const STATUS_REFRESH: Duration = Duration::from_secs(1);
/// 请求发现间隔。
const REQUEST_POLL: Duration = Duration::from_millis(500);

/// 宿主侧桥：GUI 每次渲染节流轮询，不阻塞 UI 线程。
pub struct AiBridgeHost {
    paths: BridgePaths,
    last_status: Option<Instant>,
    last_poll: Option<Instant>,
    last_control: Option<Instant>,
    seen_requests: HashSet<String>,
    /// 供设置页/诊断展示的最近一次桥错误（不参与判定）。
    pub last_error: Option<String>,
}

impl Default for AiBridgeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl AiBridgeHost {
    pub fn new() -> Self {
        Self {
            paths: BridgePaths::resolve(),
            last_status: None,
            last_poll: None,
            last_control: None,
            seen_requests: HashSet::new(),
            last_error: None,
        }
    }

    /// 测试用：显式指定桥目录。
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: BridgePaths::with_root(root),
            ..Self::new()
        }
    }

    pub fn paths(&self) -> &BridgePaths {
        &self.paths
    }

    /// 把会话级开关状态投影给 agent（只读投影，不落盘设置）。
    pub fn publish_status(&mut self, autonomous: bool, imported_targets: &[String], force: bool) {
        let due = self
            .last_status
            .map_or(true, |at| at.elapsed() >= STATUS_REFRESH);
        if !force && !due {
            return;
        }
        let status = BridgeStatus::new("GUI", autonomous, imported_targets.to_vec());
        match self.paths.write_status(&status) {
            Ok(()) => {
                self.last_status = Some(Instant::now());
                self.last_error = None;
            }
            Err(error) => self.last_error = Some(format!("状态投影失败：{error}")),
        }
    }

    /// 消费 agent 的自主关闭指令（一次性）。
    pub fn take_controls(&self) -> Vec<BridgeControl> {
        self.paths.take_controls()
    }

    /// 返回新到达、且未被本宿主处理的请求（每个请求只返回一次）。
    pub fn poll_requests(&mut self, force: bool) -> Vec<BridgeRequest> {
        let due = self
            .last_poll
            .map_or(true, |at| at.elapsed() >= REQUEST_POLL);
        if !force && !due {
            return Vec::new();
        }
        self.last_poll = Some(Instant::now());
        self.paths.cleanup_expired();
        let mut fresh = Vec::new();
        for request in self.paths.read_requests() {
            if self.seen_requests.insert(request.request_id.clone()) {
                fresh.push(request);
            }
        }
        fresh
    }

    /// 只取一个待处理请求（GUI 单活动运行：其余请求留在队列，TTL 到期自然过期）。
    pub fn take_first_request(&mut self, force: bool) -> Option<BridgeRequest> {
        let due = self
            .last_poll
            .map_or(true, |at| at.elapsed() >= REQUEST_POLL);
        if !force && !due {
            return None;
        }
        self.last_poll = Some(Instant::now());
        self.paths.cleanup_expired();
        for request in self.paths.read_requests() {
            if !self.seen_requests.contains(&request.request_id) {
                self.seen_requests.insert(request.request_id.clone());
                return Some(request);
            }
        }
        None
    }

    /// 用户确认（F3）：写入授权决定。`autonomous=false` 即路径 A。
    pub fn approve(
        &self,
        request: &BridgeRequest,
        autonomous: bool,
    ) -> Result<BridgeDecision, String> {
        let decision = BridgeDecision::approve(request, autonomous);
        self.paths
            .write_decision(&decision)
            .map_err(|error| format!("写入授权失败：{error}"))?;
        Ok(decision)
    }

    /// 用户拒绝：直接回传 DENIED 结果（不产生任何发包）。
    pub fn deny(&self, request: &BridgeRequest) -> Result<BridgeResult, String> {
        let result = BridgeResult::denied(request, "用户拒绝授权或关闭了弹窗");
        self.paths
            .write_result(&result)
            .map_err(|error| format!("写入拒绝结果失败：{error}"))?;
        Ok(result)
    }

    /// 真实运行结束：回传脱敏报告。
    pub fn complete(
        &self,
        request: &BridgeRequest,
        report: Report,
        autonomous: bool,
    ) -> Result<BridgeResult, String> {
        let result = BridgeResult::completed(request, report, autonomous);
        self.paths
            .write_result(&result)
            .map_err(|error| format!("写入结果失败：{error}"))?;
        Ok(result)
    }

    /// 运行失败（队列拒绝/配置错误）：显式回传失败，绝不伪造报告。
    pub fn fail(
        &self,
        request: &BridgeRequest,
        error: impl Into<String>,
    ) -> Result<BridgeResult, String> {
        let result = BridgeResult::failed(request, error);
        self.paths
            .write_result(&result)
            .map_err(|error| format!("写入失败结果失败：{error}"))?;
        Ok(result)
    }

    /// 读取状态文件（宿主自身诊断用）。
    pub fn status(&self) -> Option<BridgeStatus> {
        self.paths.read_status()
    }

    /// 把控制指令映射为布尔（便于调用方直接处理）；与状态投影同频，避免每帧读盘。
    pub fn take_disable_requests(&mut self, force: bool) -> Vec<Option<String>> {
        let due = self
            .last_control
            .map_or(true, |at| at.elapsed() >= STATUS_REFRESH);
        if !force && !due {
            return Vec::new();
        }
        self.last_control = Some(Instant::now());
        self.take_controls()
            .into_iter()
            .filter(|control| control.action == ControlAction::DisableAutonomous)
            .map(|control| control.reason)
            .collect()
    }
}

/// 校验请求目标是否全部是宿主当前**已导入**的用户目标（真实发包前提）。
pub fn request_is_importable(request: &BridgeRequest, imported: &[String]) -> bool {
    if request.scope.targets.is_empty() {
        // profile 请求：内置目录默认零发包，允许执行但不会有网络请求。
        return true;
    }
    request
        .scope
        .targets
        .iter()
        .all(|target| imported.iter().any(|id| id == target))
}

/// 供 UI 展示的范围摘要（不含 URL）。
pub fn request_scope_label(request: &BridgeRequest) -> String {
    let mut scope = request.scope.label();
    if !request.scope.purpose.is_empty() {
        scope.push_str(" · ");
        scope.push_str(&request.scope.purpose);
    }
    scope
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallaxe_core::ai_bridge::AiScope;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "parallaxe-gui-bridge-test-{name}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ))
    }

    #[test]
    fn publishes_session_only_status_and_returns_requests_once() {
        let root = temp_root("poll");
        let mut host = AiBridgeHost::with_root(&root);
        host.publish_status(true, &["example.com".to_string()], true);
        let status = host.status().unwrap();
        assert!(status.autonomous_enabled);
        assert!(status.session_only);

        let request = BridgeRequest::new(
            AiScope::for_targets(["example.com"], "用户导入首跳"),
            3_500,
            5,
        )
        .unwrap();
        host.paths().write_request(&request).unwrap();
        let first = host.poll_requests(true);
        assert_eq!(first.len(), 1);
        assert!(host.poll_requests(true).is_empty(), "同一请求只交付一次");
    }

    #[test]
    fn requests_are_queued_one_at_a_time_without_dropping() {
        let root = temp_root("queue");
        let mut host = AiBridgeHost::with_root(&root);
        let a = BridgeRequest::new(AiScope::for_targets(["a.example"], "x"), 3_500, 5).unwrap();
        let b = BridgeRequest::new(AiScope::for_targets(["b.example"], "x"), 3_500, 5).unwrap();
        host.paths().write_request(&a).unwrap();
        host.paths().write_request(&b).unwrap();
        let first = host.take_first_request(true).expect("first request");
        let second = host.take_first_request(true).expect("second request");
        assert_ne!(first.request_id, second.request_id);
        assert!(host.take_first_request(true).is_none());
    }

    #[test]
    fn approve_and_complete_write_sanitized_result() {
        let root = temp_root("complete");
        let mut host = AiBridgeHost::with_root(&root);
        host.publish_status(false, &["example.com".to_string()], true);
        let request = BridgeRequest::new(
            AiScope::for_targets(["example.com"], "用户导入首跳"),
            3_500,
            5,
        )
        .unwrap();
        host.paths().write_request(&request).unwrap();
        assert!(request_is_importable(
            &request,
            &["example.com".to_string()]
        ));
        assert!(!request_is_importable(
            &request,
            &["other.example".to_string()]
        ));

        host.approve(&request, false).unwrap();
        assert!(host.paths().read_decision(&request.request_id).is_some());

        let mut report = Report::new(
            parallaxe_core::VERSION,
            "2026-11-25T00:00:00Z",
            "run-gui-bridge",
            parallaxe_core::Mode::Online,
        );
        report.network_request_count = 1;
        let result = host.complete(&request, report, false).unwrap();
        assert_eq!(result.network_request_count, 1);
        let stored = host.paths().read_result(&request.request_id).unwrap();
        assert_eq!(
            stored.status,
            parallaxe_core::ai_bridge::ResultStatus::Completed
        );
    }

    #[test]
    fn agent_disable_control_is_consumed() {
        let root = temp_root("disable");
        let mut host = AiBridgeHost::with_root(&root);
        host.paths()
            .write_control(&BridgeControl::disable_autonomous(Some(
                "批量结束".to_string(),
            )))
            .unwrap();
        let reasons = host.take_disable_requests(true);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].as_deref(), Some("批量结束"));
        assert!(host.take_disable_requests(true).is_empty());
    }

    #[test]
    fn label_does_not_leak_urls() {
        let request = BridgeRequest::new(
            AiScope::for_targets(["example.com"], "只观测首跳"),
            3_500,
            5,
        )
        .unwrap();
        let label = request_scope_label(&request);
        assert!(label.contains("example.com"));
        assert!(!label.contains("://"));
    }
}
