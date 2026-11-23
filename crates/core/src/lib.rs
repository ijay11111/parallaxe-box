//! Parallaxe box 共享只读核心。
//!
//! 判定、证据、报告与事件只在这里生成；CLI、agent 与 GUI 只做投影。
//! 本 crate 不依赖任何 GUI 库，也不提供系统写入能力。

#![forbid(unsafe_code)]

pub mod contracts;
pub mod eligibility;
pub mod events;
pub mod fixtures;
pub mod forensics;
pub mod guard;
pub mod guard_export;
pub mod i18n;
pub mod import_bundle;
pub mod linkcheck;
pub mod network;
pub mod probe;
pub mod reports;
pub mod runtime;
pub mod schedule;
pub mod selftest;
pub mod signals;
pub mod state;
pub mod sysblock;
pub mod targets;
pub mod verdict;

pub use contracts::{
    normalize_country, Agreement, Attempt, AttemptFamily, BindStatus, Confidence, Conflict,
    Coverage, Covered, Event, EventName, Evidence, Expectation, ExpectationStatus, Freshness,
    FreshnessStatus, GuardPlan, Layer, LayerRef, LayerValue, LocalPolicy, Mode, ObservedFamily,
    Path, Report, RequestedFamily, ResultRecord, RiskSignal, ServiceObserved, Source, State,
    Termination, TransportLayerResult, Uncovered, Vote, ACCOUNT_MODE, BROWSER_PROFILE_SOURCES,
    CREDENTIAL_SOURCES, EVENT_SCHEMA, REPORT_SCHEMA, SESSION_MODE, TOOL_NAME,
};
pub use eligibility::{EligibilityCategory, EligibilityNote};
pub use i18n::{translate, Locale};
pub use runtime::{run_check, CheckOptions, RunOutput};
pub use targets::{
    Admission, Budget, ConfigError, LoadPaths, Profile, Registry, Selection, Target, HARD_LIMITS,
};

/// 当前 crate 版本，写入报告。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
