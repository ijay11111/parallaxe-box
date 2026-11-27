//! Native desktop plugin host: consumes `parallaxe_core::plugins` with fail-closed digest checks.
//!
//! Excluded on Android, iOS, and wasm32 (Web provides its own localStorage/manifest adapter).
//! Red lines:
//! - Plugins are display-only suggestions; failures or circuit breaks never affect core verdicts.
//! - Local plugin directory paths are local user data and are never exported into reports.

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parallaxe_core::plugins::{self, HookEvent, Manifest, Page, Runtime};
use parallaxe_core::settings::Settings;

/// A candidate local plugin verified on disk, awaiting explicit user approval of its SHA-256.
#[derive(Debug, Clone)]
pub struct StagedPlugin {
    pub canonical_dir: String,
    pub sha256: String,
    pub manifest: Manifest,
    pub bytes: Vec<u8>,
}

/// Live runtime entry for an enabled plugin.
#[derive(Debug)]
pub struct EnabledPluginEntry {
    pub id: String,
    pub approved_sha256: String,
    pub manifest: Option<Manifest>,
    pub runtime: Runtime,
    pub last_error: Option<String>,
    pub circuit_open: bool,
}

/// Non-verdict display output produced by an enabled plugin invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDisplaySuggestion {
    pub plugin_id: String,
    pub plugin_name: String,
    pub messages: Vec<String>,
}

/// Desktop plugin runtime coordinator.
#[derive(Debug)]
pub struct PluginHost {
    /// Master execution toggle (when false, no plugin is invoked and no badges/pages run).
    pub master_enabled: bool,
    /// User input for adding a local plugin directory.
    pub dir_input: String,
    /// Staged plugin after `verify` awaiting explicit user SHA-256 approval.
    pub staged: Option<StagedPlugin>,
    pub staged_error: Option<String>,
    pub status_feedback: Option<String>,
    /// Loaded/verified entries keyed by plugin ID.
    pub entries: BTreeMap<String, EnabledPluginEntry>,
    /// Latest display suggestions from light/hook operations (never enters verdicts).
    pub suggestions: Vec<PluginDisplaySuggestion>,
    /// Currently selected heavy plugin page `(plugin_id, page_id)`.
    pub active_page: Option<(String, String)>,
    /// In-app read-only third-party notices modals.
    pub plugin_notices_open: bool,
    pub dep_notices_open: bool,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            master_enabled: true,
            dir_input: String::new(),
            staged: None,
            staged_error: None,
            status_feedback: None,
            entries: BTreeMap::new(),
            suggestions: Vec::new(),
            active_page: None,
            plugin_notices_open: false,
            dep_notices_open: false,
        }
    }
}

impl PluginHost {
    /// Synchronizes loaded plugin metadata against `Settings`, re-verifying every enabled plugin.
    pub fn sync_from_settings(&mut self, settings: &Settings) {
        self.entries
            .retain(|id, _| settings.plugins.contains_key(id));
        for (id, approved_sha) in &settings.plugins {
            let entry = self
                .entries
                .entry(id.clone())
                .or_insert_with(|| EnabledPluginEntry {
                    id: id.clone(),
                    approved_sha256: approved_sha.clone(),
                    manifest: None,
                    runtime: Runtime::default(),
                    last_error: None,
                    circuit_open: false,
                });
            if entry.approved_sha256 != *approved_sha {
                entry.approved_sha256 = approved_sha.clone();
                entry.runtime = Runtime::default();
                entry.circuit_open = false;
            }
            match plugins::read_enabled(settings, id) {
                Ok((_, manifest)) => {
                    entry.manifest = Some(manifest);
                    if !entry.circuit_open {
                        entry.last_error = None;
                    }
                }
                Err(err) => {
                    entry.manifest = None;
                    entry.last_error = Some(err);
                }
            }
        }
        if let Some((pid, page_id)) = &self.active_page {
            let valid = self.entries.get(pid).is_some_and(|e| {
                e.manifest
                    .as_ref()
                    .is_some_and(|m| m.pages.iter().any(|pg| pg.id == *page_id))
            });
            if !valid {
                self.active_page = None;
            }
        }
    }

    /// Reads and validates a local plugin directory without enabling it.
    pub fn stage_directory(&mut self, dir: &Path) {
        self.staged = None;
        self.staged_error = None;
        self.status_feedback = None;
        match plugins::read_directory(dir) {
            Ok((bytes, manifest)) => match std::fs::canonicalize(dir) {
                Ok(canonical) => match canonical.to_str() {
                    Some(canonical_str) => {
                        let sha256 = plugins::digest(&bytes);
                        self.staged = Some(StagedPlugin {
                            canonical_dir: canonical_str.to_string(),
                            sha256,
                            manifest,
                            bytes,
                        });
                    }
                    None => {
                        self.staged_error = Some("plugin path must be valid UTF-8".to_string());
                    }
                },
                Err(err) => {
                    self.staged_error = Some(err.to_string());
                }
            },
            Err(err) => {
                self.staged_error = Some(err);
            }
        }
    }

    /// Approves the currently staged plugin's SHA-256 and records it in `settings`.
    pub fn approve_staged(&mut self, settings: &mut Settings) -> Result<String, String> {
        let staged = self
            .staged
            .clone()
            .ok_or_else(|| "no staged plugin to approve".to_string())?;
        let normalized = plugins::normalize_sha256(&staged.sha256)?;
        plugins::verified(&staged.bytes, &normalized)?;
        let id = staged.manifest.id.clone();
        settings.plugins.insert(id.clone(), normalized);
        settings
            .plugin_directories
            .insert(id.clone(), staged.canonical_dir);
        settings.validate()?;
        self.staged = None;
        self.staged_error = None;
        self.sync_from_settings(settings);
        Ok(id)
    }

    /// Invokes all enabled plugins through `Runtime::invoke` with per-invocation digest checks.
    /// Never modifies verdicts; failures are isolated per plugin and trip the 3-error circuit breaker.
    pub fn invoke_enabled(
        &mut self,
        settings: &Settings,
        input: &str,
        event: Option<HookEvent>,
        counts: Option<(usize, usize)>,
    ) {
        self.suggestions.clear();
        if !self.master_enabled || settings.plugins.is_empty() {
            return;
        }
        let ids: Vec<String> = settings.plugins.keys().cloned().collect();
        for id in ids {
            let Some(approved_sha) = settings.plugins.get(&id).cloned() else {
                continue;
            };
            let entry = self
                .entries
                .entry(id.clone())
                .or_insert_with(|| EnabledPluginEntry {
                    id: id.clone(),
                    approved_sha256: approved_sha.clone(),
                    manifest: None,
                    runtime: Runtime::default(),
                    last_error: None,
                    circuit_open: false,
                });
            let Some(dir) = settings.plugin_directories.get(&id) else {
                entry.last_error = Some("plugin directory missing".into());
                continue;
            };
            let raw_bytes = match plugins::read_directory(Path::new(dir)) {
                Ok((b, manifest)) if manifest.id == id => b,
                Ok(_) => {
                    let invoke_err = entry
                        .runtime
                        .invoke(&[], &approved_sha, input, event, counts)
                        .err()
                        .unwrap_or_else(|| "plugin id changed".to_string());
                    entry.circuit_open = invoke_err == "plugin circuit open";
                    entry.last_error = Some(if entry.circuit_open {
                        invoke_err
                    } else {
                        "plugin id changed".to_string()
                    });
                    entry.manifest = None;
                    continue;
                }
                Err(err) => {
                    // Feed invalid bytes to runtime so consecutive errors still trip circuit breaker.
                    let invoke_err = entry
                        .runtime
                        .invoke(&[], &approved_sha, input, event, counts)
                        .err()
                        .unwrap_or_else(|| err.clone());
                    entry.circuit_open = invoke_err == "plugin circuit open";
                    entry.last_error = Some(if entry.circuit_open { invoke_err } else { err });
                    entry.manifest = None;
                    continue;
                }
            };
            match entry
                .runtime
                .invoke(&raw_bytes, &approved_sha, input, event, counts)
            {
                Ok(messages) => {
                    entry.last_error = None;
                    entry.circuit_open = false;
                    if let Ok(manifest) = plugins::verified(&raw_bytes, &approved_sha) {
                        let plugin_name = manifest.name.clone();
                        entry.manifest = Some(manifest);
                        if !messages.is_empty() {
                            self.suggestions.push(PluginDisplaySuggestion {
                                plugin_id: id,
                                plugin_name,
                                messages,
                            });
                        }
                    }
                }
                Err(err) => {
                    entry.circuit_open = err == "plugin circuit open";
                    entry.last_error = Some(err);
                    if plugins::verified(&raw_bytes, &approved_sha).is_err() {
                        entry.manifest = None;
                    }
                }
            }
        }
    }

    /// Returns all pages exposed by currently verified, enabled heavy plugins.
    pub fn available_pages(&self) -> Vec<(String, String, Page)> {
        if !self.master_enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        for entry in self.entries.values() {
            if entry.circuit_open {
                continue;
            }
            if let Some(manifest) = &entry.manifest {
                for page in &manifest.pages {
                    out.push((entry.id.clone(), manifest.name.clone(), page.clone()));
                }
            }
        }
        out
    }

    /// Looks up the active heavy plugin page, returning `(manifest, page, sha256)`.
    pub fn resolve_active_page(&self) -> Option<(&Manifest, &Page, &str)> {
        if !self.master_enabled {
            return None;
        }
        let (pid, page_id) = self.active_page.as_ref()?;
        let entry = self.entries.get(pid)?;
        if entry.circuit_open {
            return None;
        }
        let manifest = entry.manifest.as_ref()?;
        let page = manifest.pages.iter().find(|pg| pg.id == *page_id)?;
        Some((manifest, page, entry.approved_sha256.as_str()))
    }

    /// Returns local repository sample plugin directories if present on disk (convenience helper).
    pub fn discover_local_sample_dirs() -> Vec<(&'static str, PathBuf)> {
        let mut candidates = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("plugins").join("samples"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("plugins").join("samples"));
            }
        }
        for base in candidates {
            let light = base.join("light-report-badge");
            let heavy = base.join("heavy-report-guide");
            let mut found = Vec::new();
            if light.join("manifest.json").is_file() {
                found.push(("light-report-badge", light));
            }
            if heavy.join("manifest.json").is_file() {
                found.push(("heavy-report-guide", heavy));
            }
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }
}
