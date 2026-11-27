//! Native OS folder & file picker dialogs for explicit rule export and custom import selection.
//!
//! Zero external crate dependencies and zero unsafe code:
//! - Windows: invokes built-in PowerShell .NET `FolderBrowserDialog` / `OpenFileDialog` (STA).
//! - Linux: tries `zenity --file-selection`, then `kdialog`.
//! - macOS: invokes `osascript` `choose folder` / `choose file`.
//! - Android: returns explicit mobile guidance (zero storage permission).

use std::path::PathBuf;

/// Import file dialog extension filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFileFilter {
    /// `.txt`, `.md`, `.csv`, `.json`
    TextBundle,
    /// `.zip`
    ZipBundle,
}

/// Opens a native file selection dialog for importing URL lists or ZIP bundles.
/// Returns `Ok(Some(path))` if selected, `Ok(None)` if cancelled.
pub fn pick_import_file(prompt: &str, filter: ImportFileFilter) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let win_filter = match filter {
            ImportFileFilter::TextBundle => {
                "Import Files (*.txt;*.md;*.csv;*.json)|*.txt;*.md;*.csv;*.json|All Files (*.*)|*.*"
            }
            ImportFileFilter::ZipBundle => "ZIP Bundle (*.zip)|*.zip|All Files (*.*)|*.*",
        };
        let script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.OpenFileDialog
$dlg.Title = $env:PARALLAXE_PICKER_PROMPT
$dlg.Filter = $env:PARALLAXE_PICKER_FILTER
$dlg.CheckFileExists = $true
$dlg.Multiselect = $false
$res = $dlg.ShowDialog()
if ($res -eq [System.Windows.Forms.DialogResult]::OK -and -not [string]::IsNullOrWhiteSpace($dlg.FileName)) {
    [Console]::Out.Write($dlg.FileName)
}
"#;
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
            .env("PARALLAXE_PICKER_PROMPT", prompt)
            .env("PARALLAXE_PICKER_FILTER", win_filter)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|err| format!("failed to launch file dialog: {err}"))?;
        if !output.status.success() {
            return Err("无法打开文件选择窗口，请直接粘贴网址文本导入".to_string());
        }
        let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if chosen.is_empty() {
            Ok(None)
        } else {
            Ok(Some(PathBuf::from(chosen)))
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let zenity_filter = match filter {
            ImportFileFilter::TextBundle => "Import files | *.txt *.md *.csv *.json",
            ImportFileFilter::ZipBundle => "ZIP bundle | *.zip",
        };
        if let Ok(output) = Command::new("zenity")
            .args([
                "--file-selection",
                "--title",
                prompt,
                "--file-filter",
                zenity_filter,
            ])
            .output()
        {
            if output.status.success() {
                let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
            }
            if output.status.code() == Some(1) {
                return Ok(None);
            }
        }
        let kdialog_filter = match filter {
            ImportFileFilter::TextBundle => "*.txt *.md *.csv *.json",
            ImportFileFilter::ZipBundle => "*.zip",
        };
        if let Ok(output) = Command::new("kdialog")
            .args(["--getopenfilename", ".", kdialog_filter, "--title", prompt])
            .output()
        {
            if output.status.success() {
                let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
            }
            if output.status.code() == Some(1) {
                return Ok(None);
            }
        }
        Err("系统未提供文件选择框，请直接粘贴网址文本导入".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let _ = filter;
        let output = Command::new("osascript")
            .args([
                "-e",
                r#"POSIX path of (choose file with prompt (system attribute "PARALLAXE_PICKER_PROMPT"))"#,
            ])
            .env("PARALLAXE_PICKER_PROMPT", prompt)
            .output()
            .map_err(|err| format!("failed to launch file dialog: {err}"))?;
        if output.status.success() {
            let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            Ok(None)
        } else {
            Err("无法打开文件选择框，请直接粘贴网址文本导入".to_string())
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (prompt, filter);
        Err("移动端不支持直接读取本地文件路径，请粘贴网址文本".to_string())
    }
}

/// Opens a native folder selection dialog.
/// Returns `Ok(Some(path))` if the user explicitly chose a folder, `Ok(None)` if cancelled.
pub fn pick_export_folder(prompt: &str) -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.FolderBrowserDialog
$dlg.Description = $env:PARALLAXE_PICKER_PROMPT
$dlg.ShowNewFolderButton = $true
$res = $dlg.ShowDialog()
if ($res -eq [System.Windows.Forms.DialogResult]::OK -and -not [string]::IsNullOrWhiteSpace($dlg.SelectedPath)) {
    [Console]::Out.Write($dlg.SelectedPath)
}
"#;
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
            .env("PARALLAXE_PICKER_PROMPT", prompt)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|err| format!("failed to launch folder dialog: {err}"))?;
        if !output.status.success() {
            return Err("folder dialog failed; copy rules from preview instead".to_string());
        }
        let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if chosen.is_empty() {
            Ok(None)
        } else {
            Ok(Some(PathBuf::from(chosen)))
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        if let Ok(output) = Command::new("zenity")
            .args(["--file-selection", "--directory", "--title", prompt])
            .output()
        {
            if output.status.success() {
                let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
            }
            if output.status.code() == Some(1) {
                return Ok(None);
            }
        }
        if let Ok(output) = Command::new("kdialog")
            .args(["--getexistingdirectory", ".", "--title", prompt])
            .output()
        {
            if output.status.success() {
                let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
            }
            if output.status.code() == Some(1) {
                return Ok(None);
            }
        }
        Err("no native folder dialog available (zenity/kdialog); copy rules from preview or use CLI --out-dir".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let output = Command::new("osascript")
            .args([
                "-e",
                r#"POSIX path of (choose folder with prompt (system attribute "PARALLAXE_PICKER_PROMPT"))"#,
            ])
            .env("PARALLAXE_PICKER_PROMPT", prompt)
            .output()
            .map_err(|err| format!("failed to launch folder dialog: {err}"))?;
        if output.status.success() {
            let chosen = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok((!chosen.is_empty()).then(|| PathBuf::from(chosen)));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            Ok(None)
        } else {
            Err("folder dialog failed; copy rules from preview instead".to_string())
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = prompt;
        Err("folder picker unavailable on mobile; copy rules to clipboard instead".to_string())
    }
}
