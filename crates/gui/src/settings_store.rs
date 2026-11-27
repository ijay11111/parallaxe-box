//! Explicit settings persistence adapter for desktop and Android GUI.
//!
//! Zero-write-by-default: loading a missing file returns `Settings::default()` without
//! creating any directory or file. Files are only written when the user explicitly
//! changes a setting.

use std::path::{Path, PathBuf};

use parallaxe_core::settings::{self, Settings};

/// Returns the platform-appropriate default settings path for desktop GUI.
/// Uses standard OS environment variables with zero extra crate dependencies.
pub fn default_desktop_settings_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|base| {
            PathBuf::from(base)
                .join("parallaxe-box")
                .join("settings.jsonl")
        })
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("parallaxe-box")
                .join("settings.jsonl")
        })
    }
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "macos"),
        not(target_os = "android")
    ))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|base| base.join("parallaxe-box").join("settings.jsonl"))
    }
    #[cfg(target_os = "android")]
    {
        None
    }
}

/// Loads persisted settings without creating any files.
/// Returns `(Settings, Option<error_message>)`.
pub fn load_or_default(path: Option<&Path>) -> (Settings, Option<String>) {
    let Some(path) = path else {
        return (Settings::default(), None);
    };
    match settings::load(path) {
        Ok(loaded) => (loaded, None),
        Err(err) => (Settings::default(), Some(err)),
    }
}

/// Persists a user-initiated settings mutation atomically via `parallaxe_core::settings::update`.
pub fn persist<F>(path: Option<&Path>, mutate: F) -> Result<Settings, String>
where
    F: FnOnce(&mut Settings) -> Result<(), String>,
{
    let Some(path) = path else {
        let mut s = Settings::default();
        mutate(&mut s)?;
        s.validate()?;
        return Ok(s);
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    settings::update(path, mutate)
}

/// Repairs a partially written trailing settings record.
pub fn repair(path: Option<&Path>) -> Result<Settings, String> {
    let Some(path) = path else {
        return Ok(Settings::default());
    };
    settings::repair(path)
}
