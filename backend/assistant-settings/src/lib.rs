// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The assistant's advanced settings (`phase-7-local-ai/PLAN.md`, step R1):
//! which backend(s) answer, where the models are, and the resource limits the
//! launcher enforces on the assistant process.
//!
//! One type, one validator. Every reader and writer goes through
//! [`AssistantSettings::validate`], so a hand-edited file, a future editor,
//! and the launcher's spawn logic can never disagree about what is legal. The
//! launcher turns valid settings into `blueice-ai-assistant` flags
//! ([`AssistantSettings::assistant_args`]) rather than a second parser reading
//! the file, so the assistant's own flag parsing stays the single
//! interpretation of "which backend is selected".
//!
//! A missing file means "no assistant configured" (the default). A file that
//! is present but invalid is an *error*, never silently treated as the
//! default: quietly ignoring a person's settings is worse than saying so.

use blueice_loopback_model::validate_config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The file format version this build reads and writes.
pub const SETTINGS_VERSION: u32 = 1;

pub const MIN_IDLE_TIMEOUT_SECS: u64 = 30;
pub const MAX_IDLE_TIMEOUT_SECS: u64 = 86_400;
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;
pub const MIN_RESIDENT_MB: u64 = 256;
pub const MAX_RESIDENT_MB: u64 = 1_048_576;
/// Scheduling niceness: 0 is the launcher's own priority, 19 the lowest.
pub const MAX_NICE: i32 = 19;
pub const DEFAULT_NICE: i32 = 10;
pub const MIN_CANDLE_CONTEXT: usize = 64;
pub const MAX_CANDLE_CONTEXT: usize = 131_072;
pub const DEFAULT_CANDLE_CONTEXT: usize = 4096;
const MAX_PATH_BYTES: usize = 4096;

/// Which backend(s) answer requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// No assistant is configured: it is never started.
    #[default]
    None,
    /// A local `llama.cpp`, Ollama, or TGI server.
    Loopback,
    /// An in-process Qwen3 GGUF model (needs the `candle` build feature).
    Candle,
    /// Both at once; the first success wins. Costs double the resources.
    Both,
}

/// An external loopback model server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopbackSettings {
    pub provider: String,
    pub base_url: String,
    pub model: String,
}

/// An in-process model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandleSettings {
    pub model_path: String,
    pub tokenizer_path: String,
    pub context: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantSettings {
    pub version: u32,
    pub backend: BackendKind,
    #[serde(default)]
    pub loopback: Option<LoopbackSettings>,
    #[serde(default)]
    pub candle: Option<CandleSettings>,
    /// How long the assistant may sit with no connection before the launcher
    /// may tear it down (under memory pressure). Activity includes every
    /// translation and task, so an assistant in use is never idle.
    pub idle_timeout_secs: u64,
    /// The resident-memory ceiling in MiB; the launcher kills the assistant if
    /// it grows past it. `None` means unlimited.
    #[serde(default)]
    pub max_resident_mb: Option<u64>,
    /// Scheduling niceness applied to the assistant (0 to [`MAX_NICE`]).
    pub nice: i32,
}

impl Default for AssistantSettings {
    fn default() -> Self {
        AssistantSettings {
            version: SETTINGS_VERSION,
            backend: BackendKind::None,
            loopback: None,
            candle: None,
            idle_timeout_secs: DEFAULT_IDLE_TIMEOUT_SECS,
            max_resident_mb: None,
            nice: DEFAULT_NICE,
        }
    }
}

fn validate_path(label: &str, path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(format!(
            "{label} must be a path of 1 to {MAX_PATH_BYTES} bytes"
        ));
    }
    if !Path::new(path).is_absolute() {
        return Err(format!("{label} must be an absolute path"));
    }
    Ok(())
}

impl AssistantSettings {
    /// Checks every documented bound and that the backend selection matches the
    /// sections present (a backend needs its own section and refuses the other's).
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SETTINGS_VERSION {
            return Err(format!(
                "assistant settings version {} is not supported (this build reads {SETTINGS_VERSION})",
                self.version
            ));
        }
        if !(MIN_IDLE_TIMEOUT_SECS..=MAX_IDLE_TIMEOUT_SECS).contains(&self.idle_timeout_secs) {
            return Err(format!(
                "idle_timeout_secs must be {MIN_IDLE_TIMEOUT_SECS} to {MAX_IDLE_TIMEOUT_SECS}"
            ));
        }
        if let Some(mb) = self.max_resident_mb {
            if !(MIN_RESIDENT_MB..=MAX_RESIDENT_MB).contains(&mb) {
                return Err(format!(
                    "max_resident_mb must be {MIN_RESIDENT_MB} to {MAX_RESIDENT_MB}, or omitted for no limit"
                ));
            }
        }
        if !(0..=MAX_NICE).contains(&self.nice) {
            return Err(format!("nice must be 0 to {MAX_NICE}"));
        }
        if let Some(loopback) = &self.loopback {
            validate_config(
                loopback.provider.clone(),
                loopback.base_url.clone(),
                loopback.model.clone(),
            )?;
        }
        if let Some(candle) = &self.candle {
            validate_path("candle model_path", &candle.model_path)?;
            validate_path("candle tokenizer_path", &candle.tokenizer_path)?;
            if !(MIN_CANDLE_CONTEXT..=MAX_CANDLE_CONTEXT).contains(&candle.context) {
                return Err(format!(
                    "candle context must be {MIN_CANDLE_CONTEXT} to {MAX_CANDLE_CONTEXT}"
                ));
            }
        }
        let (needs_loopback, needs_candle) = match self.backend {
            BackendKind::None => (false, false),
            BackendKind::Loopback => (true, false),
            BackendKind::Candle => (false, true),
            BackendKind::Both => (true, true),
        };
        for (needed, present, section, backend) in [
            (
                needs_loopback,
                self.loopback.is_some(),
                "loopback",
                self.backend,
            ),
            (needs_candle, self.candle.is_some(), "candle", self.backend),
        ] {
            if needed && !present {
                return Err(format!("backend {backend:?} needs a {section} section"));
            }
            if !needed && present {
                return Err(format!(
                    "a {section} section is present but backend {backend:?} does not use it"
                ));
            }
        }
        Ok(())
    }

    /// Whether an assistant should be started at all.
    pub fn is_configured(&self) -> bool {
        self.backend != BackendKind::None
    }

    /// The `blueice-ai-assistant` flags these settings select. Only meaningful
    /// for valid settings; the assistant's own flag parser stays the one
    /// interpretation of them.
    pub fn assistant_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let backend = match self.backend {
            BackendKind::None => return args,
            BackendKind::Loopback => "loopback",
            BackendKind::Candle => "candle",
            BackendKind::Both => "both",
        };
        args.extend(["--backend".to_string(), backend.to_string()]);
        if let Some(loopback) = &self.loopback {
            args.extend([
                "--model-provider".to_string(),
                loopback.provider.clone(),
                "--model-base-url".to_string(),
                loopback.base_url.clone(),
                "--model-name".to_string(),
                loopback.model.clone(),
            ]);
        }
        if let Some(candle) = &self.candle {
            args.extend([
                "--candle-model".to_string(),
                candle.model_path.clone(),
                "--candle-tokenizer".to_string(),
                candle.tokenizer_path.clone(),
                "--candle-context".to_string(),
                candle.context.to_string(),
            ]);
        }
        args
    }
}

/// Where the settings live by default, beside the gatekeeper's.
pub fn default_settings_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("blueice").join("assistant-settings.json")
}

/// Reads and validates the settings at `path`. A missing file is the default
/// (no assistant); an unreadable, malformed, or invalid file is an error.
pub fn load(path: &Path) -> Result<AssistantSettings, String> {
    load_existing(path).map(Option::unwrap_or_default)
}

/// Like [`load`], but says whether there was a file at all: `Ok(None)` for a
/// missing one, so a display can tell "not configured" from "configured with
/// the defaults".
pub fn load_existing(path: &Path) -> Result<Option<AssistantSettings>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("reading assistant settings {}: {error}", path.display()))
        }
    };
    let settings: AssistantSettings = serde_json::from_str(&raw)
        .map_err(|error| format!("parsing assistant settings {}: {error}", path.display()))?;
    settings
        .validate()
        .map_err(|error| format!("assistant settings {}: {error}", path.display()))?;
    Ok(Some(settings))
}

/// Validates and atomically writes `settings` to `path` (a temporary file in
/// the same directory, then a rename), readable only by the owner. The parent
/// directory is created if needed.
pub fn save(path: &Path, settings: &AssistantSettings) -> Result<(), String> {
    settings.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("assistant settings path {} has no parent", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("encoding assistant settings: {error}"))?;
    let temporary = temporary_path(path);
    write_private(&temporary, &json)
        .map_err(|error| format!("writing {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("replacing {}: {error}", path.display())
    })
}

/// A fresh sibling path for one save. It *appends* to the file name (rather
/// than replacing its extension, which would map `settings.json-1` and
/// `settings.json-2` to the same name) and carries a per-process counter, so
/// two saves -- of different files or of the same one -- never share a
/// temporary, and the rename onto the target stays within one directory.
fn temporary_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    path.with_file_name(name)
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn loopback() -> LoopbackSettings {
        LoopbackSettings {
            provider: "llamacpp".into(),
            base_url: "http://127.0.0.1:8080/v1/".into(),
            model: "local".into(),
        }
    }

    fn candle() -> CandleSettings {
        CandleSettings {
            model_path: "/models/qwen3.gguf".into(),
            tokenizer_path: "/models/tokenizer.json".into(),
            context: DEFAULT_CANDLE_CONTEXT,
        }
    }

    fn with_backend(backend: BackendKind) -> AssistantSettings {
        AssistantSettings {
            backend,
            loopback: matches!(backend, BackendKind::Loopback | BackendKind::Both).then(loopback),
            candle: matches!(backend, BackendKind::Candle | BackendKind::Both).then(candle),
            ..AssistantSettings::default()
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "as-settings-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn the_default_means_no_assistant_and_is_valid() {
        let settings = AssistantSettings::default();
        assert!(settings.validate().is_ok());
        assert!(!settings.is_configured());
        assert!(settings.assistant_args().is_empty());
    }

    #[test]
    fn every_backend_validates_with_exactly_its_own_sections() {
        for backend in [
            BackendKind::None,
            BackendKind::Loopback,
            BackendKind::Candle,
            BackendKind::Both,
        ] {
            assert!(with_backend(backend).validate().is_ok(), "{backend:?}");
        }
    }

    #[test]
    fn a_backend_without_its_section_is_refused() {
        for backend in [
            BackendKind::Loopback,
            BackendKind::Candle,
            BackendKind::Both,
        ] {
            let settings = AssistantSettings {
                backend,
                ..AssistantSettings::default()
            };
            assert!(
                settings.validate().unwrap_err().contains("needs a"),
                "{backend:?}"
            );
        }
        // Both needs *both* sections, not just one of them.
        let half = AssistantSettings {
            backend: BackendKind::Both,
            ..with_backend(BackendKind::Loopback)
        };
        assert!(half.validate().unwrap_err().contains("candle"));
    }

    #[test]
    fn a_section_the_chosen_backend_does_not_use_is_refused() {
        let mut stray = with_backend(BackendKind::Loopback);
        stray.candle = Some(candle());
        assert!(stray.validate().unwrap_err().contains("does not use it"));
        let none_with_section = AssistantSettings {
            loopback: Some(loopback()),
            ..AssistantSettings::default()
        };
        assert!(none_with_section.validate().is_err());
    }

    #[test]
    fn every_numeric_bound_is_enforced_at_and_beyond_its_edge() {
        let mut s = AssistantSettings::default();
        for (secs, ok) in [
            (MIN_IDLE_TIMEOUT_SECS - 1, false),
            (MIN_IDLE_TIMEOUT_SECS, true),
            (MAX_IDLE_TIMEOUT_SECS, true),
            (MAX_IDLE_TIMEOUT_SECS + 1, false),
        ] {
            s.idle_timeout_secs = secs;
            assert_eq!(s.validate().is_ok(), ok, "idle {secs}");
        }
        s.idle_timeout_secs = DEFAULT_IDLE_TIMEOUT_SECS;
        for (mb, ok) in [
            (None, true),
            (Some(MIN_RESIDENT_MB - 1), false),
            (Some(MIN_RESIDENT_MB), true),
            (Some(MAX_RESIDENT_MB), true),
            (Some(MAX_RESIDENT_MB + 1), false),
        ] {
            s.max_resident_mb = mb;
            assert_eq!(s.validate().is_ok(), ok, "mb {mb:?}");
        }
        s.max_resident_mb = None;
        for (nice, ok) in [
            (-1, false),
            (0, true),
            (MAX_NICE, true),
            (MAX_NICE + 1, false),
        ] {
            s.nice = nice;
            assert_eq!(s.validate().is_ok(), ok, "nice {nice}");
        }
        s.nice = DEFAULT_NICE;
        let mut c = with_backend(BackendKind::Candle);
        for (context, ok) in [
            (MIN_CANDLE_CONTEXT - 1, false),
            (MIN_CANDLE_CONTEXT, true),
            (MAX_CANDLE_CONTEXT, true),
            (MAX_CANDLE_CONTEXT + 1, false),
        ] {
            c.candle.as_mut().unwrap().context = context;
            assert_eq!(c.validate().is_ok(), ok, "context {context}");
        }
    }

    #[test]
    fn the_loopback_endpoint_rules_are_the_shared_ones() {
        for base_url in [
            "https://127.0.0.1:8080/v1/",
            "http://example.com:8080/v1/",
            "http://localhost:8080/v1/",
            "http://user@127.0.0.1:8080/v1/",
        ] {
            let mut s = with_backend(BackendKind::Loopback);
            s.loopback.as_mut().unwrap().base_url = base_url.into();
            assert!(s.validate().is_err(), "{base_url}");
        }
    }

    #[test]
    fn candle_paths_must_be_absolute_and_bounded() {
        for path in [
            "",
            "relative/model.gguf",
            "../model.gguf",
            "/ok\0bad",
            &"/".repeat(5000),
        ] {
            let mut s = with_backend(BackendKind::Candle);
            s.candle.as_mut().unwrap().model_path = path.to_string();
            assert!(s.validate().is_err(), "{path:?}");
            let mut s = with_backend(BackendKind::Candle);
            s.candle.as_mut().unwrap().tokenizer_path = path.to_string();
            assert!(s.validate().is_err(), "{path:?}");
        }
    }

    #[test]
    fn an_unknown_version_is_refused() {
        let s = AssistantSettings {
            version: SETTINGS_VERSION + 1,
            ..AssistantSettings::default()
        };
        assert!(s.validate().unwrap_err().contains("not supported"));
    }

    #[test]
    fn settings_render_to_the_flags_the_assistant_parses() {
        assert_eq!(
            with_backend(BackendKind::Loopback).assistant_args(),
            [
                "--backend",
                "loopback",
                "--model-provider",
                "llamacpp",
                "--model-base-url",
                "http://127.0.0.1:8080/v1/",
                "--model-name",
                "local",
            ]
        );
        assert_eq!(
            with_backend(BackendKind::Candle).assistant_args(),
            [
                "--backend",
                "candle",
                "--candle-model",
                "/models/qwen3.gguf",
                "--candle-tokenizer",
                "/models/tokenizer.json",
                "--candle-context",
                "4096",
            ]
        );
        let both = with_backend(BackendKind::Both).assistant_args();
        assert_eq!(&both[..2], ["--backend", "both"]);
        assert!(both.contains(&"--model-name".to_string()));
        assert!(both.contains(&"--candle-model".to_string()));
    }

    #[test]
    fn a_missing_file_is_the_default_and_a_saved_file_round_trips() {
        let path = temp_path("roundtrip");
        assert_eq!(load(&path).unwrap(), AssistantSettings::default());
        let mut settings = with_backend(BackendKind::Both);
        settings.max_resident_mb = Some(2048);
        settings.nice = 15;
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        let _ = fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_only_by_its_owner_and_no_temporary_is_left_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_path("perm");
        let path = dir.join("nested").join("assistant-settings.json");
        save(&path, &with_backend(BackendKind::Loopback)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(leftovers, ["assistant-settings.json"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_temporary_file_never_collides_with_another_save() {
        // `with_extension` would map both of these to the same temporary name.
        let a = PathBuf::from("/d/settings.json-1");
        let b = PathBuf::from("/d/settings.json-2");
        assert_ne!(temporary_path(&a), temporary_path(&b));
        // Two saves of the very same file (two threads, or a retry) differ too.
        assert_ne!(temporary_path(&a), temporary_path(&a));
        // The temporary sits beside its target, so the rename stays atomic.
        assert_eq!(temporary_path(&a).parent(), a.parent());
    }

    #[test]
    fn concurrent_saves_of_different_files_do_not_clobber_each_other() {
        let dir = temp_path("concurrent");
        fs::create_dir_all(&dir).unwrap();
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let path = dir.join(format!("settings.json-{n}"));
                thread::spawn(move || {
                    for _ in 0..50 {
                        save(&path, &with_backend(BackendKind::Loopback)).unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        for n in 0..8 {
            let path = dir.join(format!("settings.json-{n}"));
            assert_eq!(load(&path).unwrap(), with_backend(BackendKind::Loopback));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_invalid_settings_writes_nothing() {
        let path = temp_path("invalid");
        let bad = AssistantSettings {
            nice: 99,
            ..AssistantSettings::default()
        };
        assert!(save(&path, &bad).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn a_present_but_invalid_file_is_an_error_not_the_default() {
        let path = temp_path("bad");
        for contents in [
            "not json",
            r#"{"version":1,"backend":"loopback","idle_timeout_secs":600,"nice":10}"#,
            r#"{"version":9,"backend":"none","idle_timeout_secs":600,"nice":10}"#,
            r#"{"version":1,"backend":"none","idle_timeout_secs":1,"nice":10}"#,
        ] {
            fs::write(&path, contents).unwrap();
            let error = load(&path).unwrap_err();
            assert!(error.contains("assistant settings"), "{contents}: {error}");
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_distinguishable_from_one_holding_the_defaults() {
        let path = temp_path("existing");
        assert_eq!(load_existing(&path).unwrap(), None);
        save(&path, &AssistantSettings::default()).unwrap();
        assert_eq!(load_existing(&path).unwrap(), Some(AssistantSettings::default()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_path_is_an_error() {
        // A directory where a file is expected cannot be read as one.
        let dir = temp_path("dir");
        fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir)
            .unwrap_err()
            .contains("reading assistant settings"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_path_is_beside_the_gatekeepers_settings() {
        let path = default_settings_path();
        assert_eq!(path.file_name().unwrap(), "assistant-settings.json");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "blueice");
    }
}
