//! T-7-10：Markdown / 纯文本结果导出的列序、i18n、转义、对齐与空报告行为。

use parallaxe_core::contracts::{Covered, Mode, Report, ResultRecord, Uncovered};
use parallaxe_core::{reports, Locale, Path};

fn sample_report() -> Report {
    let mut report = Report::new("0.1.0", "2026-11-23T00:00:00Z", "run-export", Mode::Online);
    report.exit_code = 3;
    report.network_request_count = 2;
    report.simulated_request_count = 0;
    report.wall_duration_ms = 1234;
    report.coverage.catalog_size = 5;
    report.coverage.selected_count = 2;
    report.coverage.covered.push(Covered {
        target: "google.gemini".to_string(),
        request_count: 1,
        http_received: true,
    });
    report.coverage.uncovered.push(Uncovered {
        target: "google.books".to_string(),
        selected: true,
        reason: "NO_REGION_SIGNAL".to_string(),
    });
    let mut record = ResultRecord {
        destination: Default::default(),
        target: "google.gemini|pipe\nnewline".to_string(),
        state: parallaxe_core::State::Unknown,
        reason: None,
        confidence: parallaxe_core::Confidence::Low,
        source: None,
        path: Path::system("path-system"),
        duration_ms: 42,
        request_count: 1,
        evidence: Vec::new(),
        region_layers: parallaxe_core::verdict::all_layers_null(),
        observed: Vec::new(),
        conflicts: Vec::new(),
        agreement: Vec::new(),
        transport: parallaxe_core::TransportLayerResult {
            attempts: Vec::new(),
            path_diagnostic: Some("SYSTEM_PATH".to_string()),
        },
        service_observed: parallaxe_core::ServiceObserved {
            surface: "search".to_string(),
            observed_country: None,
            evidence_ids: Vec::new(),
            reason: Some("NO_REGION_SIGNAL".to_string()),
        },
        risk_signal: parallaxe_core::RiskSignal {
            markers: Vec::new(),
            evidence_ids: Vec::new(),
            reason: None,
        },
        local_policy: parallaxe_core::LocalPolicy {
            protected: false,
            guard_eligible: false,
            plan_id: None,
            reason: None,
        },
        expectation: None,
        origin: None,
        consent_version: None,
        imported_at: None,
        rule_digest: None,
        eligibility_notes: Vec::new(),
    };
    record.region_layers[0].value = Some("JPN".to_string());
    report.results.push(record);
    report
}

/// 统计 Markdown 表格行的“未转义竖线”数量。
fn unescaped_pipes(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut count = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'|' {
            count += 1;
        }
        index += 1;
    }
    count
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            if matches!(
                code,
                0x1100..=0x115F
                    | 0x2E80..=0xA4CF
                    | 0xAC00..=0xD7A3
                    | 0xF900..=0xFAFF
                    | 0xFE30..=0xFE4F
                    | 0xFF00..=0xFF60
                    | 0xFFE0..=0xFFE6
            ) {
                2
            } else {
                1
            }
        })
        .sum()
}

#[test]
fn markdown_has_ten_columns_and_escapes_pipes() {
    let report = sample_report();
    let md = reports::to_markdown(&report, Locale::ZhCn);
    assert!(md.starts_with("# "), "缺一级标题：{md}");
    assert!(md.contains("run-export"));
    assert!(md.contains("| 目标 | 状态 |"), "缺中文表头：{md}");
    let table_lines: Vec<&str> = md
        .lines()
        .skip_while(|line| !line.starts_with("| "))
        .take_while(|line| line.starts_with('|'))
        .collect();
    assert_eq!(table_lines.len(), 3, "表头 + 分隔 + 1 行结果");
    for line in &table_lines {
        assert_eq!(
            unescaped_pipes(line),
            11,
            "每行必须恰好 10 列（11 个边界竖线）：{line}"
        );
    }
    assert!(table_lines[1].contains("---|"));
    assert!(
        md.contains("google.gemini\\|pipe newline"),
        "竖线必须转义、换行压成空格：{md}"
    );
    // 空备注写 —，不留空白单元格。
    assert!(md.contains("| UNKNOWN |"), "机器码保持 token");
    assert!(md.contains("## 覆盖与未覆盖"));
    assert!(md.contains("google.books"));
    assert!(md.contains("能力边界"));
}

#[test]
fn markdown_and_text_share_locale_headers_and_machine_codes() {
    let report = sample_report();
    let zh_md = reports::to_markdown(&report, Locale::ZhCn);
    let en_md = reports::to_markdown(&report, Locale::EnUs);
    assert!(zh_md.contains("| 目标 |"));
    assert!(en_md.contains("| Target |"));
    assert_ne!(zh_md, en_md);
    // 机器码列跨 locale 不变。
    for token in ["UNKNOWN", "NO_REGION_SIGNAL", "path-system", "LOW"] {
        assert!(zh_md.contains(token), "zh 缺少机器码 {token}");
        assert!(en_md.contains(token), "en 缺少机器码 {token}");
    }
    let zh_txt = reports::to_text(&report, Locale::ZhCn);
    let en_txt = reports::to_text(&report, Locale::EnUs);
    assert!(zh_txt.contains("目标"));
    assert!(en_txt.contains("Target"));
    for token in ["UNKNOWN", "path-system", "LOW"] {
        assert!(zh_txt.contains(token));
        assert!(en_txt.contains(token));
    }
    // 不依赖 ANSI 颜色；UTF-8/LF。
    assert!(!zh_txt.contains('\u{1b}'));
    assert!(!zh_txt.contains('\r'));
    assert!(!zh_md.contains('\r'));
}

#[test]
fn text_table_is_display_width_aligned() {
    let report = sample_report();
    let text = reports::to_text(&report, Locale::ZhCn);
    let lines: Vec<&str> = text.lines().collect();
    let header_index = lines
        .iter()
        .position(|line| line.starts_with("目标"))
        .expect("表头行");
    let header_width = display_width(lines[header_index]);
    let rule = lines[header_index + 1];
    assert!(
        rule.chars().all(|ch| ch == '-' || ch == ' '),
        "分隔行只能是横线：{rule}"
    );
    assert_eq!(display_width(rule), header_width);
    let row = lines[header_index + 2];
    assert_eq!(
        display_width(row),
        header_width,
        "中文按 2 宽对齐后行宽必须一致：{row:?}"
    );
}

#[test]
fn empty_report_formats_do_not_panic() {
    let report = Report::new("0.1.0", "2026-11-23T00:00:00Z", "run-empty", Mode::Fixture);
    for locale in Locale::ALL {
        let md = reports::to_markdown(&report, locale);
        assert!(md.contains("run-empty"));
        assert!(md.contains("## "));
        let text = reports::to_text(&report, locale);
        assert!(text.contains("run-empty"));
        assert!(text.contains("—") || text.contains("None") || text.contains("无"));
    }
}
