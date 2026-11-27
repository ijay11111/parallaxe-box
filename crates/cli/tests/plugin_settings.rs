#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::io::Write;
use std::process::Command;
use std::sync::Arc;

use parallaxe_core::{plugins, settings};

const MANIFEST: &str = r#"{"schema":"parallaxe.plugin/v1","id":"example","name":"Example","version":"1","author":"Example","license":"MIT","source":"https://example.com/"}"#;

#[test]
fn cli_requires_approval_and_rechecks_enabled_content() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("plugin");
    std::fs::create_dir(&directory).unwrap();
    let manifest_path = directory.join("manifest.json");
    std::fs::write(&manifest_path, MANIFEST).unwrap();
    let settings_path = temp.path().join("settings.jsonl");
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_parallaxe"))
            .args(args)
            .output()
            .unwrap()
    };
    let settings_arg = settings_path.as_os_str();
    assert!(run(&[
        "plugin".as_ref(),
        "list".as_ref(),
        "--settings".as_ref(),
        settings_arg
    ])
    .status
    .success());
    assert!(!settings_path.exists());
    assert!(
        run(&["plugin".as_ref(), "verify".as_ref(), directory.as_os_str()])
            .status
            .success()
    );
    assert!(!run(&[
        "plugin".as_ref(),
        "enable".as_ref(),
        directory.as_os_str(),
        "--settings".as_ref(),
        settings_arg,
        "--approve-sha256".as_ref(),
        "wrong".as_ref()
    ])
    .status
    .success());
    assert!(!settings_path.exists());
    let digest = plugins::digest(MANIFEST.as_bytes());
    assert!(run(&[
        "plugin".as_ref(),
        "enable".as_ref(),
        directory.as_os_str(),
        "--settings".as_ref(),
        settings_arg,
        "--approve-sha256".as_ref(),
        digest.as_ref()
    ])
    .status
    .success());
    let config = settings::load(&settings_path).unwrap();
    assert!(plugins::read_enabled(&config, "example").is_ok());
    std::fs::write(&manifest_path, format!("{MANIFEST} ")).unwrap();
    assert!(plugins::read_enabled(&config, "example").is_err());
    assert!(run(&[
        "plugin".as_ref(),
        "disable".as_ref(),
        "example".as_ref(),
        "--settings".as_ref(),
        settings_arg
    ])
    .status
    .success());
    assert!(settings::load(&settings_path).unwrap().plugins.is_empty());
}

#[test]
fn settings_serializes_concurrent_updates_and_detects_partial_record() {
    let temp = tempfile::tempdir().unwrap();
    let path = Arc::new(temp.path().join("settings.jsonl"));
    assert_eq!(
        settings::load(&path).unwrap(),
        settings::Settings::default()
    );
    assert!(!path.exists());
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let path = Arc::clone(&path);
            std::thread::spawn(move || {
                settings::update(&path, |config| {
                    config.min_interval_ms += 1;
                    Ok(())
                })
                .unwrap();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let committed = settings::load(&path).unwrap();
    assert_eq!(
        committed.min_interval_ms,
        settings::Settings::default().min_interval_ms + 8
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&*path)
        .unwrap()
        .write_all(b"{partial")
        .unwrap();
    assert_eq!(settings::load(&path).unwrap(), committed);
    assert!(settings::update(&path, |_| Ok(())).is_err());

    // Repair truncates the incomplete trailing line and restores updates.
    let repaired = settings::repair(&path).unwrap();
    assert_eq!(repaired, committed);
    let after = settings::update(&path, |config| {
        config.language = "en-US".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(after.language, "en-US");

    // Failed update on a nonexistent file must not leave an empty 0-byte file behind.
    let fresh = temp.path().join("fresh-failed.jsonl");
    assert!(settings::update(&fresh, |config| {
        config.language = "invalid".into();
        Ok(())
    })
    .is_err());
    assert!(!fresh.exists());
}

#[test]
fn cli_accepts_uppercase_sha256_detailed_list_and_repair() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("plugin");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("manifest.json"), MANIFEST).unwrap();
    let settings_path = temp.path().join("settings.jsonl");
    let upper_digest = plugins::digest(MANIFEST.as_bytes()).to_ascii_uppercase();

    let enable = Command::new(env!("CARGO_BIN_EXE_parallaxe"))
        .args(["plugin", "enable"])
        .arg(&directory)
        .args(["--settings"])
        .arg(&settings_path)
        .args(["--approve-sha256", &upper_digest])
        .output()
        .unwrap();
    assert!(
        enable.status.success(),
        "{}",
        String::from_utf8_lossy(&enable.stderr)
    );

    let list = Command::new(env!("CARGO_BIN_EXE_parallaxe"))
        .args(["plugin", "list", "--detailed", "--settings"])
        .arg(&settings_path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(list_out.contains("\"name\": \"Example\""));
    assert!(list_out.contains("\"status\": \"verified\""));
    assert!(!list_out.contains("plugin_directories"));

    std::fs::OpenOptions::new()
        .append(true)
        .open(&settings_path)
        .unwrap()
        .write_all(b"{corrupt")
        .unwrap();

    let repair = Command::new(env!("CARGO_BIN_EXE_parallaxe"))
        .args(["plugin", "repair", "--settings"])
        .arg(&settings_path)
        .output()
        .unwrap();
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    assert!(settings::update(&settings_path, |_| Ok(())).is_ok());
}

#[test]
fn selftest_takes_precedence_over_plugin_subcommand() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.jsonl");
    let result = Command::new(env!("CARGO_BIN_EXE_parallaxe"))
        .args(["--selftest", "plugin", "list", "--settings"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!path.exists());
    assert!(!String::from_utf8_lossy(&result.stdout)
        .trim()
        .starts_with('{'));
}
