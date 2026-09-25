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
//! State lives in one [`AssistantPanel`] shared by every tab (a `core`-wide
//! list, newest first, bounded at [`MAX_ENTRIES`]); the page itself is a pure
//! function of that list.

use crate::downloads_page::escape_html;
use std::collections::VecDeque;
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

const STYLE: &str = "body { padding: 12px; font-size: 15px; color: #222222; } \
    .note { color: #666666; } \
    .entry { margin-top: 14px; padding: 8px; border-width: 1px; border-style: solid; border-color: #cccccc; } \
    .kind { font-weight: bold; } \
    .meta { color: #555555; font-size: 13px; margin-top: 3px; } \
    .error { color: #b00020; }";

/// Builds the panel for `locale`, every string through `blueice-i18n`'s
/// `assistant` namespace and everything from outside escaped.
pub fn assistant_html(entries: &[PanelEntry], available: bool, locale: &str) -> String {
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
    format!(
        "<html><head><title>{}</title><style>{STYLE}</style></head><body>{body}</body></html>",
        escape_html(&t("assistant-title"))
    )
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
        let html = assistant_html(&[entry(PanelKind::Summary, Ok("STALE-RESULT"))], false, "en");
        assert!(html.contains("No local assistant is configured"));
        assert!(!html.contains("STALE-RESULT"));
    }

    #[test]
    fn an_empty_panel_says_how_to_start() {
        let html = assistant_html(&[], true, "en");
        assert!(html.contains("Nothing here yet"));
    }

    #[test]
    fn a_result_is_labelled_attributed_and_split_into_paragraphs() {
        let mut organized = entry(PanelKind::Organized, Ok("row one\n\n  row two  \n"));
        organized.request = Some("make a table".into());
        let html = assistant_html(&[organized], true, "en");
        assert!(html.contains("Organized data"));
        assert!(html.contains("Written by a local model"));
        assert!(html.contains("Page: https://example.com/a"));
        assert!(html.contains("Request: make a table"));
        assert!(html.contains("<p>row one</p><p>row two</p>"));
    }

    #[test]
    fn a_failure_is_shown_with_its_reason() {
        let html = assistant_html(
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
        let html = assistant_html(&[hostile], true, "en");
        assert!(!html.contains("<script"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("<b>bold"));
        assert!(!html.contains("<a href"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_locale_is_honored_and_an_unknown_one_falls_back() {
        let zh = assistant_html(&[], true, "zh-TW");
        assert!(zh.contains("目前沒有內容"));
        let fallback = assistant_html(&[], true, "xx");
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
}
