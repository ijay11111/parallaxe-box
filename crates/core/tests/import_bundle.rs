//! T-5-07 统一导入包（Import Bundle）核心验收测试：
//! 1. 文本文件导入（3 行不同规则绑定 + Markdown 表格解析）；
//! 2. ZIP 导入包（manifest + targets + rules + README，含 STORED 与 DEFLATE）；
//! 3. ZIP 安全防线（路径穿越、非白名单脚本、超 10MiB、DEFLATE 炸弹超 50MiB）；
//! 4. 责任声明分层门槛（含规则未勾规则声明 → 零写入拒绝）；
//! 5. 原子性回滚（包内任一 URL 非法 → 规则与目标整体零写入回滚）；
//! 6. 零发包计数传输器证明（未获 F3 一次性授权前 `network_request_count == 0`）。

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parallaxe_core::contracts::ORIGIN_USER_IMPORTED;
use parallaxe_core::import_bundle::{
    build_stored_zip_bytes, parse_import_text, parse_import_zip_bytes, read_import_text_file,
    read_import_zip, ImportSource, IMPORT_BUNDLE_SCHEMA,
};
use parallaxe_core::probe::{FetchOutcome, FetchRequest, Transport};
use parallaxe_core::runtime::{self, CheckOptions};
use parallaxe_core::targets::Registry;

struct CountingTransport(Arc<AtomicU64>);

impl Transport for CountingTransport {
    fn fetch<'a>(
        &'a self,
        _req: FetchRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = FetchOutcome> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            panic!("未授权用户导入目标严禁触发任何真实传输调用");
        })
    }
}

#[test]
fn text_file_import_binds_distinct_rules_per_url() {
    let mut reg = Registry::embedded().unwrap();
    let txt = r#"
# 我的多规则检查清单
https://edge-a.example.com/cdn-cgi/trace | rule.http,rule.trace | edge-a | 出口 Trace 检查
https://edge-b.example.com/books | rule.http,rule.inline3 | edge-b | 内联地区码检查
https://edge-c.example.com/status | rule.http | edge-c | 仅连通性检查
"#;
    let bundle = parse_import_text(txt, &[], false).unwrap();
    assert_eq!(bundle.schema, IMPORT_BUNDLE_SCHEMA);
    assert_eq!(bundle.source, ImportSource::TextFile);
    assert_eq!(bundle.targets.len(), 3);

    let preview = reg.preview_import_bundle(&bundle);
    assert!(
        preview.all_valid,
        "预览应全部有效：{:?}",
        preview.summary_error
    );
    assert!(preview.requires_target_consent);
    assert!(!preview.requires_rule_consent);

    let report = reg.import_bundle(&bundle, true, false).unwrap();
    assert_eq!(report.imported_targets.len(), 3);
    assert_eq!(report.imported_rules.len(), 0);
    assert_eq!(report.origin, ORIGIN_USER_IMPORTED);

    let a = reg.user_target_meta("user.edge-a").unwrap();
    let b = reg.user_target_meta("user.edge-b").unwrap();
    let c = reg.user_target_meta("user.edge-c").unwrap();
    assert_eq!(a.signals, vec!["rule.http", "rule.trace"]);
    assert_eq!(b.signals, vec!["rule.http", "rule.inline3"]);
    assert_eq!(c.signals, vec!["rule.http"]);

    // 额外验证 Markdown 表格格式从真实临时文件读取
    let md_path = std::env::temp_dir().join(format!(
        "px-import-md-{}.md",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    let md_content = r#"
# 检查表

| URL | 规则 | ID | 表面说明 |
| --- | --- | --- | --- |
| https://md-a.example.com/ | rule.http,rule.type | md-a | MD 表格项 A |
"#;
    std::fs::write(&md_path, md_content).unwrap();
    let md_bundle = read_import_text_file(&md_path, &[], false).unwrap();
    std::fs::remove_file(&md_path).unwrap();
    assert_eq!(md_bundle.targets.len(), 1);
    assert_eq!(md_bundle.targets[0].signals, vec!["rule.http", "rule.type"]);
}

#[test]
fn zip_bundle_imports_rules_targets_and_bindings_atomically() {
    let manifest = r#"{
  "schema": "parallaxe.import/v1",
  "source": "ZIP_BUNDLE",
  "targets": []
}"#;
    let rules_toml = r#"
[[rules]]
id = "rule.bundle-region"
version = 1
enabled = true
layer = "SERVICE_POLICY"
channel = "SERVICE"
credibility = "LOW"
source_grade = "DIRECT_ANONYMOUS_OBSERVATION"
notes = "随 ZIP 包携带的自定义提取规则"
negative_case = "未命中记缺失"
extract = { kind = "regex", pattern = "region=([A-Z]{2})", capture = "1" }
applies_to = []
"#;
    let targets_txt = r#"
# 绑定包内新增规则与内置规则
https://zip-a.example.com/api | rule.bundle-region,rule.http | zip-a | 绑定包内规则
https://zip-b.example.com/trace | rule.trace | zip-b | 绑定内置规则
"#;
    let readme = "# 示例导入包\n仅作只读说明展示，不执行任何脚本。";

    let zip_bytes = build_stored_zip_bytes(&[
        ("my-bundle/manifest.json", manifest.as_bytes()),
        ("my-bundle/rules.toml", rules_toml.as_bytes()),
        ("my-bundle/targets.txt", targets_txt.as_bytes()),
        ("my-bundle/README.md", readme.as_bytes()),
    ]);

    let zip_path = std::env::temp_dir().join(format!(
        "px-import-valid-{}.zip",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    std::fs::write(&zip_path, &zip_bytes).unwrap();
    let bundle = read_import_zip(&zip_path).unwrap();
    std::fs::remove_file(&zip_path).unwrap();

    assert_eq!(bundle.source, ImportSource::ZipBundle);
    assert_eq!(bundle.rules.len(), 1);
    assert_eq!(bundle.targets.len(), 2);
    assert!(bundle.readme.as_deref().unwrap().contains("只读说明展示"));

    let mut reg = Registry::embedded().unwrap();
    let preview = reg.preview_import_bundle(&bundle);
    assert!(preview.all_valid);
    assert!(preview.requires_target_consent);
    assert!(preview.requires_rule_consent);
    // 确认预览标注了 rule.bundle-region 为“包内新增”
    assert!(preview.targets[0]
        .bound_signals
        .iter()
        .any(|(id, is_new)| id == "rule.bundle-region" && *is_new));

    // 声明门槛：只勾网址声明、未勾规则声明 → 零写入拒绝
    let denied = reg.import_bundle(&bundle, true, false);
    assert!(denied.is_err());
    assert!(reg.user_targets().is_empty());
    assert!(reg.user_rules().is_empty());

    // 两项声明都勾选 → 原子导入成功
    let report = reg.import_bundle(&bundle, true, true).unwrap();
    assert_eq!(report.imported_rules.len(), 1);
    assert_eq!(report.imported_targets.len(), 2);
    assert!(reg.is_user_rule("rule.bundle-region"));
    assert_eq!(
        reg.user_target_meta("user.zip-a").unwrap().signals,
        vec!["rule.bundle-region", "rule.http"]
    );
    assert_eq!(
        reg.user_target_meta("user.zip-b").unwrap().signals,
        vec!["rule.trace"]
    );
}

#[test]
fn zip_security_rejects_traversal_disallowed_files_and_oversized_archives() {
    // 1. 路径穿越 ../
    let traversal_zip = build_stored_zip_bytes(&[("../targets.txt", b"https://a.example.com/")]);
    let err = parse_import_zip_bytes(&traversal_zip).unwrap_err();
    assert!(
        err.to_string().contains("目录穿越"),
        "错误应指出目录穿越：{err}"
    );

    // 2. 非白名单脚本文件
    let script_zip = build_stored_zip_bytes(&[
        ("targets.txt", b"https://a.example.com/"),
        ("install.ps1", b"Write-Host bad"),
    ]);
    let err = parse_import_zip_bytes(&script_zip).unwrap_err();
    assert!(
        err.to_string().contains("白名单"),
        "错误应拒绝脚本文件：{err}"
    );

    // 3. DEFLATE 压缩炸弹（解压后超过 50 MiB）
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let zeros = vec![b' '; 1024 * 1024];
    for _ in 0..52 {
        encoder.write_all(&zeros).unwrap();
    }
    let compressed = encoder.finish().unwrap();
    let crc = crc32fast::hash(&vec![b' '; 52 * 1024 * 1024]);
    let bomb_zip = build_deflated_single_zip("targets.txt", &compressed, 1024, crc);
    let err = parse_import_zip_bytes(&bomb_zip).unwrap_err();
    assert!(
        err.to_string().contains("50 MiB"),
        "应拦截超过 50 MiB 的 ZIP 炸弹：{err}"
    );
}

fn build_deflated_single_zip(name: &str, comp: &[u8], fake_uncomp: u32, crc: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let name_b = name.as_bytes();
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes()); // DEFLATE
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    out.extend_from_slice(&fake_uncomp.to_le_bytes());
    out.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name_b);
    out.extend_from_slice(comp);

    let cd_offset = out.len() as u32;
    let mut cd = Vec::new();
    cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    cd.extend_from_slice(&20u16.to_le_bytes());
    cd.extend_from_slice(&20u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&8u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&crc.to_le_bytes());
    cd.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    cd.extend_from_slice(&fake_uncomp.to_le_bytes());
    cd.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u16.to_le_bytes());
    cd.extend_from_slice(&0u32.to_le_bytes());
    cd.extend_from_slice(&0u32.to_le_bytes());
    cd.extend_from_slice(name_b);

    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn bundle_import_is_atomic_on_single_invalid_url() {
    let mut reg = Registry::embedded().unwrap();
    let txt = r#"
https://valid-1.example.com/ | rule.http | ok-1 | 有效项 1
https://127.0.0.1/private | rule.http | bad-2 | 回环地址应被拒
https://valid-3.example.com/ | rule.http | ok-3 | 有效项 3
"#;
    let bundle = parse_import_text(txt, &[], false).unwrap();
    let preview = reg.preview_import_bundle(&bundle);
    assert!(!preview.all_valid);
    assert!(preview.targets[0].valid);
    assert!(!preview.targets[1].valid);

    let err = reg.import_bundle(&bundle, true, false);
    assert!(err.is_err());
    // 确认原子回滚：没有任何部分写入
    assert!(reg.user_targets().is_empty());
    assert!(reg.user_rules().is_empty());
    assert_eq!(reg.catalog_size(), 135);
}

#[tokio::test]
async fn imported_bundle_targets_send_zero_packets_before_f3_grant() {
    let mut reg = Registry::embedded().unwrap();
    let bundle = parse_import_text(
        "https://zero-pkt.example.com/trace | rule.http,rule.trace | zero-pkt | 零发包测试",
        &[],
        false,
    )
    .unwrap();
    reg.import_bundle(&bundle, true, false).unwrap();

    let calls = Arc::new(AtomicU64::new(0));
    let transport: Arc<dyn Transport> = Arc::new(CountingTransport(Arc::clone(&calls)));
    let opts = CheckOptions {
        targets: vec!["user.zero-pkt".to_string()],
        run_id: Some("run-bundle-zero-pkt".to_string()),
        ..Default::default()
    };
    let output = runtime::run_check(&reg, &opts, Some(transport), true, None)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(output.report.network_request_count, 0);
    assert_eq!(output.report.exit_code, 3);
}
