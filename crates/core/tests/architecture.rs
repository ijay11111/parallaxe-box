//! 架构红线：共享核心不依赖 GUI，也不依赖 CLI 解析库。

use std::process::Command;

#[test]
fn core_dependency_tree_has_no_gui_or_clap() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let output = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "parallaxe-core",
            "-e",
            "normal",
            "--prefix",
            "none",
        ])
        .current_dir(&root)
        .output()
        .expect("运行 cargo tree");
    assert!(
        output.status.success(),
        "cargo tree 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);
    for forbidden in ["egui", "eframe", "clap", "egui_extras"] {
        assert!(
            !tree.lines().any(|line| {
                line.split_whitespace()
                    .next()
                    .map(|name| name == forbidden || name.starts_with(&format!("{forbidden} ")))
                    .unwrap_or(false)
            }),
            "parallaxe-core 依赖树不应包含 {forbidden}\n{tree}"
        );
    }
    // 必须包含共享核心所需的传输与序列化依赖。
    assert!(tree.contains("reqwest"));
    assert!(tree.contains("serde"));
}
