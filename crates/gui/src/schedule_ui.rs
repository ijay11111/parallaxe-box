//! T-7-11 网络画像、间隔锁定与保守自适应退让状态（纯调度参数层，零判定逻辑）。
//!
//! 红线：
//! - 画像仅为提示性用户配置，不主动探测运营商，也不进入任何判定或地区信号；
//! - 自适应退让只消费 `parallaxe-core` 报告里既有的 `risk_signal.markers`（如 `LOCAL_QOS_SUSPECT`），
//!   只做保守放慢（×1.5，封顶 GUI 上限 5000ms（全员规范）），永不反向加速；
//! - 优先级：手动锁定 > 自适应退让 > 网络画像基线。

use parallaxe_core::i18n::{translate, Locale};
use parallaxe_core::runtime::CheckOptions;
use parallaxe_core::targets::HARD_LIMITS;

/// GUI 调度参数上限（全员规范 3000–5000ms；网络检测的 core 上限可到 10000ms，两者分层）。
pub const GUI_MAX_INTERVAL_MS: u64 = 5_000;

/// 用户自选的网络画像（提示性配置，不探测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkProfileKind {
    Mobile,
    HomeBroadband,
    Campus,
    Idc,
    #[default]
    Unknown,
}

impl NetworkProfileKind {
    pub const ALL: [NetworkProfileKind; 5] = [
        NetworkProfileKind::Mobile,
        NetworkProfileKind::HomeBroadband,
        NetworkProfileKind::Campus,
        NetworkProfileKind::Idc,
        NetworkProfileKind::Unknown,
    ];

    pub fn code(self) -> &'static str {
        match self {
            NetworkProfileKind::Mobile => "MOBILE",
            NetworkProfileKind::HomeBroadband => "HOME_BROADBAND",
            NetworkProfileKind::Campus => "CAMPUS",
            NetworkProfileKind::Idc => "IDC",
            NetworkProfileKind::Unknown => "UNKNOWN",
        }
    }

    pub fn label(self, locale: Locale) -> String {
        let key = match self {
            NetworkProfileKind::Mobile => "gui.schedule.profile.mobile",
            NetworkProfileKind::HomeBroadband => "gui.schedule.profile.home",
            NetworkProfileKind::Campus => "gui.schedule.profile.campus",
            NetworkProfileKind::Idc => "gui.schedule.profile.idc",
            NetworkProfileKind::Unknown => "gui.schedule.profile.unknown",
        };
        translate(locale, key, &[])
    }
}

/// 运营商提示（仅用户自填，不探测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IspHint {
    #[default]
    Unknown,
    Unicom,
    Mobile,
    Telecom,
    Other,
}

impl IspHint {
    pub const ALL: [IspHint; 5] = [
        IspHint::Unknown,
        IspHint::Unicom,
        IspHint::Mobile,
        IspHint::Telecom,
        IspHint::Other,
    ];

    pub fn code(self) -> &'static str {
        match self {
            IspHint::Unknown => "UNKNOWN",
            IspHint::Unicom => "UNICOM",
            IspHint::Mobile => "MOBILE",
            IspHint::Telecom => "TELECOM",
            IspHint::Other => "OTHER",
        }
    }

    pub fn label(self, locale: Locale) -> String {
        let key = match self {
            IspHint::Unknown => "gui.schedule.isp.unknown",
            IspHint::Unicom => "gui.schedule.isp.unicom",
            IspHint::Mobile => "gui.schedule.isp.mobile",
            IspHint::Telecom => "gui.schedule.isp.telecom",
            IspHint::Other => "gui.schedule.isp.other",
        };
        translate(locale, key, &[])
    }
}

/// 时段策略（晚高峰更保守 / 固定基线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeOfDayPolicy {
    #[default]
    PeakAware,
    Fixed,
}

impl TimeOfDayPolicy {
    pub const ALL: [TimeOfDayPolicy; 2] = [TimeOfDayPolicy::PeakAware, TimeOfDayPolicy::Fixed];

    pub fn code(self) -> &'static str {
        match self {
            TimeOfDayPolicy::PeakAware => "PEAK_AWARE",
            TimeOfDayPolicy::Fixed => "FIXED",
        }
    }

    pub fn label(self, locale: Locale) -> String {
        let key = match self {
            TimeOfDayPolicy::PeakAware => "gui.schedule.tod.peak_aware",
            TimeOfDayPolicy::Fixed => "gui.schedule.tod.fixed",
        };
        translate(locale, key, &[])
    }
}

/// 单次自适应调度退让记录（仅记录调度参数变化，不进判定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleAdjustment {
    pub run_id: String,
    pub from_ms: u64,
    pub to_ms: u64,
    pub reason: String,
}

/// 解析后的生效调度参数（用于风险预览与 `CheckOptions` 透传）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSchedule {
    pub min_interval_ms: u64,
    pub batch_size: u32,
    pub batch_cooldown_ms: u64,
    pub source_code: &'static str,
    pub reason_zh: String,
    pub reason_en: String,
}

impl EffectiveSchedule {
    pub fn source_text(&self, locale: Locale) -> &str {
        match locale {
            Locale::ZhCn => &self.reason_zh,
            Locale::EnUs => &self.reason_en,
        }
    }
}

/// 设置页与工作台持有的调度偏好。
#[derive(Debug, Clone)]
pub struct SchedulePrefs {
    pub profile_kind: NetworkProfileKind,
    pub isp_hint: IspHint,
    pub time_of_day: TimeOfDayPolicy,
    /// 用户锁定间隔后，QoS 仅告警不自动改值（等同于关闭限速自动放慢）。
    pub lock_interval: bool,
    /// 用户手动指定的最小间隔（毫秒）。
    pub manual_interval_ms: u64,
    /// 用户在滑块上显式自定义的基础间隔（非锁定状态下也可直接生效，同时保留限速自动放慢能力）。
    pub custom_interval_ms: Option<u64>,
    /// 用户自定义的每批检测数量（None 时跟随当前网络环境基线）。
    pub custom_batch_size: Option<u32>,
    /// 跨目标（不同 host）检验之间的间隔（0=不额外延迟；可调至 10s）。
    pub inter_target_interval_ms: u64,
    /// 自适应退让后的保守间隔（只增不减，封顶 GUI 上限 5000ms（全员规范））。
    pub adaptive_interval_ms: Option<u64>,
    /// 本会话内的调度调整日志。
    pub adjustments: Vec<ScheduleAdjustment>,
    /// 已处理 QoS 标记的 run_id（避免同一 run 重复退让）。
    pub last_qos_run_id: Option<String>,
    /// 当前是否展示克制的 QoS 退让提示条。
    pub qos_banner_active: bool,
}

impl Default for SchedulePrefs {
    fn default() -> Self {
        let default_kind = if cfg!(target_os = "android") {
            NetworkProfileKind::Mobile
        } else {
            NetworkProfileKind::Unknown
        };
        let default_manual = if cfg!(target_os = "android") {
            4_500
        } else {
            3_500
        };
        SchedulePrefs {
            profile_kind: default_kind,
            isp_hint: IspHint::Unknown,
            time_of_day: TimeOfDayPolicy::Fixed,
            lock_interval: false,
            manual_interval_ms: default_manual,
            custom_interval_ms: None,
            custom_batch_size: None,
            inter_target_interval_ms: 0,
            adaptive_interval_ms: None,
            adjustments: Vec::new(),
            last_qos_run_id: None,
            qos_banner_active: false,
        }
    }
}

impl SchedulePrefs {
    /// 按画像与时段策略计算保守基线，间隔封顶 5000ms、批次/冷却封顶于 `HARD_LIMITS`。
    pub fn profile_baseline(&self) -> (u64, u32, u64) {
        let (mut interval, mut batch, mut cooldown) = match self.profile_kind {
            NetworkProfileKind::Mobile => (4_500_u64, 3_u32, 55_000_u64),
            NetworkProfileKind::HomeBroadband => (3_500_u64, 5_u32, 45_000_u64),
            NetworkProfileKind::Campus => (5_000_u64, 3_u32, 60_000_u64),
            NetworkProfileKind::Idc => (4_200_u64, 4_u32, 50_000_u64),
            NetworkProfileKind::Unknown => (3_500_u64, 5_u32, 45_000_u64),
        };
        if self.isp_hint == IspHint::Telecom {
            // 电信骨干晚间对 UDP/高频连接更敏感，基线再略保守（仍在 GUI 规范内）。
            interval = interval.saturating_add(500);
            batch = batch.min(4);
            cooldown = cooldown.saturating_add(5_000);
        }
        if self.time_of_day == TimeOfDayPolicy::PeakAware {
            interval = interval.saturating_add(300);
            cooldown = cooldown.saturating_add(5_000);
        }
        (
            interval.min(GUI_MAX_INTERVAL_MS),
            batch.clamp(1, HARD_LIMITS.max_batch_size),
            cooldown.min(HARD_LIMITS.max_batch_cooldown_ms),
        )
    }

    /// 优先级解析：手动锁定 > 自适应退让 > 用户自定义基础间隔 / 网络环境基线。
    pub fn effective(&self) -> EffectiveSchedule {
        let (profile_interval, profile_batch, batch_cooldown_ms) = self.profile_baseline();
        let batch_size = self
            .custom_batch_size
            .unwrap_or(profile_batch)
            .clamp(1, HARD_LIMITS.max_batch_size);
        let profile_zh = self.profile_kind.label(Locale::ZhCn);
        let profile_en = self.profile_kind.label(Locale::EnUs);
        let tod_zh = self.time_of_day.label(Locale::ZhCn);
        let tod_en = self.time_of_day.label(Locale::EnUs);
        let isp_zh = self.isp_hint.label(Locale::ZhCn);
        let isp_en = self.isp_hint.label(Locale::EnUs);

        if self.lock_interval {
            let locked = self.manual_interval_ms.min(GUI_MAX_INTERVAL_MS);
            return EffectiveSchedule {
                min_interval_ms: locked,
                batch_size,
                batch_cooldown_ms,
                source_code: "MANUAL_LOCKED",
                reason_zh: format!("已手动锁定间隔为 {locked} ms（批次与冷却参考：{profile_zh}）"),
                reason_en: format!(
                    "Interval manually locked at {locked} ms (batch/cooldown from {profile_en})"
                ),
            };
        }
        let base_interval = self
            .custom_interval_ms
            .unwrap_or(profile_interval)
            .min(GUI_MAX_INTERVAL_MS);
        if let Some(adapted) = self.adaptive_interval_ms {
            let clamped = adapted.max(base_interval).min(GUI_MAX_INTERVAL_MS);
            return EffectiveSchedule {
                min_interval_ms: clamped,
                batch_size: batch_size.min(3),
                batch_cooldown_ms: batch_cooldown_ms
                    .clamp(50_000, HARD_LIMITS.max_batch_cooldown_ms),
                source_code: "ADAPTIVE_BACKOFF",
                reason_zh: format!(
                    "网络限速自动放慢至 {clamped} ms（基础节奏：{profile_zh} · {tod_zh}）"
                ),
                reason_en: format!(
                    "Slowed down automatically to {clamped} ms (baseline: {profile_en} · {tod_en})"
                ),
            };
        }
        if self.custom_interval_ms.is_some() {
            return EffectiveSchedule {
                min_interval_ms: base_interval,
                batch_size,
                batch_cooldown_ms,
                source_code: "USER_CUSTOM",
                reason_zh: format!(
                    "自定义节奏 {base_interval} ms（限速自动放慢已开启 · {profile_zh}）"
                ),
                reason_en: format!(
                    "Custom pace {base_interval} ms (auto slow-down enabled · {profile_en})"
                ),
            };
        }
        EffectiveSchedule {
            min_interval_ms: base_interval,
            batch_size,
            batch_cooldown_ms,
            source_code: "NETWORK_PROFILE",
            reason_zh: format!("{profile_zh} · {tod_zh} · 运营商：{isp_zh}"),
            reason_en: format!("{profile_en} · {tod_en} · ISP: {isp_en}"),
        }
    }

    /// 用户在滑块或预设按钮上调整请求间隔（始终可拖、立即生效）。
    pub fn set_interval_ms(&mut self, ms: u64) {
        let clamped = ms.min(GUI_MAX_INTERVAL_MS);
        self.manual_interval_ms = clamped;
        self.custom_interval_ms = Some(clamped);
        self.adaptive_interval_ms = None;
    }

    /// 用户调整每批并发数量。
    pub fn set_batch_size(&mut self, batch: u32) {
        self.custom_batch_size = Some(batch.clamp(1, HARD_LIMITS.max_batch_size));
    }

    /// 当前是否开启「限速自动放慢（QoS 自适应）」。
    pub fn qos_adaptive_enabled(&self) -> bool {
        !self.lock_interval
    }

    /// 切换「限速自动放慢（QoS 自适应）」。
    pub fn set_qos_adaptive(&mut self, enabled: bool) {
        self.lock_interval = !enabled;
        if self.lock_interval {
            self.manual_interval_ms = self.effective().min_interval_ms;
        }
    }

    /// 当前生效间隔是否低于 3000ms（用于展示非模态风控提示）。
    pub fn is_low_interval(&self) -> bool {
        self.effective().min_interval_ms < 3_000
    }

    /// 切换网络环境时重置到新环境基线（若未锁定）。
    pub fn select_profile_kind(&mut self, kind: NetworkProfileKind) {
        self.profile_kind = kind;
        self.custom_interval_ms = None;
        self.custom_batch_size = None;
        self.adaptive_interval_ms = None;
        if !self.lock_interval {
            self.manual_interval_ms = self.profile_baseline().0;
        }
    }

    /// 把生效调度参数透传给 `CheckOptions`（仅影响调度，绝不影响判定）。
    pub fn apply_to_options(&self, options: &mut CheckOptions) {
        let eff = self.effective();
        options.min_interval_ms = Some(eff.min_interval_ms);
        options.inter_target_interval_ms = Some(self.inter_target_interval_ms);
        options.batch_size = Some(eff.batch_size);
        options.batch_cooldown_ms = Some(eff.batch_cooldown_ms);
    }

    /// 消费 `parallaxe-core` 终态报告里既有的 QoS 标记；只做保守放慢，永不加速。
    pub fn on_run_qos_observed(&mut self, run_id: &str, markers: &[String]) {
        let has_qos = markers
            .iter()
            .any(|m| m == "LOCAL_QOS_SUSPECT" || m == "MULTI_AIRPORT_UDP_FAILURE");
        if !has_qos {
            return;
        }
        if self.last_qos_run_id.as_deref() == Some(run_id) {
            return;
        }
        self.last_qos_run_id = Some(run_id.to_string());
        self.qos_banner_active = true;
        if self.lock_interval {
            // 锁定后只告警、不修改数值。
            return;
        }
        let current = self.effective().min_interval_ms;
        let stepped = ((current.max(1_000) as f64) * 1.5).round() as u64;
        let next = stepped.max(current).min(GUI_MAX_INTERVAL_MS);
        self.adaptive_interval_ms = Some(next);
        self.manual_interval_ms = next;
        self.adjustments.push(ScheduleAdjustment {
            run_id: run_id.to_string(),
            from_ms: current,
            to_ms: next,
            reason: "LOCAL_QOS_SUSPECT".to_string(),
        });
    }

    /// 一键锁定当前生效间隔。
    pub fn lock_current(&mut self) {
        self.manual_interval_ms = self.effective().min_interval_ms;
        self.lock_interval = true;
    }

    /// 清除自适应退让与自定义覆盖，回到当前网络环境推荐基线。
    pub fn reset_to_baseline(&mut self) {
        self.adaptive_interval_ms = None;
        self.custom_interval_ms = None;
        self.custom_batch_size = None;
        self.qos_banner_active = false;
        if !self.lock_interval {
            self.manual_interval_ms = self.profile_baseline().0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_profiles_and_policies_respect_hard_limits() {
        for kind in NetworkProfileKind::ALL {
            for isp in IspHint::ALL {
                for tod in TimeOfDayPolicy::ALL {
                    let prefs = SchedulePrefs {
                        profile_kind: kind,
                        isp_hint: isp,
                        time_of_day: tod,
                        ..Default::default()
                    };
                    let eff = prefs.effective();
                    assert!(eff.min_interval_ms <= GUI_MAX_INTERVAL_MS);
                    assert!(eff.batch_size >= 1 && eff.batch_size <= HARD_LIMITS.max_batch_size);
                    assert!(eff.batch_cooldown_ms <= HARD_LIMITS.max_batch_cooldown_ms);
                }
            }
        }
    }

    #[test]
    fn adaptive_backoff_only_slows_down_caps_at_hard_limit_and_respects_lock() {
        let mut prefs = SchedulePrefs {
            profile_kind: NetworkProfileKind::HomeBroadband,
            time_of_day: TimeOfDayPolicy::Fixed,
            ..Default::default()
        };
        assert_eq!(prefs.effective().min_interval_ms, 3_500);

        // 注入核心 LOCAL_QOS_SUSPECT 标记 -> 3500 * 1.5 = 5250 -> GUI 封顶 5000
        prefs.on_run_qos_observed("run-qos-1", &["LOCAL_QOS_SUSPECT".to_string()]);
        assert!(prefs.qos_banner_active);
        assert_eq!(prefs.effective().min_interval_ms, 5_000);
        assert_eq!(prefs.effective().source_code, "ADAPTIVE_BACKOFF");
        assert_eq!(prefs.adjustments.len(), 1);
        assert_eq!(prefs.adjustments[0].from_ms, 3_500);
        assert_eq!(prefs.adjustments[0].to_ms, 5_000);

        // 锁定手动值：优先级 手动锁定 > 自适应 > 画像；再收到 QoS 仅告警不改值
        prefs.manual_interval_ms = 3_000;
        prefs.lock_interval = true;
        assert_eq!(prefs.effective().min_interval_ms, 3_000);
        assert_eq!(prefs.effective().source_code, "MANUAL_LOCKED");

        prefs.on_run_qos_observed("run-qos-2", &["MULTI_AIRPORT_UDP_FAILURE".to_string()]);
        assert_eq!(prefs.effective().min_interval_ms, 3_000);
        assert_eq!(prefs.adjustments.len(), 1, "锁定后不得新增自动修改记录");
    }
}
