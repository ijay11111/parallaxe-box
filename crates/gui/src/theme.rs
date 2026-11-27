//! 双主题视觉令牌、9 态强类型徽章、7 归因层标签与微动画辅助。
//!
//! 铁律：
//! - 9 个 `State` 全部穷举匹配，禁止 `_ =>` 通配符。
//! - 仅 `State::Ok` 允许使用绿色；`ReachableOnly`、`Unknown`、`Unsupported` 绝不画成绿色。
//! - 图标 + 中文词 + 枚举码三要素同显，不以红绿作为唯一编码。
//! - Dark / Light 切换仅改变色彩令牌，绝不改变卡片数量或布局结构。

use egui::{
    vec2, Color32, Context, FontData, FontDefinitions, FontFamily, Margin, Rounding, Sense, Stroke,
    Ui, Visuals,
};
use parallaxe_core::contracts::{
    BindStatus, Confidence, ExpectationStatus, FreshnessStatus, Layer, LayerRef, Mode,
    ObservedFamily, RequestedFamily, Source, State, Termination,
};
use parallaxe_core::i18n::{translate, Locale};

/// 4 / 8 基准间距系统（杜绝 3/5/7/14/21 等随手写的杂散像素）。
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
/// 基准档：卡片内边距与最小卡片间距（满足“卡片间距 >= 卡片内边距”）。
pub const SPACE_4: f32 = 16.0;
/// 组间间距 / 宽松卡片间距（严格大于同组内间距）。
pub const SPACE_5: f32 = 20.0;
/// 页面左右留白与主区块间距。
pub const SPACE_6: f32 = 24.0;
/// 大区块分隔间距。
pub const SPACE_8: f32 = 32.0;

/// 统一圆角体系。
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_PILL: f32 = 16.0;

/// 严格收敛的 3 级字号层级（+ 12px 次要元信息/徽章标签）。
pub const FONT_DISPLAY: f32 = 24.0;
pub const FONT_TITLE: f32 = 16.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_META: f32 = 12.0;

/// 表格行密度预设（参考 the_grid SizePreset：仅调行高与内边距，绝不隐藏任何冲突或 UNKNOWN）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableDensity {
    #[default]
    Comfortable,
    Compact,
}

impl TableDensity {
    pub fn label(self) -> &'static str {
        match self {
            TableDensity::Comfortable => "舒适",
            TableDensity::Compact => "紧凑",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        match self {
            TableDensity::Comfortable => translate(locale, "gui.matrix.density.comfortable", &[]),
            TableDensity::Compact => translate(locale, "gui.matrix.density.compact", &[]),
        }
    }

    pub fn row_height(self) -> f32 {
        match self {
            TableDensity::Comfortable => 48.0,
            TableDensity::Compact => 38.0,
        }
    }

    pub fn cell_v_pad(self) -> f32 {
        match self {
            TableDensity::Comfortable => SPACE_2,
            TableDensity::Compact => SPACE_1,
        }
    }
}

/// 明暗主题模式（纯视觉维度，不改变布局结构或核心判定数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn toggle(self) -> Self {
        match self {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Dark => "深色",
            ThemeMode::Light => "浅色",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        match self {
            ThemeMode::Dark => translate(locale, "gui.top.theme.dark", &[]),
            ThemeMode::Light => translate(locale, "gui.top.theme.light", &[]),
        }
    }
}

/// 信息密度模式（`Normal` 完整仪表盘 vs `Minimal` 极简三卡；不改变运行范围、预算或核心数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DensityMode {
    #[default]
    Normal,
    Minimal,
}

impl DensityMode {
    pub fn toggle(self) -> Self {
        match self {
            DensityMode::Normal => DensityMode::Minimal,
            DensityMode::Minimal => DensityMode::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DensityMode::Normal => "完整",
            DensityMode::Minimal => "极简",
        }
    }

    pub fn label_localized(self, locale: Locale) -> String {
        match self {
            DensityMode::Normal => translate(locale, "gui.top.density.normal", &[]),
            DensityMode::Minimal => translate(locale, "gui.top.density.minimal", &[]),
        }
    }
}

/// 语义化主题色板（参考 gpui-kit 语义 token 分层与现代桌面暗/亮表面层次，全部满足 WCAG AA 对比度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub canvas: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_alt: Color32,
    pub surface_hover: Color32,
    pub table_head_bg: Color32,
    pub table_even_bg: Color32,
    pub border: Color32,
    pub border_subtle: Color32,
    pub border_hover: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_fg: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub warning_bg: Color32,
    pub warning_fg: Color32,
    pub danger_bg: Color32,
    pub danger_fg: Color32,
    pub info_bg: Color32,
    pub info_fg: Color32,
    pub ok_bg: Color32,
    pub ok_fg: Color32,
    pub scrim: Color32,
}

impl Palette {
    pub fn for_theme(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Palette {
                canvas: Color32::from_rgb(10, 16, 28),
                sidebar: Color32::from_rgb(13, 21, 36),
                card: Color32::from_rgb(17, 27, 46),
                card_alt: Color32::from_rgb(22, 34, 58),
                surface_hover: Color32::from_rgb(27, 42, 70),
                table_head_bg: Color32::from_rgb(14, 23, 40),
                table_even_bg: Color32::from_rgb(20, 31, 52),
                border: Color32::from_rgb(35, 55, 88),
                border_subtle: Color32::from_rgb(26, 41, 68),
                border_hover: Color32::from_rgb(45, 185, 218),
                accent: Color32::from_rgb(34, 211, 238),
                accent_soft: Color32::from_rgb(16, 54, 74),
                accent_fg: Color32::from_rgb(7, 20, 33),
                text_primary: Color32::from_rgb(241, 245, 249),
                text_secondary: Color32::from_rgb(156, 177, 204),
                text_muted: Color32::from_rgb(112, 134, 166),
                warning_bg: Color32::from_rgb(48, 35, 14),
                warning_fg: Color32::from_rgb(251, 191, 36),
                danger_bg: Color32::from_rgb(54, 20, 28),
                danger_fg: Color32::from_rgb(248, 113, 113),
                info_bg: Color32::from_rgb(14, 42, 68),
                info_fg: Color32::from_rgb(56, 189, 248),
                ok_bg: Color32::from_rgb(14, 46, 34),
                ok_fg: Color32::from_rgb(74, 222, 128),
                scrim: Color32::from_rgba_unmultiplied(4, 8, 16, 155),
            },
            ThemeMode::Light => Palette {
                canvas: Color32::from_rgb(244, 247, 251),
                sidebar: Color32::from_rgb(236, 241, 248),
                card: Color32::from_rgb(255, 255, 255),
                card_alt: Color32::from_rgb(246, 249, 253),
                surface_hover: Color32::from_rgb(233, 240, 249),
                table_head_bg: Color32::from_rgb(241, 245, 251),
                table_even_bg: Color32::from_rgb(249, 251, 254),
                border: Color32::from_rgb(214, 224, 238),
                border_subtle: Color32::from_rgb(229, 236, 245),
                border_hover: Color32::from_rgb(8, 145, 178),
                accent: Color32::from_rgb(0, 115, 140),
                accent_soft: Color32::from_rgb(224, 244, 250),
                accent_fg: Color32::from_rgb(255, 255, 255),
                text_primary: Color32::from_rgb(15, 23, 42),
                text_secondary: Color32::from_rgb(71, 85, 105),
                text_muted: Color32::from_rgb(90, 105, 127),
                warning_bg: Color32::from_rgb(254, 245, 214),
                warning_fg: Color32::from_rgb(180, 83, 9),
                danger_bg: Color32::from_rgb(254, 230, 230),
                danger_fg: Color32::from_rgb(185, 28, 28),
                info_bg: Color32::from_rgb(226, 243, 254),
                info_fg: Color32::from_rgb(3, 105, 161),
                ok_bg: Color32::from_rgb(220, 252, 231),
                ok_fg: Color32::from_rgb(21, 128, 61),
                scrim: Color32::from_rgba_unmultiplied(15, 23, 42, 115),
            },
        }
    }
}

/// 强类型状态徽章规范（与 `State` 严格一对一锁定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateBadge {
    pub state: State,
    pub symbol: &'static str,
    pub zh_label: &'static str,
    /// 稳定 i18n key（供 GUI 多语言渲染；`zh_label` 保留为兜底）。
    pub label_key: &'static str,
    pub code: &'static str,
    pub fg: Color32,
    pub bg: Color32,
    pub border: Color32,
    /// 是否属于绿色视觉语义（铁律：仅 `State::Ok` 为 true）。
    pub is_green: bool,
}

impl StateBadge {
    /// 按 locale 渲染状态标签；语言包缺失时回退到 `zh_label`。
    pub fn label(&self, locale: Locale) -> String {
        let text = translate(locale, self.label_key, &[]);
        if text == self.label_key {
            self.zh_label.to_string()
        } else {
            text
        }
    }
}

/// 9 个顶层 `State` 的穷举徽章映射（禁用 `_ =>` 兜底；全部使用 BMP 通用符号，无豆腐块）。
pub fn state_badge(state: State, theme: ThemeMode) -> StateBadge {
    match (state, theme) {
        (State::Ok, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "✓",
            zh_label: "正常",
            label_key: "state.ok",
            code: state.as_str(),
            fg: Color32::from_rgb(74, 222, 128),
            bg: Color32::from_rgb(15, 48, 35),
            border: Color32::from_rgb(34, 197, 94),
            is_green: true,
        },
        (State::Ok, ThemeMode::Light) => StateBadge {
            state,
            symbol: "✓",
            zh_label: "正常",
            label_key: "state.ok",
            code: state.as_str(),
            fg: Color32::from_rgb(21, 128, 61),
            bg: Color32::from_rgb(220, 252, 231),
            border: Color32::from_rgb(34, 197, 94),
            is_green: true,
        },
        (State::ReachableOnly, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "●",
            zh_label: "仅可达",
            label_key: "state.reachable_only",
            code: state.as_str(),
            fg: Color32::from_rgb(56, 189, 248),
            bg: Color32::from_rgb(12, 41, 66),
            border: Color32::from_rgb(14, 165, 233),
            is_green: false,
        },
        (State::ReachableOnly, ThemeMode::Light) => StateBadge {
            state,
            symbol: "●",
            zh_label: "仅可达",
            label_key: "state.reachable_only",
            code: state.as_str(),
            fg: Color32::from_rgb(3, 105, 161),
            bg: Color32::from_rgb(224, 242, 254),
            border: Color32::from_rgb(2, 132, 199),
            is_green: false,
        },
        (State::Restricted, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "!",
            zh_label: "受限",
            label_key: "state.restricted",
            code: state.as_str(),
            fg: Color32::from_rgb(251, 191, 36),
            bg: Color32::from_rgb(54, 39, 12),
            border: Color32::from_rgb(245, 158, 11),
            is_green: false,
        },
        (State::Restricted, ThemeMode::Light) => StateBadge {
            state,
            symbol: "!",
            zh_label: "受限",
            label_key: "state.restricted",
            code: state.as_str(),
            fg: Color32::from_rgb(180, 83, 9),
            bg: Color32::from_rgb(254, 243, 199),
            border: Color32::from_rgb(217, 119, 6),
            is_green: false,
        },
        (State::Blocked, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "×",
            zh_label: "阻断",
            label_key: "state.blocked",
            code: state.as_str(),
            fg: Color32::from_rgb(248, 113, 113),
            bg: Color32::from_rgb(58, 21, 28),
            border: Color32::from_rgb(239, 68, 68),
            is_green: false,
        },
        (State::Blocked, ThemeMode::Light) => StateBadge {
            state,
            symbol: "×",
            zh_label: "阻断",
            label_key: "state.blocked",
            code: state.as_str(),
            fg: Color32::from_rgb(185, 28, 28),
            bg: Color32::from_rgb(254, 226, 226),
            border: Color32::from_rgb(220, 38, 38),
            is_green: false,
        },
        (State::LocalPathError, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "!",
            zh_label: "链路异常",
            label_key: "state.local_path_error",
            code: state.as_str(),
            fg: Color32::from_rgb(251, 113, 133),
            bg: Color32::from_rgb(56, 20, 31),
            border: Color32::from_rgb(244, 63, 94),
            is_green: false,
        },
        (State::LocalPathError, ThemeMode::Light) => StateBadge {
            state,
            symbol: "!",
            zh_label: "链路异常",
            label_key: "state.local_path_error",
            code: state.as_str(),
            fg: Color32::from_rgb(190, 18, 60),
            bg: Color32::from_rgb(255, 228, 230),
            border: Color32::from_rgb(225, 29, 72),
            is_green: false,
        },
        (State::Conflict, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "≠",
            zh_label: "信号冲突",
            label_key: "state.conflict",
            code: state.as_str(),
            fg: Color32::from_rgb(251, 146, 60),
            bg: Color32::from_rgb(54, 31, 13),
            border: Color32::from_rgb(249, 115, 22),
            is_green: false,
        },
        (State::Conflict, ThemeMode::Light) => StateBadge {
            state,
            symbol: "≠",
            zh_label: "信号冲突",
            label_key: "state.conflict",
            code: state.as_str(),
            fg: Color32::from_rgb(194, 65, 12),
            bg: Color32::from_rgb(255, 237, 213),
            border: Color32::from_rgb(234, 88, 12),
            is_green: false,
        },
        (State::Unknown, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "?",
            zh_label: "未知",
            label_key: "state.unknown",
            code: state.as_str(),
            fg: Color32::from_rgb(168, 184, 205),
            bg: Color32::from_rgb(30, 41, 59),
            border: Color32::from_rgb(100, 116, 139),
            is_green: false,
        },
        (State::Unknown, ThemeMode::Light) => StateBadge {
            state,
            symbol: "?",
            zh_label: "未知",
            label_key: "state.unknown",
            code: state.as_str(),
            fg: Color32::from_rgb(71, 85, 105),
            bg: Color32::from_rgb(241, 245, 249),
            border: Color32::from_rgb(148, 163, 184),
            is_green: false,
        },
        (State::SignalStale, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "↻",
            zh_label: "规则失效",
            label_key: "state.signal_stale",
            code: state.as_str(),
            fg: Color32::from_rgb(234, 179, 8),
            bg: Color32::from_rgb(48, 38, 10),
            border: Color32::from_rgb(202, 138, 4),
            is_green: false,
        },
        (State::SignalStale, ThemeMode::Light) => StateBadge {
            state,
            symbol: "↻",
            zh_label: "规则失效",
            label_key: "state.signal_stale",
            code: state.as_str(),
            fg: Color32::from_rgb(161, 98, 7),
            bg: Color32::from_rgb(254, 249, 195),
            border: Color32::from_rgb(202, 138, 4),
            is_green: false,
        },
        (State::Unsupported, ThemeMode::Dark) => StateBadge {
            state,
            symbol: "—",
            zh_label: "不支持",
            label_key: "state.unsupported",
            code: state.as_str(),
            fg: Color32::from_rgb(180, 155, 255),
            bg: Color32::from_rgb(35, 26, 61),
            border: Color32::from_rgb(139, 92, 246),
            is_green: false,
        },
        (State::Unsupported, ThemeMode::Light) => StateBadge {
            state,
            symbol: "—",
            zh_label: "不支持",
            label_key: "state.unsupported",
            code: state.as_str(),
            fg: Color32::from_rgb(109, 40, 217),
            bg: Color32::from_rgb(237, 233, 254),
            border: Color32::from_rgb(124, 58, 237),
            is_green: false,
        },
    }
}

/// 七个归因层的穷举中文名称与枚举标识。
pub fn layer_label(layer: Layer) -> (&'static str, &'static str) {
    match layer {
        Layer::EgressNetwork => ("出口网络", "EGRESS_NETWORK"),
        Layer::ServicePolicy => ("服务策略", "SERVICE_POLICY"),
        Layer::Account => ("账号归属", "ACCOUNT"),
        Layer::Payments => ("付款资料", "PAYMENTS"),
        Layer::Preference => ("偏好/版次", "PREFERENCE"),
        Layer::Device => ("设备定位", "DEVICE"),
        Layer::Cloud => ("云资源区域", "CLOUD"),
    }
}

/// 七个归因层的按语言本地化名称（复用核心 `layer.*` 语言包 key，枚举码保持不变）。
pub fn layer_label_localized(layer: Layer, locale: Locale) -> (String, &'static str) {
    let (key, code) = match layer {
        Layer::EgressNetwork => ("layer.egress_network", "EGRESS_NETWORK"),
        Layer::ServicePolicy => ("layer.service_policy", "SERVICE_POLICY"),
        Layer::Account => ("layer.account", "ACCOUNT"),
        Layer::Payments => ("layer.payments", "PAYMENTS"),
        Layer::Preference => ("layer.preference", "PREFERENCE"),
        Layer::Device => ("layer.device", "DEVICE"),
        Layer::Cloud => ("layer.cloud", "CLOUD"),
    };
    (translate(locale, key, &[]), code)
}

/// Evidence 层的标签（包含 `TRANSPORT` 伪层）。
pub fn layer_ref_label(layer: LayerRef) -> &'static str {
    layer_ref_label_plain(layer, true)
}

/// Evidence 层的可读标签；`show_tech = false` 时隐藏英文机器码。
pub fn layer_ref_label_plain(layer: LayerRef, show_tech: bool) -> &'static str {
    match layer {
        LayerRef::Layer(inner) => layer_label(inner).0,
        LayerRef::Transport(_) => {
            if show_tech {
                "传输层 (TRANSPORT)"
            } else {
                "传输连接"
            }
        }
    }
}

/// 可信度穷举标签。
pub fn confidence_label(confidence: Confidence) -> &'static str {
    confidence_label_plain(confidence, true)
}

pub fn confidence_label_plain(confidence: Confidence, show_tech: bool) -> &'static str {
    match (confidence, show_tech) {
        (Confidence::High, true) => "高 (HIGH)",
        (Confidence::Medium, true) => "中 (MEDIUM)",
        (Confidence::Low, true) => "低 (LOW)",
        (Confidence::None, true) => "无 (NONE)",
        (Confidence::High, false) => "高",
        (Confidence::Medium, false) => "中",
        (Confidence::Low, false) => "低",
        (Confidence::None, false) => "无",
    }
}

/// 来源类别穷举标签。
pub fn source_label(source: Source) -> &'static str {
    source_label_plain(source, true)
}

pub fn source_label_plain(source: Source, show_tech: bool) -> &'static str {
    match (source, show_tech) {
        (Source::OfficialDoc, true) => "官方文档 (OFFICIAL_DOC)",
        (Source::DirectAnonymousObservation, true) => "匿名直接观测 (DIRECT_ANONYMOUS_OBSERVATION)",
        (Source::CommunityReport, true) => "社区资料 (COMMUNITY_REPORT)",
        (Source::LegacyCode, true) => "历史代码 (LEGACY_CODE)",
        (Source::Inference, true) => "推断 (INFERENCE)",
        (Source::OfficialDoc, false) => "官方文档",
        (Source::DirectAnonymousObservation, false) => "匿名直接观测",
        (Source::CommunityReport, false) => "社区资料",
        (Source::LegacyCode, false) => "历史资料",
        (Source::Inference, false) => "综合推断",
    }
}

/// 新鲜度状态穷举标签。
pub fn freshness_label(status: FreshnessStatus) -> &'static str {
    freshness_label_plain(status, true)
}

pub fn freshness_label_plain(status: FreshnessStatus, show_tech: bool) -> &'static str {
    match (status, show_tech) {
        (FreshnessStatus::Current, true) => "当前有效 (CURRENT)",
        (FreshnessStatus::Missing, true) => "本次缺失 (MISSING)",
        (FreshnessStatus::Stale, true) => "连续失效 (STALE)",
        (FreshnessStatus::Historical, true) => "历史记录 (>7天)",
        (FreshnessStatus::NotTracked, true) => "未跟踪 (NOT_TRACKED)",
        (FreshnessStatus::Current, false) => "当前有效",
        (FreshnessStatus::Missing, false) => "本次缺失",
        (FreshnessStatus::Stale, false) => "连续失效",
        (FreshnessStatus::Historical, false) => "历史记录 (>7天)",
        (FreshnessStatus::NotTracked, false) => "未跟踪",
    }
}

/// 路径绑定状态穷举标签。
pub fn bind_status_label(status: BindStatus) -> &'static str {
    bind_status_label_plain(status, true)
}

pub fn bind_status_label_plain(status: BindStatus, show_tech: bool) -> &'static str {
    match (status, show_tech) {
        (BindStatus::NotRequested, true) => "未请求 (NOT_REQUESTED)",
        (BindStatus::Verified, true) => "已验证 (VERIFIED)",
        (BindStatus::Unsupported, true) => "不支持 (UNSUPPORTED)",
        (BindStatus::Unknown, true) => "未验证 (UNKNOWN)",
        (BindStatus::NotRequested, false) => "未指定",
        (BindStatus::Verified, false) => "已验证",
        (BindStatus::Unsupported, false) => "不支持",
        (BindStatus::Unknown, false) => "未验证",
    }
}

/// 请求地址族穷举标签。
pub fn requested_family_label(family: RequestedFamily) -> &'static str {
    match family {
        RequestedFamily::Auto => "自动双栈",
        RequestedFamily::V4 => "IPv4",
        RequestedFamily::V6 => "IPv6",
    }
}

/// 实见地址族穷举标签。
pub fn observed_family_label(family: Option<ObservedFamily>) -> &'static str {
    match family {
        Some(ObservedFamily::V4) => "IPv4",
        Some(ObservedFamily::V6) => "IPv6",
        None => "未知",
    }
}

/// 运行模式标签。
pub fn mode_label(mode: Mode) -> &'static str {
    mode_label_plain(mode, true)
}

pub fn mode_label_plain(mode: Mode, show_tech: bool) -> &'static str {
    match (mode, show_tech) {
        (Mode::Online, true) => "在线只读 (ONLINE)",
        (Mode::Fixture, true) => "离线模拟 (FIXTURE)",
        (Mode::Online, false) => "在线只读",
        (Mode::Fixture, false) => "离线模拟",
    }
}

/// 运行终止原因标签。
pub fn termination_label(termination: Termination) -> &'static str {
    termination_label_plain(termination, true)
}

pub fn termination_label_plain(termination: Termination, show_tech: bool) -> &'static str {
    match (termination, show_tech) {
        (Termination::Complete, true) => "已完成 (COMPLETE)",
        (Termination::Cancelled, true) => "已取消 (CANCELLED)",
        (Termination::GlobalTimeout, true) => "全局超时 (GLOBAL_TIMEOUT)",
        (Termination::Failed, true) => "运行失败 (FAILED)",
        (Termination::Partial, true) => "部分完成 (PARTIAL)",
        (Termination::Complete, false) => "已完成",
        (Termination::Cancelled, false) => "已取消",
        (Termination::GlobalTimeout, false) => "全局超时",
        (Termination::Failed, false) => "运行失败",
        (Termination::Partial, false) => "部分完成",
    }
}

/// `--expect` 比较结果标签。
pub fn expectation_status_label(status: ExpectationStatus) -> &'static str {
    expectation_status_label_plain(status, true)
}

pub fn expectation_status_label_plain(status: ExpectationStatus, show_tech: bool) -> &'static str {
    match (status, show_tech) {
        (ExpectationStatus::Match, true) => "一致 (MATCH)",
        (ExpectationStatus::Mismatch, true) => "差异提示 (MISMATCH)",
        (ExpectationStatus::NotObservable, true) => "不可观测 (NOT_OBSERVABLE)",
        (ExpectationStatus::Match, false) => "一致",
        (ExpectationStatus::Mismatch, false) => "存在差异",
        (ExpectationStatus::NotObservable, false) => "暂不可观测",
    }
}

/// 退出码语义说明（严格对齐 ADR-003）。
pub fn exit_code_label(code: u8) -> (&'static str, &'static str) {
    exit_code_label_plain(code, false)
}

pub fn exit_code_label_plain(code: u8, show_tech: bool) -> (&'static str, &'static str) {
    match (code, show_tech) {
        (0, _) => ("结果正常", "全部已测目标处于可接受状态（仍须核对未覆盖栏）"),
        (1, _) => ("配置或运行错误", "输入被拒绝或运行内部异常"),
        (2, true) => (
            "2 · 明确异常",
            "存在 RESTRICTED / BLOCKED / LOCAL_PATH_ERROR（优先于 3 类披露）",
        ),
        (2, false) => ("发现明确异常", "存在受限、阻断或本机连接路径异常"),
        (3, true) => (
            "3 · 不确定/未完成",
            "存在 UNKNOWN / CONFLICT / SIGNAL_STALE / UNSUPPORTED / 未覆盖选中项或已取消",
        ),
        (3, false) => (
            "部分未完成或待确认",
            "存在未知项、信号冲突、规则失效、未覆盖项或已取消",
        ),
        (_, _) => ("未知状态", "非标准退出码"),
    }
}

/// 家族中文名称映射（Google 12 家族 + 对照 3 家族）。
pub fn family_label(family: &str) -> &'static str {
    match family {
        "identity" => "身份与登录",
        "search" => "搜索",
        "content" => "内容与新闻",
        "workspace" => "Workspace 办公",
        "media" => "媒体与视频",
        "store" => "商店与分发",
        "ai" => "AI 服务",
        "education" => "教育",
        "cloud" => "云平台",
        "developer" => "开发者",
        "static" => "静态资源",
        "api" => "通用 API",
        "cdn" => "CDN 对照",
        "domestic" => "国内对照",
        "social" => "社交与开发对照",
        "user" => "用户自定义导入",
        _ => "其他",
    }
}

/// 应用主题到 egui Context（显式同步所有控件前景/背景，彻底杜绝深色下暗字不可见问题）。
pub fn apply_theme(ctx: &Context, mode: ThemeMode) {
    let p = Palette::for_theme(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };
    visuals.panel_fill = p.canvas;
    visuals.window_fill = p.card;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_rounding = Rounding::same(RADIUS_LG);
    visuals.extreme_bg_color = p.sidebar;
    visuals.faint_bg_color = p.card_alt;
    visuals.override_text_color = Some(p.text_primary);
    visuals.selection.bg_fill = p.accent_soft;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.accent);
    visuals.hyperlink_color = p.accent;

    visuals.widgets.noninteractive.bg_fill = p.card;
    visuals.widgets.noninteractive.weak_bg_fill = p.card_alt;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.noninteractive.rounding = Rounding::same(RADIUS_MD);

    visuals.widgets.inactive.bg_fill = p.card_alt;
    visuals.widgets.inactive.weak_bg_fill = p.card_alt;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.rounding = Rounding::same(RADIUS_MD);

    visuals.widgets.hovered.bg_fill = p.surface_hover;
    visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_hover);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.hovered.rounding = Rounding::same(RADIUS_MD);

    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent_soft;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    // egui 也用 active.fg_stroke 绘制普通 strong 标题。按钮自己的强调前景单独指定。
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.active.rounding = Rounding::same(RADIUS_MD);

    visuals.widgets.open.bg_fill = p.card_alt;
    visuals.widgets.open.weak_bg_fill = p.card_alt;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.open.rounding = Rounding::same(RADIUS_MD);

    ctx.style_mut(|style| {
        style.visuals = visuals;
        // 严格收敛 3 级字号体系 + 等宽数据字体
        for (kind, size) in [
            (egui::TextStyle::Heading, FONT_DISPLAY),
            (egui::TextStyle::Body, FONT_BODY),
            (egui::TextStyle::Button, FONT_BODY),
            (egui::TextStyle::Small, FONT_META),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
        style.spacing.item_spacing = vec2(SPACE_2, SPACE_2);
        style.spacing.button_padding = vec2(SPACE_3, SPACE_2);
        style.spacing.interact_size.y = 32.0;
        style.spacing.scroll.floating = false;
        style.spacing.scroll.foreground_color = false;
        style.spacing.scroll.bar_width = 4.0;
        style.spacing.scroll.bar_inner_margin = SPACE_1;
        style.spacing.scroll.bar_outer_margin = SPACE_1;
    });
}

/// 加载中文字体，避免中文字符显示方块；只读，不落盘、不报错中止。
///
/// 优先使用内嵌的开源 CJK 子集（Android/Web 上没有系统字体路径），再回退系统字体。
pub fn install_cjk_fonts(ctx: &Context) {
    let embedded: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular-subset.ttf");
    // Noto Sans SC 不含 U+21BB（状态徽章 “↻”），单独内嵌一个 OFL 符号子集做字形回退。
    let symbols: &[u8] = include_bytes!("../../../assets/fonts/NotoSansMath-Symbols-subset.ttf");
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk_embedded".to_owned(), FontData::from_static(embedded));
    fonts
        .font_data
        .insert("ui_symbols".to_owned(), FontData::from_static(symbols));
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "cjk_embedded".to_owned());
        family.insert(1, "ui_symbols".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.push("cjk_embedded".to_owned());
        family.push("ui_symbols".to_owned());
    }
    ctx.set_fonts(fonts);

    let candidates: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk_system".to_owned(), FontData::from_owned(bytes));
            fonts
                .font_data
                .insert("cjk_embedded".to_owned(), FontData::from_static(embedded));
            fonts
                .font_data
                .insert("ui_symbols".to_owned(), FontData::from_static(symbols));
            if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                family.insert(0, "cjk_system".to_owned());
                family.insert(1, "cjk_embedded".to_owned());
                family.insert(2, "ui_symbols".to_owned());
            }
            if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                family.push("cjk_system".to_owned());
                family.push("cjk_embedded".to_owned());
                family.push("ui_symbols".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

/// 统一主卡片 Frame（严格遵守 4/8 基准：内边距 16px，圆角 12px）。
pub fn card_frame(palette: Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .rounding(Rounding::same(RADIUS_LG))
        .inner_margin(Margin::same(SPACE_4))
}

/// 次级浅底分组容器（用于卡片内浅底分组/数据表外框，避免“重边框套重边框”）。
pub fn subtle_well_frame(palette: Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.card_alt)
        .stroke(Stroke::new(1.0_f32, palette.border_subtle))
        .rounding(Rounding::same(RADIUS_MD))
        .inner_margin(Margin::same(SPACE_3))
}

/// 带柔和悬停边框动画的卡片容器（纯视觉微动画，不改变数据）。
pub fn animated_card_frame(
    ui: &egui::Ui,
    id: egui::Id,
    hovered: bool,
    palette: Palette,
) -> egui::Frame {
    let t = ui.ctx().animate_bool_with_time(id, hovered, 0.14);
    let border = lerp_color(palette.border, palette.border_hover, t);
    egui::Frame::none()
        .fill(palette.card)
        .stroke(Stroke::new(1.0_f32, border))
        .rounding(Rounding::same(RADIUS_LG))
        .inner_margin(Margin::same(SPACE_4))
}

/// 现代提示/声明卡片（左侧 3px 语义色条 + 柔和浅底，灵感来自现代桌面设计系统 Admonition 层次）。
pub fn render_admonition(
    ui: &mut Ui,
    bg: Color32,
    accent: Color32,
    add_contents: impl FnOnce(&mut Ui),
) {
    let frame_resp = egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, accent.gamma_multiply(0.55)))
        .rounding(Rounding::same(RADIUS_MD))
        .inner_margin(Margin {
            left: SPACE_4,
            right: SPACE_3,
            top: SPACE_3,
            bottom: SPACE_3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
    let rect = frame_resp.response.rect;
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, rect.top() + 5.0),
        egui::pos2(rect.left() + 4.5, rect.bottom() - 5.0),
    );
    ui.painter().rect_filled(bar, Rounding::same(2.0), accent);
}

/// 按钮层级 1：Filled 主操作按钮（全屏仅允许一个主 CTA 使用）。
pub fn btn_filled(ui: &mut Ui, p: Palette, label: &str, enabled: bool) -> egui::Response {
    let (fill, fg) = if enabled {
        (p.accent, p.accent_fg)
    } else {
        (p.card_alt, p.text_muted)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .size(FONT_BODY)
                .color(fg),
        )
        .fill(fill)
        .stroke(if enabled {
            Stroke::NONE
        } else {
            Stroke::new(1.0_f32, p.border)
        })
        .rounding(Rounding::same(RADIUS_MD))
        .min_size(vec2(108.0, 36.0)),
    )
}

/// 按钮层级 2：Tonal 强调次操作按钮（用于高优先二级操作）。
pub fn btn_tonal(ui: &mut Ui, p: Palette, label: &str, enabled: bool) -> egui::Response {
    let (fill, stroke_col, fg) = if enabled {
        (p.accent_soft, p.accent.gamma_multiply(0.75), p.accent)
    } else {
        (p.card_alt, p.border_subtle, p.text_muted)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .size(FONT_BODY)
                .color(fg),
        )
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, stroke_col))
        .rounding(Rounding::same(RADIUS_MD))
        .min_size(vec2(0.0, 32.0)),
    )
}

/// 按钮层级 3：Outlined 常规次操作按钮（用于辅助功能/刷新/取消）。
pub fn btn_outlined(ui: &mut Ui, p: Palette, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .size(FONT_BODY)
                .color(if enabled {
                    p.text_primary
                } else {
                    p.text_muted
                }),
        )
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.border))
        .rounding(Rounding::same(RADIUS_MD))
        .min_size(vec2(0.0, 32.0)),
    )
}

/// 危险/清理类 Tonal 按钮（清除/撤销操作）。
pub fn btn_danger_tonal(ui: &mut Ui, p: Palette, label: &str, enabled: bool) -> egui::Response {
    let (fill, stroke_col, fg) = if enabled {
        (p.danger_bg, p.danger_fg.gamma_multiply(0.7), p.danger_fg)
    } else {
        (p.card_alt, p.border_subtle, p.text_muted)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(label).size(FONT_BODY).color(fg))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, stroke_col))
            .rounding(Rounding::same(RADIUS_MD))
            .min_size(vec2(0.0, 32.0)),
    )
}

/// 严格按 4/8 间距（默认 16px）排布的等宽多列布局（取代默认 8px 挤贴的 `ui.columns`）。
pub fn spaced_columns(
    ui: &mut Ui,
    count: usize,
    gap: f32,
    mut draw_col: impl FnMut(&mut Ui, usize),
) {
    if count == 0 {
        return;
    }
    if count == 1 {
        draw_col(ui, 0);
        return;
    }
    let avail = ui.available_width();
    let col_w = ((avail - gap * (count as f32 - 1.0)) / (count as f32)).max(60.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for i in 0..count {
            ui.allocate_ui_with_layout(
                vec2(col_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(col_w);
                    ui.set_max_width(col_w);
                    draw_col(ui, i);
                },
            );
        }
    });
}

/// 结构化键值对行（参考 gpui-kit DescriptionList：左定宽次色标签，右高对比值）。
pub fn kv_row(ui: &mut Ui, p: Palette, label: &str, label_w: f32, value: &str, mono: bool) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = SPACE_2;
        ui.allocate_ui_with_layout(
            vec2(label_w, 18.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(FONT_META)
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
            },
        );
        let text = if mono {
            egui::RichText::new(value)
                .size(FONT_META)
                .monospace()
                .color(p.text_primary)
        } else {
            egui::RichText::new(value)
                .size(FONT_META)
                .color(p.text_primary)
        };
        ui.add(egui::Label::new(text).wrap());
    });
}

/// 模态弹窗半透明背景遮罩（标准 egui Area 遮罩层，避免弹窗与底页混为一团）。
pub fn render_modal_scrim(ctx: &Context, id: &str, p: Palette) {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, Rounding::ZERO, p.scrim);
        });
}

/// 线性插值颜色，用于边框与微动效过渡。
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { ((x as f32) + ((y as f32) - (x as f32)) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// 绘制运行中扫描进度微动画条（仅 `active == true` 时请求定时重绘，空闲零 CPU 消耗）。
pub fn render_running_pulse_bar(ui: &mut egui::Ui, active: bool, palette: Palette) {
    if !active {
        return;
    }
    let width = ui.available_width().max(60.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 4.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let time = ui.ctx().input(|i| i.time) as f32;
        let phase = (time * 0.85).fract();
        let seg_w = (width * 0.28).clamp(48.0, 180.0);
        let x0 = rect.left() + (width + seg_w) * phase - seg_w;
        let seg = egui::Rect::from_min_max(
            egui::pos2(x0.max(rect.left()), rect.top()),
            egui::pos2((x0 + seg_w).min(rect.right()), rect.bottom()),
        );
        ui.painter()
            .rect_filled(rect, Rounding::same(2.0), palette.border);
        if seg.width() > 0.0 {
            ui.painter()
                .rect_filled(seg, Rounding::same(2.0), palette.accent);
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

/// 绘制带平滑缓动动画的比例进度条（用于覆盖比例与单目标耗时分布）。
pub fn render_animated_bar(
    ui: &mut egui::Ui,
    id: egui::Id,
    target_ratio: f32,
    height: f32,
    fill_color: Color32,
    track_color: Color32,
) {
    let clamped = target_ratio.clamp(0.0, 1.0);
    let anim = ui.ctx().animate_value_with_time(id, clamped, 0.28);
    let width = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let rounding = Rounding::same(height * 0.5);
        ui.painter().rect_filled(rect, rounding, track_color);
        let fill_w = (rect.width() * anim).max(if clamped > 0.0 { 4.0 } else { 0.0 });
        if fill_w > 0.0 {
            let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, height));
            ui.painter().rect_filled(fill_rect, rounding, fill_color);
        }
    }
}

/// 绘制强类型 9 态徽章（按当前 `locale` 查 `StateBadge::label(locale)` 渲染，单行不折行）。
pub fn render_state_badge(
    ui: &mut egui::Ui,
    state: State,
    theme: ThemeMode,
    locale: Locale,
    compact: bool,
) {
    let _ = compact;
    let badge = state_badge(state, theme);
    let localized = badge.label(locale);
    // 注意：不在中文胶囊文本中硬插易变豆腐块的 U+2713，直接由语义色 + 图标/文字表达（仅 State::Ok 为绿）。
    let prefix = match state {
        State::Ok => "",
        State::ReachableOnly => "• ",
        State::Restricted => "! ",
        State::Blocked => "× ",
        State::LocalPathError => "! ",
        State::Conflict => "≠ ",
        State::Unknown => "? ",
        State::SignalStale => "↻ ",
        State::Unsupported => "— ",
    };
    // 状态码（`OK` / `BLOCKED` 等）属于机器码：默认视图只显示本地化语义，
    // 技术细节模式才在其它位置单独展示。
    let text = format!("{prefix}{localized}");
    egui::Frame::none()
        .fill(badge.bg)
        .stroke(Stroke::new(1.0_f32, badge.border.gamma_multiply(0.8)))
        .rounding(Rounding::same(RADIUS_PILL))
        .inner_margin(Margin::symmetric(SPACE_2, SPACE_1))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .color(badge.fg)
                        .strong()
                        .size(FONT_META),
                )
                .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
}

/// 紧凑胶囊标签（通用状态/标记用；优先单行绘制，剩余宽度不足时自动折行，绝不溢出视口）。
pub fn render_pill(
    ui: &mut egui::Ui,
    text: &str,
    fg: Color32,
    bg: Color32,
    border: Color32,
) -> egui::Response {
    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border.gamma_multiply(0.75)))
        .rounding(Rounding::same(RADIUS_PILL))
        .inner_margin(Margin::symmetric(SPACE_2, SPACE_1))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(fg).strong().size(FONT_META))
                    .wrap(),
            );
        })
        .response
}
