//! 轻量 i18n：语言包是数据（TOML），机器码与判定契约不参与翻译。
//!
//! 设计要点：
//! 1. 语言包通过 `include_str!` 内嵌，保持单文件分发；
//! 2. 缺失 key 回退到 zh-CN；仍缺失则返回 key 本身，不 panic；
//! 3. `translate` 只处理给人看的文字；JSON/agent 的 State/reason/layer 等机器码不变。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const ZH_CN: &str = "zh-CN";
pub const EN_US: &str = "en-US";

/// 支持的界面语言。默认中文；`--locale` 只影响人类可读输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Locale {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::ZhCn, Locale::EnUs];

    /// 解析 `zh-CN` / `zh` / `en-US` / `en`（大小写不敏感）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "zh-cn" | "zh" => Some(Locale::ZhCn),
            "en-us" | "en" => Some(Locale::EnUs),
            _ => None,
        }
    }

    /// 从命令行预扫描 `--locale`（用于在 clap 解析帮助前决定帮助语言）。
    pub fn detect_from_args<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            let arg = arg.as_ref();
            if let Some(value) = arg.strip_prefix("--locale=") {
                return Locale::parse(value).unwrap_or_default();
            }
            if arg == "--locale" {
                if let Some(value) = args.peek() {
                    return Locale::parse(value.as_ref()).unwrap_or_default();
                }
            }
        }
        Locale::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Locale::ZhCn => ZH_CN,
            Locale::EnUs => EN_US,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Locale::ZhCn => "中文（简体）",
            Locale::EnUs => "English (US)",
        }
    }
}

const ZH_PACK: &str = include_str!("../../../assets/i18n/zh-CN.toml");
const EN_PACK: &str = include_str!("../../../assets/i18n/en-US.toml");

static ZH_MESSAGES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
static EN_MESSAGES: OnceLock<BTreeMap<String, String>> = OnceLock::new();

fn parse_pack(text: &str) -> BTreeMap<String, String> {
    // 语言包损坏时退化为空表：translate 会回退到 zh-CN 或 key，不会 panic。
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return BTreeMap::new();
    };
    value
        .get("messages")
        .and_then(|messages| messages.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pack_for(locale: Locale) -> &'static BTreeMap<String, String> {
    match locale {
        Locale::ZhCn => ZH_MESSAGES.get_or_init(|| parse_pack(ZH_PACK)),
        Locale::EnUs => EN_MESSAGES.get_or_init(|| parse_pack(EN_PACK)),
    }
}

/// 查语言包（不做回退），供测试与启动自检使用。
pub fn has_key(locale: Locale, key: &str) -> bool {
    pack_for(locale).contains_key(key)
}

/// 某语言包的全部 key（测试用于核对 zh/en 对齐）。
pub fn keys(locale: Locale) -> std::collections::BTreeSet<String> {
    pack_for(locale).keys().cloned().collect()
}

/// 翻译一条消息；`{0}`、`{1}` 会被 `args` 顺序替换。
pub fn translate(locale: Locale, key: &str, args: &[&str]) -> String {
    let template = pack_for(locale)
        .get(key)
        .or_else(|| pack_for(Locale::ZhCn).get(key))
        .map(String::as_str)
        .unwrap_or(key);
    interpolate(template, args)
}

fn interpolate(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '{' {
            out.push(ch);
            continue;
        }
        let mut index = String::new();
        let mut closed = false;
        while let Some(&next) = chars.peek() {
            if next == '}' {
                chars.next();
                closed = true;
                break;
            }
            if next.is_ascii_digit() {
                index.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if closed {
            if let Some(value) = index
                .parse::<usize>()
                .ok()
                .and_then(|position| args.get(position))
            {
                out.push_str(value);
                continue;
            }
        }
        out.push('{');
        out.push_str(&index);
        if closed {
            out.push('}');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_parse_and_default() {
        assert_eq!(Locale::default(), Locale::ZhCn);
        assert_eq!(Locale::parse("zh-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::parse("en"), Some(Locale::EnUs));
        assert_eq!(Locale::parse("fr-FR"), None);
        assert_eq!(
            Locale::detect_from_args(["--locale", "en-US", "check"]),
            Locale::EnUs
        );
        assert_eq!(Locale::detect_from_args(["--locale=zh-CN"]), Locale::ZhCn);
    }

    #[test]
    fn placeholders_and_fallback() {
        assert_eq!(translate(Locale::ZhCn, "state.ok", &[]), "正常");
        assert_eq!(translate(Locale::EnUs, "state.ok", &[]), "OK");
        // 只存在于 zh-CN 的探针 key 应回退到中文。
        assert_eq!(
            translate(Locale::EnUs, "internal.zh_only_probe", &[]),
            "中文回退"
        );
        assert_eq!(translate(Locale::ZhCn, "missing.key", &[]), "missing.key");
        assert_eq!(
            translate(Locale::EnUs, "report.empty_notes", &["3", "5"]),
            "Selected 3, uncovered 5"
        );
    }
}
