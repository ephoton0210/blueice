// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `about:assistant` page (`phase-7-local-ai/PLAN.md`, checklist step S3):
//! where summaries and organized data land. It is a built-in page rendered by
//! BlueIce's own engine, like `about:downloads`, so a person sees the exact
//! render pass an AI agent reads through the snapshot -- not a native panel
//! only one frontend could draw.
//!
//! Every entry is *model output derived from untrusted page text*. It is
//! therefore shown only as escaped plain text, one paragraph per line, and the
//! page says it was written by a model. Nothing here is ever interpreted as
//! HTML, and nothing is written back into the page it describes.
//!
//! Below the results the page shows the assistant's *effective settings*, read
//! from the settings file the launcher gave `core` -- read-only. `core` never
//! writes that file: a control reachable through `core` is reachable by every
//! client of `core`, an AI agent included, and letting one edit its own
//! resource limits is a decision that has not been made (see
//! `phase-7-local-ai/PLAN.md`, R4).
//!
//! State lives in one [`AssistantPanel`] shared by every tab (a `core`-wide
//! list, newest first, bounded at [`MAX_ENTRIES`]); the page itself is a pure
//! function of that list.

use crate::downloads_page::escape_html;
use blueice_assistant_settings::{AssistantSettings, BackendKind};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The well-known URL `Page` recognizes as a request for the panel. An
/// optional `?lang=<locale>` picks a locale, exactly like `about:credits`.
pub const ASSISTANT_URL: &str = "about:assistant";

pub fn is_assistant_url(url: &str) -> bool {
    url == ASSISTANT_URL || url.starts_with("about:assistant?")
}

/// How many results the panel keeps; older ones are dropped.
pub const MAX_ENTRIES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Summary,
    Organized,
}

/// One requested task and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelEntry {
    pub id: u64,
    pub kind: PanelKind,
    /// The page the text was read from.
    pub source_url: Option<String>,
    /// The instruction, for an organize request.
    pub request: Option<String>,
    /// The model's text, or why the task could not be completed.
    pub outcome: Result<String, String>,
}

#[derive(Debug, Default)]
struct PanelState {
    /// Newest first.
    entries: VecDeque<PanelEntry>,
    next_id: u64,
}

/// The panel's shared state: whether an assistant is configured at all, and
/// the recent results.
#[derive(Debug, Default)]
pub struct AssistantPanel {
    state: Mutex<PanelState>,
    available: AtomicBool,
    /// The settings file to display, if the launcher gave one.
    settings_file: Mutex<Option<PathBuf>>,
}

impl AssistantPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Relaxed);
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    pub fn set_settings_file(&self, path: Option<PathBuf>) {
        *self.settings_file.lock().unwrap_or_else(|e| e.into_inner()) = path;
    }

    pub fn settings_file(&self) -> Option<PathBuf> {
        self.settings_file
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Records a finished task and returns its id. The oldest entry beyond
    /// [`MAX_ENTRIES`] is dropped.
    pub fn push(
        &self,
        kind: PanelKind,
        source_url: Option<String>,
        request: Option<String>,
        outcome: Result<String, String>,
    ) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.next_id += 1;
        let id = state.next_id;
        state.entries.push_front(PanelEntry {
            id,
            kind,
            source_url,
            request,
            outcome,
        });
        state.entries.truncate(MAX_ENTRIES);
        id
    }

    /// The current entries, newest first.
    pub fn entries(&self) -> Vec<PanelEntry> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.entries.iter().cloned().collect()
    }
}

/// What the settings section shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsView {
    /// The launcher gave `core` no settings file.
    Unspecified,
    /// A file was named but does not exist: no assistant is configured.
    Missing,
    /// The file exists but cannot be used; carries the validator's reason.
    Invalid(String),
    Loaded(Box<AssistantSettings>),
}

/// Reads (never writes) the settings file for display.
pub fn read_settings_view(path: Option<&Path>) -> SettingsView {
    let Some(path) = path else {
        return SettingsView::Unspecified;
    };
    match blueice_assistant_settings::load_existing(path) {
        Ok(None) => SettingsView::Missing,
        Ok(Some(settings)) => SettingsView::Loaded(Box::new(settings)),
        Err(reason) => SettingsView::Invalid(reason),
    }
}

const STYLE: &str = "body { padding: 12px; font-size: 15px; color: #222222; } \
    .note { color: #666666; } \
    .entry { margin-top: 14px; padding: 8px; border-width: 1px; border-style: solid; border-color: #cccccc; } \
    .kind { font-weight: bold; } \
    .meta { color: #555555; font-size: 13px; margin-top: 3px; } \
    .error { color: #b00020; } \
    .settings { margin-top: 18px; } \
    .setting { margin-top: 3px; }";

/// Builds the panel for `locale`, every string through `blueice-i18n`'s
/// `assistant` namespace and everything from outside escaped.
pub fn assistant_html(
    entries: &[PanelEntry],
    available: bool,
    settings: &SettingsView,
    settings_file: Option<&Path>,
    locale: &str,
) -> String {
    let locale = if blueice_i18n::SUPPORTED_LOCALES.contains(&locale) {
        locale
    } else {
        blueice_i18n::DEFAULT_LOCALE
    };
    let t = |key: &str| blueice_i18n::translate(locale, "assistant", key, &[]);

    let mut body = format!("<h1>{}</h1>", escape_html(&t("assistant-title")));
    if !available {
        body.push_str(&format!(
            "<p class=\"error\">{}</p>",
            escape_html(&t("assistant-unavailable"))
        ));
    } else if entries.is_empty() {
        body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            escape_html(&t("assistant-empty"))
        ));
    } else {
        body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            escape_html(&t("assistant-note"))
        ));
        for entry in entries {
            push_entry(&mut body, entry, &t);
        }
    }
    push_settings(&mut body, settings, settings_file, &t);
    format!(
        "<html><head><title>{}</title><style>{STYLE}</style></head><body>{body}</body></html>",
        escape_html(&t("assistant-title"))
    )
}

/// One `label value` line of the settings section, both escaped.
fn setting_line(body: &mut String, label: String, value: &str) {
    body.push_str(&format!(
        "<p class=\"setting\">{} {}</p>",
        escape_html(&label),
        escape_html(value)
    ));
}

fn push_settings(
    body: &mut String,
    settings: &SettingsView,
    file: Option<&Path>,
    t: &dyn Fn(&str) -> String,
) {
    body.push_str(&format!(
        "<div class=\"settings\"><h2>{}</h2>",
        escape_html(&t("settings-heading"))
    ));
    match settings {
        SettingsView::Unspecified => body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            escape_html(&t("settings-none"))
        )),
        SettingsView::Missing => body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            escape_html(&t("settings-missing"))
        )),
        SettingsView::Invalid(reason) => body.push_str(&format!(
            "<p class=\"error\">{} {}</p>",
            escape_html(&t("settings-invalid")),
            escape_html(reason)
        )),
        SettingsView::Loaded(settings) => push_loaded_settings(body, settings, t),
    }
    if let Some(file) = file {
        setting_line(body, t("settings-file"), &file.display().to_string());
        body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            escape_html(&t("settings-apply"))
        ));
    }
    body.push_str("</div>");
}

fn push_loaded_settings(
    body: &mut String,
    settings: &AssistantSettings,
    t: &dyn Fn(&str) -> String,
) {
    let backend = match settings.backend {
        BackendKind::None => t("backend-none"),
        BackendKind::Loopback => t("backend-loopback"),
        BackendKind::Candle => t("backend-candle"),
        BackendKind::Both => t("backend-both"),
    };
    setting_line(body, t("settings-backend"), &backend);
    if let Some(loopback) = &settings.loopback {
        setting_line(
            body,
            t("settings-loopback"),
            &format!(
                "{} · {} · {}",
                loopback.provider, loopback.base_url, loopback.model
            ),
        );
    }
    if let Some(candle) = &settings.candle {
        setting_line(body, t("settings-candle-model"), &candle.model_path);
        setting_line(body, t("settings-candle-tokenizer"), &candle.tokenizer_path);
        setting_line(
            body,
            t("settings-candle-context"),
            &candle.context.to_string(),
        );
    }
    setting_line(
        body,
        t("settings-idle"),
        &settings.idle_timeout_secs.to_string(),
    );
    let memory = settings
        .max_resident_mb
        .map_or_else(|| t("settings-memory-none"), |mb| mb.to_string());
    setting_line(body, t("settings-memory"), &memory);
    setting_line(body, t("settings-nice"), &settings.nice.to_string());
}

fn push_entry(body: &mut String, entry: &PanelEntry, t: &dyn Fn(&str) -> String) {
    let kind = match entry.kind {
        PanelKind::Summary => t("kind-summary"),
        PanelKind::Organized => t("kind-organized"),
    };
    body.push_str(&format!(
        "<div class=\"entry\"><p class=\"kind\">{}</p>",
        escape_html(&kind)
    ));
    if let Some(url) = &entry.source_url {
        body.push_str(&format!(
            "<p class=\"meta\">{} {}</p>",
            escape_html(&t("label-source")),
            escape_html(url)
        ));
    }
    if let Some(request) = &entry.request {
        body.push_str(&format!(
            "<p class=\"meta\">{} {}</p>",
            escape_html(&t("label-request")),
            escape_html(request)
        ));
    }
    match &entry.outcome {
        Ok(text) => {
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                body.push_str(&format!("<p>{}</p>", escape_html(line)));
            }
        }
        Err(reason) => body.push_str(&format!(
            "<p class=\"error\">{} {}</p>",
            escape_html(&t("label-failed")),
            escape_html(reason)
        )),
    }
    body.push_str("</div>");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page with no settings section input, for tests about entries.
    fn render(entries: &[PanelEntry], available: bool, locale: &str) -> String {
        assistant_html(entries, available, &SettingsView::Unspecified, None, locale)
    }

    fn settings_page(view: &SettingsView, file: Option<&Path>) -> String {
        assistant_html(&[], true, view, file, "en")
    }

    fn loaded() -> AssistantSettings {
        use blueice_assistant_settings::{CandleSettings, LoopbackSettings};
        AssistantSettings {
            backend: BackendKind::Both,
            loopback: Some(LoopbackSettings {
                provider: "llamacpp".into(),
                base_url: "http://127.0.0.1:8080/v1/".into(),
                model: "local".into(),
            }),
            candle: Some(CandleSettings {
                model_path: "/models/qwen3.gguf".into(),
                tokenizer_path: "/models/tokenizer.json".into(),
                context: 4096,
            }),
            max_resident_mb: Some(2048),
            nice: 15,
            ..AssistantSettings::default()
        }
    }

    fn entry(kind: PanelKind, outcome: Result<&str, &str>) -> PanelEntry {
        PanelEntry {
            id: 1,
            kind,
            source_url: Some("https://example.com/a".into()),
            request: None,
            outcome: outcome.map(str::to_string).map_err(str::to_string),
        }
    }

    #[test]
    fn the_url_forms_are_recognized() {
        assert!(is_assistant_url("about:assistant"));
        assert!(is_assistant_url("about:assistant?lang=zh-TW"));
        for other in [
            "about:assistants",
            "about:downloads",
            "https://about:assistant",
        ] {
            assert!(!is_assistant_url(other), "{other}");
        }
    }

    #[test]
    fn an_unconfigured_assistant_is_said_plainly_and_hides_any_entries() {
        let html = render(
            &[entry(PanelKind::Summary, Ok("STALE-RESULT"))],
            false,
            "en",
        );
        assert!(html.contains("No local assistant is configured"));
        assert!(!html.contains("STALE-RESULT"));
    }

    #[test]
    fn an_empty_panel_says_how_to_start() {
        let html = render(&[], true, "en");
        assert!(html.contains("Nothing here yet"));
    }

    #[test]
    fn a_result_is_labelled_attributed_and_split_into_paragraphs() {
        let mut organized = entry(PanelKind::Organized, Ok("row one\n\n  row two  \n"));
        organized.request = Some("make a table".into());
        let html = render(&[organized], true, "en");
        assert!(html.contains("Organized data"));
        assert!(html.contains("Written by a local model"));
        assert!(html.contains("Page: https://example.com/a"));
        assert!(html.contains("Request: make a table"));
        assert!(html.contains("<p>row one</p><p>row two</p>"));
    }

    #[test]
    fn a_failure_is_shown_with_its_reason() {
        let html = render(
            &[entry(
                PanelKind::Summary,
                Err("the assistant is not running"),
            )],
            true,
            "en",
        );
        assert!(html.contains("Could not complete this request: the assistant is not running"));
    }

    #[test]
    fn model_output_and_page_urls_are_never_interpreted_as_html() {
        let mut hostile = entry(
            PanelKind::Summary,
            Ok("<script>alert(1)</script><a href=\"https://evil\">x</a>"),
        );
        hostile.source_url = Some("https://e.example/\"><img src=x>".into());
        hostile.request = Some("<b>bold</b>".into());
        let html = render(&[hostile], true, "en");
        assert!(!html.contains("<script"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("<b>bold"));
        assert!(!html.contains("<a href"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_locale_is_honored_and_an_unknown_one_falls_back() {
        let zh = render(&[], true, "zh-TW");
        assert!(zh.contains("目前沒有內容"));
        let fallback = render(&[], true, "xx");
        assert!(fallback.contains("Nothing here yet"));
    }

    #[test]
    fn the_panel_keeps_the_newest_results_and_drops_the_oldest() {
        let panel = AssistantPanel::new();
        for n in 0..MAX_ENTRIES + 3 {
            panel.push(PanelKind::Summary, None, None, Ok(format!("result {n}")));
        }
        let entries = panel.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(
            entries[0].outcome,
            Ok(format!("result {}", MAX_ENTRIES + 2))
        );
        assert_eq!(entries[MAX_ENTRIES - 1].outcome, Ok("result 3".to_string()));
        // Ids increase monotonically even as old entries drop.
        assert!(entries[0].id > entries[1].id);
    }

    #[test]
    fn availability_is_a_shared_flag() {
        let panel = AssistantPanel::new();
        assert!(!panel.is_available());
        panel.set_available(true);
        assert!(panel.is_available());
    }

    #[test]
    fn loaded_settings_show_every_field_including_both_backends_and_the_limits() {
        let html = settings_page(
            &SettingsView::Loaded(Box::new(loaded())),
            Some(Path::new(
                "/home/me/.config/blueice/assistant-settings.json",
            )),
        );
        for expected in [
            "Backend: Both at once (double the resources)",
            "Loopback model: llamacpp · http://127.0.0.1:8080/v1/ · local",
            "Candle model: /models/qwen3.gguf",
            "Candle tokenizer: /models/tokenizer.json",
            "Candle context (tokens): 4096",
            "Idle timeout (seconds): 600",
            "Memory ceiling (MiB): 2048",
            "Priority (nice): 15",
            "File: /home/me/.config/blueice/assistant-settings.json",
            "take effect when the launcher next starts",
        ] {
            assert!(html.contains(expected), "missing {expected:?} in {html}");
        }
    }

    #[test]
    fn unused_sections_and_an_unlimited_ceiling_are_shown_honestly() {
        let plain = AssistantSettings::default();
        let html = settings_page(&SettingsView::Loaded(Box::new(plain)), None);
        assert!(html.contains("Backend: None (no assistant)"));
        assert!(html.contains("Memory ceiling (MiB): No limit"));
        assert!(!html.contains("Loopback model:"));
        assert!(!html.contains("Candle model:"));
        assert!(!html.contains("File:"), "no file was named");
    }

    #[test]
    fn the_absence_of_settings_is_said_in_words() {
        assert!(
            settings_page(&SettingsView::Unspecified, None).contains("No settings file was given")
        );
        assert!(
            settings_page(&SettingsView::Missing, Some(Path::new("/x.json")))
                .contains("does not exist")
        );
        let invalid = settings_page(&SettingsView::Invalid("nice must be 0 to 19".into()), None);
        assert!(invalid.contains("could not be used: nice must be 0 to 19"));
    }

    #[test]
    fn the_settings_section_is_shown_even_when_no_assistant_is_available() {
        // Where a person learns *why* nothing is available.
        let html = assistant_html(&[], false, &SettingsView::Missing, None, "en");
        assert!(html.contains("No local assistant is configured"));
        assert!(html.contains("does not exist"));
    }

    #[test]
    fn settings_values_from_a_hand_edited_file_are_never_interpreted_as_html() {
        let mut hostile = loaded();
        hostile.loopback.as_mut().unwrap().model = "<script>alert(1)</script>".into();
        let html = settings_page(
            &SettingsView::Loaded(Box::new(hostile)),
            Some(Path::new("/tmp/<img src=x>.json")),
        );
        assert!(!html.contains("<script"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;"));
        let invalid = settings_page(&SettingsView::Invalid("<b>bad</b>".into()), None);
        assert!(!invalid.contains("<b>bad"));
    }

    #[test]
    fn the_settings_section_follows_the_locale() {
        let html = assistant_html(&[], true, &SettingsView::Missing, None, "zh-TW");
        assert!(html.contains("設定檔不存在"));
    }

    #[test]
    fn reading_the_view_never_writes_and_distinguishes_every_case() {
        assert_eq!(read_settings_view(None), SettingsView::Unspecified);
        let dir = std::env::temp_dir().join(format!("as-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        assert_eq!(read_settings_view(Some(&path)), SettingsView::Missing);
        assert!(!path.exists(), "reading must not create the file");

        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            read_settings_view(Some(&path)),
            SettingsView::Invalid(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not json",
            "and must not rewrite it"
        );

        blueice_assistant_settings::save(&path, &loaded()).unwrap();
        assert_eq!(
            read_settings_view(Some(&path)),
            SettingsView::Loaded(Box::new(loaded()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_settings_file_is_a_shared_panel_setting() {
        let panel = AssistantPanel::new();
        assert_eq!(panel.settings_file(), None);
        panel.set_settings_file(Some(PathBuf::from("/x.json")));
        assert_eq!(panel.settings_file(), Some(PathBuf::from("/x.json")));
    }
}
