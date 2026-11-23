//! `reports`：由同一份终态投影出 JSON/JSONL/表格/CSV/HTML，并归约退出码。
//!
//! 不重新求 state；所有格式共用脱敏与转义。

use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};

use crate::contracts::{ExpectationStatus, Layer, Mode, Report, ResultRecord, State, Termination};
use crate::i18n::{translate, Locale};
use crate::targets::Registry;

/// 退出码归约：1 → 2 → 3 → 0，绝不按数值取最大值。
pub fn aggregate_report(report: &Report) -> u8 {
    if report.termination == Termination::Failed {
        return 1;
    }
    if report
        .results
        .iter()
        .any(|result| result.state.is_definite_anomaly())
    {
        return 2;
    }
    let expectation_unsettled = report.results.iter().any(|result| {
        result
            .expectation
            .as_ref()
            .map(|expectation| expectation.status == ExpectationStatus::NotObservable)
            .unwrap_or(false)
    });
    let has_uncovered_selected = report.coverage.uncovered.iter().any(|item| item.selected);
    let cancelled_or_partial = matches!(
        report.termination,
        Termination::Cancelled | Termination::GlobalTimeout | Termination::Partial
    );
    let empty_check =
        report.plans.is_empty() && report.results.is_empty() && report.coverage.selected_count == 0;
    if empty_check
        || report.plans.iter().any(|plan| preview_exit_code(plan) != 0)
        || expectation_unsettled
        || has_uncovered_selected
        || cancelled_or_partial
        || report
            .results
            .iter()
            .any(|result| result.state.is_uncertain())
    {
        return 3;
    }
    0
}

/// 预览请求成功送达不等于产生可用计划（ADR-005 D-4）。
pub fn preview_exit_code(plan: &crate::contracts::GuardPlan) -> u8 {
    if plan.safe_to_apply {
        0
    } else {
        3
    }
}

/// 用法/配置/内部错误固定为 1。
pub fn usage_error_code() -> u8 {
    1
}

/// 单报告 JSON（stdout 只输出这一份）。
pub fn to_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string())
}

/// 一行一个事件对象。
pub fn to_jsonl(events: &[crate::contracts::Event]) -> String {
    let mut out = String::new();
    for event in events {
        if let Ok(text) = serde_json::to_string(event) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

/// 防公式注入的 CSV 单元格。
fn csv_cell(value: &str) -> String {
    let escaped = value.replace('"', "\"\"");
    let dangerous = value.starts_with(['\t', '\r', '\n'])
        || value.trim_start().starts_with(['=', '+', '-', '@']);
    // 中和前缀是字段内容，不是 RFC 4180 引号外的字符。
    format!("\"{}{escaped}\"", if dangerous { "'" } else { "" })
}

/// CSV 表格导出（不改变判定）。
pub fn to_csv(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("target,state,reason,confidence,source,observed_country,region_layers,path,duration_ms,request_count,covered,notes\n");
    for result in &report.results {
        let layers = result
            .region_layers
            .iter()
            .filter_map(|layer| {
                layer
                    .value
                    .as_ref()
                    .map(|value| format!("{}={value}", layer_name(layer)))
            })
            .collect::<Vec<_>>()
            .join(";");
        let notes_text = if let Some(origin) = &result.origin {
            format!(
                "{};origin={};consent_version={};imported_at={};rule_digest={}",
                result.local_policy.reason.as_deref().unwrap_or(""),
                origin,
                result.consent_version.as_deref().unwrap_or(""),
                result.imported_at.as_deref().unwrap_or(""),
                result.rule_digest.as_deref().unwrap_or("")
            )
        } else {
            result
                .local_policy
                .reason
                .as_deref()
                .unwrap_or("")
                .to_string()
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_cell(&result.target),
            csv_cell(result.state.as_str()),
            csv_cell(result.reason.as_deref().unwrap_or("")),
            csv_cell(&confidence_name(result)),
            csv_cell(
                &format!("{:?}", result.source)
                    .replace("Some(", "")
                    .replace(')', "")
            ),
            csv_cell(
                result
                    .service_observed
                    .observed_country
                    .as_deref()
                    .unwrap_or("")
            ),
            csv_cell(&layers),
            csv_cell(&result.path.path_id),
            result.duration_ms,
            result.request_count,
            report
                .coverage
                .covered
                .iter()
                .any(|item| item.target == result.target),
            csv_cell(&notes_text),
        ));
    }
    out
}

fn layer_name(layer: &crate::contracts::LayerValue) -> String {
    format!("{:?}", layer.layer).to_ascii_uppercase()
}

fn confidence_name(result: &ResultRecord) -> String {
    format!("{:?}", result.confidence).to_ascii_uppercase()
}

/// 十列展示行：表格 / Markdown / 纯文本共用同一列序与数据来源。
fn display_rows(report: &Report, locale: Locale) -> Vec<[String; 10]> {
    let mut rows: Vec<[String; 10]> = Vec::new();
    for result in &report.results {
        let layers = result
            .region_layers
            .iter()
            .filter_map(|layer| {
                layer
                    .value
                    .as_ref()
                    .map(|value| format!("{}={value}", layer_name_locale(locale, layer.layer)))
            })
            .collect::<Vec<_>>()
            .join("; ");
        rows.push([
            result.target.clone(),
            result.state.as_str().to_string(),
            if layers.is_empty() {
                "—".to_string()
            } else {
                layers
            },
            result
                .service_observed
                .observed_country
                .clone()
                .unwrap_or_else(|| "—".to_string()),
            result
                .source
                .map(|source| format!("{source:?}"))
                .unwrap_or_else(|| "—".to_string()),
            format!("{:?}", result.confidence).to_ascii_uppercase(),
            result.path.path_id.clone(),
            format!("{} ms", result.duration_ms),
            result.request_count.to_string(),
            result.reason.clone().unwrap_or_else(|| "—".to_string()),
        ]);
    }
    if report.results.is_empty() {
        let empty_notes = translate(
            locale,
            "report.empty_notes",
            &[
                &report.coverage.selected_count.to_string(),
                &report.coverage.uncovered.len().to_string(),
            ],
        );
        rows.push([
            translate(locale, "report.empty", &[]),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "0".to_string(),
            empty_notes,
        ]);
    }
    rows
}

fn column_headers(locale: Locale) -> [String; 10] {
    [
        translate(locale, "report.col.target", &[]),
        translate(locale, "report.col.state", &[]),
        translate(locale, "report.col.layers", &[]),
        translate(locale, "report.col.observed", &[]),
        translate(locale, "report.col.source", &[]),
        translate(locale, "report.col.confidence", &[]),
        translate(locale, "report.col.path", &[]),
        translate(locale, "report.col.duration", &[]),
        translate(locale, "report.col.requests", &[]),
        translate(locale, "report.col.notes", &[]),
    ]
}

/// 人读表格：目标|状态|地区层次|观察值|信号来源|可信度|路径|耗时|请求数|备注。
pub fn to_table(report: &Report) -> String {
    to_table_locale(report, Locale::ZhCn)
}

/// 按 locale 渲染人读表格；JSON/HTML/CSV 的机器字段不受 locale 影响。
pub fn to_table_locale(report: &Report, locale: Locale) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(column_headers(locale).to_vec());
    for row in display_rows(report, locale) {
        table.add_row(row.to_vec());
    }
    let mut text = table.to_string();
    let catalog = report.coverage.catalog_size.to_string();
    let selected = report.coverage.selected_count.to_string();
    let covered = report.coverage.covered.len().to_string();
    let uncovered = report.coverage.uncovered.len().to_string();
    let requests = report.network_request_count.to_string();
    let exit_code = report.exit_code.to_string();
    text.push_str(&format!(
        "\n\n{}\n",
        translate(
            locale,
            "report.coverage",
            &[&catalog, &selected, &covered, &uncovered, &requests, &exit_code,]
        )
    ));
    // E-2：人读输出追加一行克制的资格说明，不参与判定。
    text.push_str(&format!(
        "{}\n",
        translate(locale, "report.eligibility_disclaimer", &[])
    ));
    text
}

/// Markdown 单元格：转义 `|`、把换行压成空格、空值写 `—`。
fn md_cell(value: &str) -> String {
    let flattened = value
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if flattened.is_empty() {
        "—".to_string()
    } else {
        flattened
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Online => "ONLINE",
        Mode::Fixture => "FIXTURE",
    }
}

fn percent_ascii(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// Markdown 结果报告：标题 + 摘要 + 10 列标准表格 + 覆盖两栏 + Guard 摘要 + 脚注。
///
/// 与 [`to_table_locale`] 共用同一列集合与 i18n key；机器码列不本地化。
pub fn to_markdown(report: &Report, locale: Locale) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {}\n\n",
        translate(
            locale,
            "report.heading",
            &[&report.run_id, mode_label(report.mode)]
        )
    ));
    out.push_str(&format!(
        "- {}\n",
        translate(locale, "report.meta.run", &[&report.run_id])
    ));
    out.push_str(&format!(
        "- {}\n",
        translate(locale, "report.meta.mode", &[mode_label(report.mode)])
    ));
    out.push_str(&format!(
        "- {}\n",
        translate(locale, "report.meta.generated", &[&report.generated_at])
    ));
    out.push_str(&format!(
        "- {}\n",
        translate(
            locale,
            "report.meta.exit",
            &[&report.exit_code.to_string(), report.termination.as_str()]
        )
    ));
    out.push_str(&format!(
        "- {}\n",
        translate(
            locale,
            "report.meta.counts",
            &[
                &report.coverage.catalog_size.to_string(),
                &report.coverage.selected_count.to_string(),
                &report.coverage.covered.len().to_string(),
                &report.coverage.uncovered.len().to_string(),
            ]
        )
    ));
    out.push_str(&format!(
        "- {}\n\n",
        translate(
            locale,
            "report.meta.requests",
            &[
                &report.network_request_count.to_string(),
                &report.simulated_request_count.to_string(),
                &report.wall_duration_ms.to_string(),
            ]
        )
    ));

    out.push_str(&format!(
        "## {}\n\n",
        translate(locale, "report.section.results", &[])
    ));
    let headers = column_headers(locale);
    out.push_str("| ");
    out.push_str(&headers.join(" | "));
    out.push_str(" |\n|");
    for _ in 0..headers.len() {
        out.push_str("---|");
    }
    out.push('\n');
    for row in display_rows(report, locale) {
        out.push_str("| ");
        let cells: Vec<String> = row.iter().map(|cell| md_cell(cell)).collect();
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }
    out.push('\n');

    out.push_str(&format!(
        "## {}\n\n### {}\n\n",
        translate(locale, "report.section.coverage", &[]),
        translate(locale, "report.section.covered", &[])
    ));
    if report.coverage.covered.is_empty() {
        out.push_str(&format!("- {}\n", translate(locale, "report.none", &[])));
    } else {
        for item in &report.coverage.covered {
            out.push_str(&format!(
                "- {}\n",
                translate(
                    locale,
                    "report.covered_item",
                    &[
                        &item.target,
                        &item.request_count.to_string(),
                        percent_ascii(item.http_received)
                    ]
                )
            ));
        }
    }
    out.push_str(&format!(
        "\n### {}\n\n",
        translate(locale, "report.section.uncovered", &[])
    ));
    if report.coverage.uncovered.is_empty() {
        out.push_str(&format!("- {}\n", translate(locale, "report.none", &[])));
    } else {
        for item in &report.coverage.uncovered {
            out.push_str(&format!(
                "- {}\n",
                translate(
                    locale,
                    "report.uncovered_item",
                    &[
                        &item.target,
                        if item.selected {
                            translate(locale, "report.selected", &[])
                        } else {
                            translate(locale, "report.not_selected", &[])
                        }
                        .as_str(),
                        &item.reason
                    ]
                )
            ));
        }
    }

    if !report.plans.is_empty() {
        out.push_str(&format!(
            "\n## {}\n\n",
            translate(locale, "report.section.plans", &[])
        ));
        for plan in &report.plans {
            out.push_str(&format!(
                "- {}\n",
                translate(
                    locale,
                    "report.plan_item",
                    &[
                        &plan.plan_id,
                        if plan.candidates.is_empty() {
                            translate(locale, "report.none", &[])
                        } else {
                            plan.candidates.join(", ")
                        }
                        .as_str(),
                        if plan.safe_to_apply { "true" } else { "false" },
                        if plan.reasons.is_empty() {
                            "—".to_string()
                        } else {
                            plan.reasons.join("; ")
                        }
                        .as_str(),
                    ]
                )
            ));
            for limitation in &plan.limitations {
                out.push_str(&format!("  - {limitation}\n"));
            }
        }
    }

    out.push_str(&format!(
        "\n## {}\n\n{}\n",
        translate(locale, "report.section.notes", &[]),
        translate(locale, "report.eligibility_disclaimer", &[])
    ));
    out
}

/// 终端显示宽度：CJK 全角字符按 2 列计。
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            let wide = matches!(
                code,
                0x1100..=0x115F
                    | 0x2E80..=0xA4CF
                    | 0xAC00..=0xD7A3
                    | 0xF900..=0xFAFF
                    | 0xFE30..=0xFE4F
                    | 0xFF00..=0xFF60
                    | 0xFFE0..=0xFFE6
                    | 0x1F300..=0x1FAFF
                    | 0x20000..=0x3FFFD
            );
            if wide {
                2
            } else {
                1
            }
        })
        .sum()
}

fn pad_display(text: &str, width: usize) -> String {
    let used = display_width(text);
    let mut out = text.to_string();
    for _ in used..width {
        out.push(' ');
    }
    out
}

/// 纯文本单元格：换行压成空格，避免破坏行对齐。
fn flat_cell(value: &str) -> String {
    value
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 纯文本结果报告：等宽对齐表 + 同样的摘要块与覆盖两栏，无 ANSI、LF 换行。
pub fn to_text(report: &Report, locale: Locale) -> String {
    let mut out = String::new();
    out.push_str(&translate(
        locale,
        "report.heading",
        &[&report.run_id, mode_label(report.mode)],
    ));
    out.push_str("\n\n");
    out.push_str(&translate(locale, "report.meta.run", &[&report.run_id]));
    out.push('\n');
    out.push_str(&translate(
        locale,
        "report.meta.mode",
        &[mode_label(report.mode)],
    ));
    out.push('\n');
    out.push_str(&translate(
        locale,
        "report.meta.generated",
        &[&report.generated_at],
    ));
    out.push('\n');
    out.push_str(&translate(
        locale,
        "report.meta.exit",
        &[&report.exit_code.to_string(), report.termination.as_str()],
    ));
    out.push('\n');
    out.push_str(&translate(
        locale,
        "report.meta.counts",
        &[
            &report.coverage.catalog_size.to_string(),
            &report.coverage.selected_count.to_string(),
            &report.coverage.covered.len().to_string(),
            &report.coverage.uncovered.len().to_string(),
        ],
    ));
    out.push('\n');
    out.push_str(&translate(
        locale,
        "report.meta.requests",
        &[
            &report.network_request_count.to_string(),
            &report.simulated_request_count.to_string(),
            &report.wall_duration_ms.to_string(),
        ],
    ));
    out.push_str("\n\n");

    let headers = column_headers(locale);
    let rows: Vec<[String; 10]> = display_rows(report, locale)
        .iter()
        .map(|row| std::array::from_fn(|index| flat_cell(&row[index])))
        .collect();
    let mut widths: Vec<usize> = headers.iter().map(|header| display_width(header)).collect();
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }
    let render_row = |cells: &[String], out: &mut String| {
        for (index, cell) in cells.iter().enumerate() {
            if index > 0 {
                out.push_str("  ");
            }
            out.push_str(&pad_display(cell, widths[index]));
        }
        out.push('\n');
    };
    render_row(&headers, &mut out);
    let rule: String = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ");
    out.push_str(&rule);
    out.push('\n');
    for row in &rows {
        render_row(row, &mut out);
    }

    out.push('\n');
    out.push_str(&translate(locale, "report.section.coverage", &[]));
    out.push('\n');
    out.push_str(&format!(
        "  {}\n",
        translate(locale, "report.section.covered", &[])
    ));
    if report.coverage.covered.is_empty() {
        out.push_str(&format!("    {}\n", translate(locale, "report.none", &[])));
    } else {
        for item in &report.coverage.covered {
            out.push_str(&format!(
                "    {}\n",
                translate(
                    locale,
                    "report.covered_item",
                    &[
                        &item.target,
                        &item.request_count.to_string(),
                        percent_ascii(item.http_received)
                    ]
                )
            ));
        }
    }
    out.push_str(&format!(
        "  {}\n",
        translate(locale, "report.section.uncovered", &[])
    ));
    if report.coverage.uncovered.is_empty() {
        out.push_str(&format!("    {}\n", translate(locale, "report.none", &[])));
    } else {
        for item in &report.coverage.uncovered {
            out.push_str(&format!(
                "    {}\n",
                translate(
                    locale,
                    "report.uncovered_item",
                    &[
                        &item.target,
                        if item.selected {
                            translate(locale, "report.selected", &[])
                        } else {
                            translate(locale, "report.not_selected", &[])
                        }
                        .as_str(),
                        &item.reason
                    ]
                )
            ));
        }
    }

    if !report.plans.is_empty() {
        out.push('\n');
        out.push_str(&translate(locale, "report.section.plans", &[]));
        out.push('\n');
        for plan in &report.plans {
            out.push_str(&format!(
                "  {}\n",
                translate(
                    locale,
                    "report.plan_item",
                    &[
                        &plan.plan_id,
                        if plan.candidates.is_empty() {
                            translate(locale, "report.none", &[])
                        } else {
                            plan.candidates.join(", ")
                        }
                        .as_str(),
                        if plan.safe_to_apply { "true" } else { "false" },
                        if plan.reasons.is_empty() {
                            "—".to_string()
                        } else {
                            plan.reasons.join("; ")
                        }
                        .as_str(),
                    ]
                )
            ));
            for limitation in &plan.limitations {
                out.push_str(&format!("    - {limitation}\n"));
            }
        }
    }

    out.push('\n');
    out.push_str(&translate(locale, "report.section.notes", &[]));
    out.push('\n');
    out.push_str(&translate(locale, "report.eligibility_disclaimer", &[]));
    out.push('\n');
    out
}

fn layer_name_locale(locale: Locale, layer: Layer) -> String {
    let key = match layer {
        Layer::EgressNetwork => "layer.egress_network",
        Layer::ServicePolicy => "layer.service_policy",
        Layer::Account => "layer.account",
        Layer::Payments => "layer.payments",
        Layer::Preference => "layer.preference",
        Layer::Device => "layer.device",
        Layer::Cloud => "layer.cloud",
    };
    translate(locale, key, &[])
}

/// HTML 转义。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 单文件离线 HTML（内联 CSS/JS，不引用 CDN）。
pub fn to_html(report: &Report) -> String {
    let mut rows = String::new();
    for result in &report.results {
        let layers = result
            .region_layers
            .iter()
            .filter_map(|layer| {
                layer
                    .value
                    .as_ref()
                    .map(|value| format!("{}={value}", layer_name(layer)))
            })
            .collect::<Vec<_>>()
            .join(" · ");
        let conflicts = if result.conflicts.is_empty() {
            String::new()
        } else {
            format!(
                "<div class=\"conflict\">冲突：{}</div>",
                escape(
                    &result
                        .conflicts
                        .iter()
                        .map(|conflict| conflict.claim.clone())
                        .collect::<Vec<_>>()
                        .join("、")
                )
            )
        };
        let audit_html = if let Some(origin) = &result.origin {
            format!(
                "<div class=\"meta\">origin={} · consent={} · imported_at={} · rule_digest={}</div>",
                escape(origin),
                escape(result.consent_version.as_deref().unwrap_or("—")),
                escape(result.imported_at.as_deref().unwrap_or("—")),
                escape(result.rule_digest.as_deref().unwrap_or("—")),
            )
        } else {
            String::new()
        };
        rows.push_str(&format!(
            "<tr><td>{}</td><td><span class=\"state s-{}\">{}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{} ms</td><td>{}</td><td>{}{}{}</td></tr>\n",
            escape(&result.target),
            state_class(result.state),
            escape(result.state.as_str()),
            escape(&layers),
            escape(result.service_observed.observed_country.as_deref().unwrap_or("—")),
            escape(&format!("{:?}", result.confidence).to_ascii_uppercase()),
            result.duration_ms,
            result.request_count,
            escape(result.reason.as_deref().unwrap_or("")),
            conflicts,
            audit_html
        ));
    }
    let uncovered = report
        .coverage
        .uncovered
        .iter()
        .map(|item| {
            format!(
                "<li>{}{}：{}</li>",
                escape(&item.target),
                if item.selected {
                    "（已选）"
                } else {
                    "（未选）"
                },
                escape(&item.reason)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Parallaxe box 报告 · {run_id}</title>
<style>
:root{{--ink:#1c1c1c;--muted:#5b5b5b;--line:#d9d4c8;--paper:#fbf9f4;--accent:#b4441f;}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--paper);color:var(--ink);font:14px/1.7 "Microsoft YaHei","PingFang SC",sans-serif}}
main{{max-width:1200px;margin:0 auto;padding:32px 20px 64px}}
h1{{font-size:26px;margin:0 0 8px}}
h2{{font-size:18px;margin:32px 0 12px;border-bottom:1px solid var(--line);padding-bottom:6px}}
.meta{{color:var(--muted);font-size:13px}}
table{{width:100%;border-collapse:collapse;font-size:13px;background:#fff}}
th,td{{border-bottom:1px solid var(--line);padding:8px 10px;text-align:left;vertical-align:top}}
th{{background:#f1eee6;position:sticky;top:0}}
.state{{font-family:"Cascadia Code",Consolas,monospace;font-size:12px;padding:1px 6px;border:1px solid var(--line);border-radius:4px}}
.s-ok{{border-color:#2f6b3f;color:#2f6b3f}}
.s-restricted,.s-blocked,.s-local-path-error{{border-color:#a32222;color:#a32222}}
.s-unknown,.s-conflict,.s-signal-stale,.s-unsupported{{border-color:#8a5a00;color:#8a5a00}}
.conflict{{color:#a32222;font-size:12px;margin-top:4px}}
ul{{margin:0;padding-left:20px;color:var(--muted)}}
footer{{margin-top:40px;color:var(--muted);font-size:12px}}
</style>
</head>
<body>
<main>
<h1>Parallaxe box 报告</h1>
<p class="meta">运行 {run_id} · 模式 {mode} · 生成 {generated_at} · 退出码 {exit_code} · 终止 {termination}</p>
<p class="meta">目录 {catalog} / 选中 {selected} / 已覆盖 {covered} / 未覆盖 {uncovered_count} · 网络请求 {requests}</p>
<h2>结果</h2>
<table>
<thead><tr><th>目标</th><th>状态</th><th>地区层次</th><th>观察值</th><th>可信度</th><th>耗时</th><th>请求数</th><th>备注</th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
<h2>本次未覆盖</h2>
<ul>{uncovered}</ul>
<p class="meta">资格提示：访问可达不等于订阅或完整功能可用；地区清单、账号类型、年龄、家庭组、付款档案与网络修改器检测均可能影响资格，本软件不验证账号状态，请核对官方清单。</p>
<footer>本报告只描述本次只读探测的证据；共享出口与远端账号影响不可由本工具证明。报告不包含 Cookie、凭据或完整报文。</footer>
</main>
</body>
</html>
"#,
        run_id = escape(&report.run_id),
        mode = match report.mode {
            Mode::Online => "ONLINE",
            Mode::Fixture => "FIXTURE",
        },
        generated_at = escape(&report.generated_at),
        exit_code = report.exit_code,
        termination = report.termination.as_str(),
        catalog = report.coverage.catalog_size,
        selected = report.coverage.selected_count,
        covered = report.coverage.covered.len(),
        uncovered_count = report.coverage.uncovered.len(),
        requests = report.network_request_count,
        rows = rows,
        uncovered = if uncovered.is_empty() {
            "<li>无</li>".to_string()
        } else {
            uncovered
        }
    )
}

fn state_class(state: State) -> &'static str {
    match state {
        State::Ok => "ok",
        State::Restricted => "restricted",
        State::Blocked => "blocked",
        State::ReachableOnly => "reachable-only",
        State::LocalPathError => "local-path-error",
        State::Conflict => "conflict",
        State::Unknown => "unknown",
        State::SignalStale => "signal-stale",
        State::Unsupported => "unsupported",
    }
}

/// 目录列表（targets list）。
pub fn targets_json(registry: &Registry, selection: &crate::targets::Selection) -> String {
    let selected: std::collections::HashSet<&str> =
        selection.ids.iter().map(String::as_str).collect();
    let now = crate::state::format_time(time::OffsetDateTime::now_utc());
    let items: Vec<serde_json::Value> = registry
        .entries()
        .iter()
        .filter(|entry| selected.contains(entry.id()))
        .map(|entry| {
            let admission = registry.admission(entry, selection.implicit_default, &now);
            serde_json::json!({
                "id": entry.raw.id,
                "family": entry.raw.family,
                "group": entry.raw.group,
                "matrix_group": entry.raw.matrix_group,
                "url": entry.raw.url,
                "surface": entry.raw.surface,
                "protected": entry.raw.protected,
                "verification": entry.raw.verification,
                "allow_online": entry.raw.allow_online,
                "safe_default": entry.raw.safe_default,
                "admission_status": admission.status,
                "reason": admission.reason,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "schema": "parallaxe.targets/v1",
        "catalog_size": registry.catalog_size(),
        "selected_count": items.len(),
        "targets": items,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::csv_cell;
    #[test]
    fn csv_prefix_stays_inside_one_quoted_cell() {
        assert_eq!(
            csv_cell(" =1,\"field\"\r\nnext"),
            "\"' =1,\"\"field\"\"\r\nnext\""
        );
        assert_eq!(csv_cell("\n=1"), "\"'\n=1\"");
        assert_eq!(csv_cell("field,ok"), "\"field,ok\"");
    }
}
