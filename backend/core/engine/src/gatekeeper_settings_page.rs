// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The built-in `about:settings` view for the always-resident Phase 7
//! gatekeeper. It reads the actual private gatekeeper service rather than
//! duplicating a release manifest in `core`, so the human-visible policy and
//! the process enforcing it cannot drift apart.

use crate::downloads_page::escape_html;
use blueice_ipc::gatekeeper::{
    default_gatekeeper_socket_path, read_gatekeeper_settings_reply,
    write_gatekeeper_settings_request, GatekeeperSettings, GatekeeperSettingsChange,
    GatekeeperSettingsReply, GatekeeperSettingsRequest,
};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub const GATEKEEPER_SETTINGS_URL: &str = "about:settings";

pub fn is_gatekeeper_settings_url(url: &str) -> bool {
    url == GATEKEEPER_SETTINGS_URL || url.starts_with("about:settings?")
}

const SETTINGS_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsNotice {
    Saved,
    Rejected(String),
}

pub enum GatekeeperSettingsView {
    Settings(GatekeeperSettings),
    Unavailable,
}

/// A bounded client of the private gatekeeper settings socket. It is separate
/// from navigation's fail-closed review client: an unavailable settings page
/// must not stall or change the enforcement workflow.
pub struct GatekeeperSettingsSource {
    socket: PathBuf,
}

impl GatekeeperSettingsSource {
    pub fn new() -> Self {
        Self::at(default_gatekeeper_socket_path())
    }

    pub fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    pub fn fetch(&self) -> Result<GatekeeperSettings, String> {
        match self.exchange(GatekeeperSettingsRequest::Read)? {
            GatekeeperSettingsReply::Settings(settings) => Ok(settings),
            GatekeeperSettingsReply::Rejected { reason } => Err(reason),
        }
    }

    pub fn update(&self, change: GatekeeperSettingsChange) -> Result<GatekeeperSettings, String> {
        match self.exchange(GatekeeperSettingsRequest::Update { change })? {
            GatekeeperSettingsReply::Settings(settings) => Ok(settings),
            GatekeeperSettingsReply::Rejected { reason } => Err(reason),
        }
    }

    fn exchange(
        &self,
        request: GatekeeperSettingsRequest,
    ) -> Result<GatekeeperSettingsReply, String> {
        let mut stream = UnixStream::connect(&self.socket).map_err(|error| {
            format!(
                "connecting to the gatekeeper settings service {}: {error}",
                self.socket.display()
            )
        })?;
        stream
            .set_read_timeout(Some(SETTINGS_TIMEOUT))
            .map_err(|error| format!("setting gatekeeper settings read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(SETTINGS_TIMEOUT))
            .map_err(|error| format!("setting gatekeeper settings write timeout: {error}"))?;
        write_gatekeeper_settings_request(&mut stream, &request)
            .map_err(|error| format!("sending gatekeeper settings request: {error}"))?;
        let reply = read_gatekeeper_settings_reply(&mut stream)
            .map_err(|error| format!("reading gatekeeper settings reply: {error}"));
        let _ = stream.shutdown(Shutdown::Both);
        reply
    }

    #[cfg(test)]
    pub fn without_default(socket: impl AsRef<Path>) -> Self {
        Self::at(socket.as_ref().to_path_buf())
    }
}

impl Default for GatekeeperSettingsSource {
    fn default() -> Self {
        Self::new()
    }
}

const STYLE: &str = "body { padding: 12px; font-size: 15px; color: #222222; } \
    .note { padding: 8px; border-width: 1px; border-style: solid; border-color: #b8860b; background-color: #fff9db; } \
    .section { margin-top: 14px; } \
    .item { margin-top: 8px; padding: 8px; border-width: 1px; border-style: solid; border-color: #cccccc; } \
    .name { font-weight: bold; } \
    .detail { color: #444444; margin-top: 3px; } \
    .mandatory { color: #8b0000; font-weight: bold; } \
    .adjustable { color: #236b2a; font-weight: bold; } \
    .error { color: #b00020; } \
    button { margin-left: 6px; }";

/// Generates a full, escaped snapshot of the policy the running gatekeeper
/// reports. Rejection details are bounded and HTML-escaped before they become
/// privileged-page text, so a user can correct an invalid local-model base.
pub fn gatekeeper_settings_html(
    view: &GatekeeperSettingsView,
    locale: &str,
    notice: Option<SettingsNotice>,
) -> String {
    let locale = if blueice_i18n::SUPPORTED_LOCALES.contains(&locale) {
        locale
    } else {
        blueice_i18n::DEFAULT_LOCALE
    };
    let t = |key: &str| blueice_i18n::translate(locale, "gatekeeper-settings", key, &[]);
    let mut body = format!(
        "<h1>{}</h1><p>{}</p>",
        escape_html(&t("title")),
        escape_html(&t("intro"))
    );
    body.push_str(&format!(
        "<p class=\"note\">{}</p>",
        escape_html(&t("locked-note"))
    ));
    if let Some(notice) = notice {
        let (class, message) = match notice {
            SettingsNotice::Saved => ("adjustable", t("saved")),
            SettingsNotice::Rejected(reason) => (
                "error",
                format!("{} {}", t("invalid"), reason.chars().take(240).collect::<String>()),
            ),
        };
        body.push_str(&format!(
            "<p class=\"{class}\">{}</p>",
            escape_html(&message)
        ));
    }
    match view {
        GatekeeperSettingsView::Unavailable => body.push_str(&format!(
            "<p class=\"error\">{}</p>",
            escape_html(&t("unavailable"))
        )),
        GatekeeperSettingsView::Settings(settings) => {
            body.push_str(&format!(
                "<p>{} <b>{}</b></p>",
                escape_html(&t("ruleset-version")),
                escape_html(&settings.ruleset_version)
            ));
            body.push_str(&format!(
                "<p>{} <b>{}</b></p>",
                escape_html(&t("model-review")),
                escape_html(&t(if settings.model_review_active { "model-active" } else { "model-inactive" })),
            ));
            let provider = settings.local_model.as_ref().map(|config| config.provider.as_str()).unwrap_or("ollama");
            let base_url = settings.local_model.as_ref().map(|config| config.base_url.as_str()).unwrap_or("http://127.0.0.1:11434/v1/");
            let model = settings.local_model.as_ref().map(|config| config.model.as_str()).unwrap_or("");
            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2><p class=\"note\">{}</p><label for=\"gatekeeper-model-provider\">{}</label><input id=\"gatekeeper-model-provider\" type=\"text\" value=\"{}\"><label for=\"gatekeeper-model-base\">{}</label><input id=\"gatekeeper-model-base\" type=\"text\" value=\"{}\"><label for=\"gatekeeper-model-name\">{}</label><input id=\"gatekeeper-model-name\" type=\"text\" value=\"{}\"><button data-gatekeeper-action=\"configure-model\">{}</button>",
                escape_html(&t("local-model-heading")),
                escape_html(&t("local-model-warning")),
                escape_html(&t("local-model-provider")), escape_html(provider),
                escape_html(&t("local-model-base")), escape_html(base_url),
                escape_html(&t("local-model-name")), escape_html(model),
                escape_html(&t("local-model-save")),
            ));
            if settings.local_model.is_some() {
                body.push_str(&format!(
                    "<button data-gatekeeper-action=\"disable-model\">{}</button>",
                    escape_html(&t("local-model-disable"))
                ));
            }
            body.push_str("</div>");
            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2>",
                escape_html(&t("baseline-rules-heading"))
            ));
            for rule in &settings.baseline_rules {
                let (status_class, status_label) = if rule.mandatory {
                    ("mandatory", t("mandatory"))
                } else {
                    ("adjustable", t("adjustable"))
                };
                body.push_str(&format!(
                    "<div class=\"item\"><p class=\"name\">{}</p><p class=\"{status_class}\">{}</p><p class=\"detail\">{} {}</p><p class=\"detail\">{}</p>",
                    escape_html(&rule.id),
                    escape_html(&status_label),
                    escape_html(&t("category")),
                    escape_html(&rule.category),
                    escape_html(&rule.description),
                ));
                body.push_str(&format!(
                    "<p class=\"detail\">{}</p><ul>",
                    escape_html(&t("conditions"))
                ));
                for condition in &rule.conditions {
                    body.push_str(&format!("<li><code>{}</code></li>", escape_html(condition)));
                }
                body.push_str("</ul>");
                if !rule.match_logic.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{} {}</p>",
                        escape_html(&t("match-logic")),
                        escape_html(&rule.match_logic)
                    ));
                }
                if !rule.workflow_steps.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{} {}</p>",
                        escape_html(&t("applies-at")),
                        escape_html(&rule.workflow_steps.join(", "))
                    ));
                }
                body.push_str("</div>");
            }
            body.push_str("</div>");

            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2>",
                escape_html(&t("workflow-heading"))
            ));
            for step in &settings.workflow {
                let (status_class, status_label) = if step.mandatory {
                    ("mandatory", t("mandatory"))
                } else {
                    ("adjustable", t("adjustable"))
                };
                body.push_str(&format!(
                    "<div class=\"item\" data-gatekeeper-step=\"{}\"><p class=\"name\">{}</p><p class=\"{status_class}\">{}</p><p class=\"detail\">{} {}</p><p class=\"detail\">{}</p>",
                    escape_html(&step.id),
                    escape_html(&step.id),
                    escape_html(&status_label),
                    escape_html(&t("trigger")),
                    escape_html(&step.trigger),
                    escape_html(&step.description),
                ));
                if !step.failure_behavior.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{} {}</p>",
                        escape_html(&t("failure-behavior")),
                        escape_html(&step.failure_behavior)
                    ));
                }
                if !step.review_order.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{}</p><ol>",
                        escape_html(&t("review-order"))
                    ));
                    for layer in &step.review_order {
                        let label = match layer.as_str() {
                            "compiled-rule-base" => t("review-layer-compiled"),
                            "user-blocked-hosts" => t("review-layer-hosts"),
                            "user-blocked-html-phrases" => t("review-layer-html-phrases"),
                            "user-blocked-downloads" => t("review-layer-downloads"),
                            "user-blocked-popup-phrases" => t("review-layer-popup-phrases"),
                            "local-model" => t("review-layer-model"),
                            _ => layer.clone(),
                        };
                        body.push_str(&format!("<li>{}</li>", escape_html(&label)));
                    }
                    body.push_str("</ol>");
                }
                let applied_rules: Vec<_> = settings
                    .baseline_rules
                    .iter()
                    .filter(|rule| rule.workflow_steps.iter().any(|id| id == &step.id))
                    .collect();
                if !applied_rules.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{}</p><ul>",
                        escape_html(&t("compiled-rules-at-step"))
                    ));
                    for rule in applied_rules {
                        body.push_str(&format!("<li><code>{}</code></li>", escape_html(&rule.id)));
                    }
                    body.push_str("</ul>");
                }
                if !step.active_user_conditions.is_empty() {
                    body.push_str(&format!(
                        "<p class=\"detail\">{}</p><ul>",
                        escape_html(&t("active-additions-at-step"))
                    ));
                    for condition in &step.active_user_conditions {
                        let kind = match condition.kind.as_str() {
                            "host" => t("addition-host"),
                            "phrase" => t("addition-html-phrase"),
                            "download-extension" => t("addition-download-extension"),
                            "popup-phrase" => t("addition-popup-phrase"),
                            _ => condition.kind.clone(),
                        };
                        body.push_str(&format!(
                            "<li>{}: <code>{}</code></li>",
                            escape_html(&kind),
                            escape_html(&condition.value)
                        ));
                    }
                    body.push_str("</ul>");
                }
                body.push_str("</div>");
            }
            body.push_str("</div>");

            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2><p class=\"adjustable\">{}</p><label for=\"gatekeeper-custom-host\">{}</label><input id=\"gatekeeper-custom-host\" type=\"text\" value=\"\"><button data-gatekeeper-action=\"add-host\">{}</button>",
                escape_html(&t("custom-blocklist-heading")),
                escape_html(&t("adjustable")),
                escape_html(&t("add-host-label")),
                escape_html(&t("add-host-button")),
            ));
            body.push_str(&format!("<p class=\"detail\">{}</p>", escape_html(&t("custom-blocklist-match"))));
            if settings.custom_blocked_hosts.is_empty() {
                body.push_str(&format!(
                    "<p>{}</p>",
                    escape_html(&t("custom-blocklist-empty"))
                ));
            } else {
                body.push_str("<ul>");
                for host in &settings.custom_blocked_hosts {
                    body.push_str(&format!(
                        "<li>{}<button data-gatekeeper-action=\"remove-host\" data-gatekeeper-host=\"{}\">{}</button></li>",
                        escape_html(host),
                        escape_html(host),
                        escape_html(&t("remove-host-button")),
                    ));
                }
                body.push_str("</ul>");
            }
            body.push_str("</div>");

            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2><p class=\"adjustable\">{}</p><label for=\"gatekeeper-custom-phrase\">{}</label><input id=\"gatekeeper-custom-phrase\" type=\"text\" value=\"\"><button data-gatekeeper-action=\"add-phrase\">{}</button>",
                escape_html(&t("custom-phrases-heading")),
                escape_html(&t("adjustable")),
                escape_html(&t("add-phrase-label")),
                escape_html(&t("add-phrase-button")),
            ));
            body.push_str(&format!("<p class=\"detail\">{}</p>", escape_html(&t("custom-phrases-match"))));
            if settings.custom_blocked_phrases.is_empty() {
                body.push_str(&format!("<p>{}</p>", escape_html(&t("custom-phrases-empty"))));
            } else {
                body.push_str("<ul>");
                for phrase in &settings.custom_blocked_phrases {
                    body.push_str(&format!(
                        "<li>{}<button data-gatekeeper-action=\"remove-phrase\" data-gatekeeper-phrase=\"{}\">{}</button></li>",
                        escape_html(phrase),
                        escape_html(phrase),
                        escape_html(&t("remove-phrase-button")),
                    ));
                }
                body.push_str("</ul>");
            }
            body.push_str("</div>");

            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2><p class=\"adjustable\">{}</p><label for=\"gatekeeper-custom-extension\">{}</label><input id=\"gatekeeper-custom-extension\" type=\"text\" value=\"\"><button data-gatekeeper-action=\"add-extension\">{}</button>",
                escape_html(&t("custom-extensions-heading")),
                escape_html(&t("adjustable")),
                escape_html(&t("add-extension-label")),
                escape_html(&t("add-extension-button")),
            ));
            body.push_str(&format!("<p class=\"detail\">{}</p>", escape_html(&t("custom-extensions-match"))));
            if settings.custom_blocked_download_extensions.is_empty() {
                body.push_str(&format!("<p>{}</p>", escape_html(&t("custom-extensions-empty"))));
            } else {
                body.push_str("<ul>");
                for extension in &settings.custom_blocked_download_extensions {
                    body.push_str(&format!(
                        "<li>{}<button data-gatekeeper-action=\"remove-extension\" data-gatekeeper-extension=\"{}\">{}</button></li>",
                        escape_html(extension),
                        escape_html(extension),
                        escape_html(&t("remove-extension-button")),
                    ));
                }
                body.push_str("</ul>");
            }
            body.push_str("</div>");

            body.push_str(&format!(
                "<div class=\"section\"><h2>{}</h2><p class=\"adjustable\">{}</p><label for=\"gatekeeper-custom-popup-phrase\">{}</label><input id=\"gatekeeper-custom-popup-phrase\" type=\"text\" value=\"\"><button data-gatekeeper-action=\"add-popup-phrase\">{}</button>",
                escape_html(&t("custom-popup-phrases-heading")),
                escape_html(&t("adjustable")),
                escape_html(&t("add-popup-phrase-label")),
                escape_html(&t("add-popup-phrase-button")),
            ));
            body.push_str(&format!("<p class=\"detail\">{}</p>", escape_html(&t("custom-popup-phrases-match"))));
            if settings.custom_blocked_popup_phrases.is_empty() {
                body.push_str(&format!("<p>{}</p>", escape_html(&t("custom-popup-phrases-empty"))));
            } else {
                body.push_str("<ul>");
                for phrase in &settings.custom_blocked_popup_phrases {
                    body.push_str(&format!(
                        "<li>{}<button data-gatekeeper-action=\"remove-popup-phrase\" data-gatekeeper-popup-phrase=\"{}\">{}</button></li>",
                        escape_html(phrase),
                        escape_html(phrase),
                        escape_html(&t("remove-popup-phrase-button")),
                    ));
                }
                body.push_str("</ul>");
            }
            body.push_str("</div>");
        }
    }
    format!(
        "<html><head><title>{}</title><style>{STYLE}</style></head><body>{body}</body></html>",
        escape_html(&t("title"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::gatekeeper::{GatekeeperLocalModel, GatekeeperRuleInfo, GatekeeperUserCondition, GatekeeperWorkflowStep};

    fn settings() -> GatekeeperSettings {
        let condition = |kind: &str, value: &str| GatekeeperUserCondition {
            kind: kind.to_string(), value: value.to_string(),
        };
        let mut workflow = vec![GatekeeperWorkflowStep {
            id: "url-before-fetch".to_string(),
            trigger: "Every navigation".to_string(),
            description: "Review first.".to_string(),
            failure_behavior: "Block <unsafe> navigation.".to_string(),
            review_order: vec!["compiled-rule-base".to_string(), "User <unsafe> rules".to_string()],
            active_user_conditions: vec![condition("host", "tracker.example")],
            mandatory: true,
        }];
        for id in [
            "content-before-parse",
            "download-before-bytes",
            "extension-before-side-effect",
            "extension-popup-before-publish",
        ] {
            workflow.push(GatekeeperWorkflowStep {
                id: id.to_string(),
                trigger: "Review before effect".to_string(),
                description: "Review first.".to_string(),
                failure_behavior: "Block the effect.".to_string(),
                review_order: vec!["compiled-rule-base".to_string()],
                active_user_conditions: match id {
                    "content-before-parse" => vec![condition("phrase", "ignore <instructions>")],
                    "download-before-bytes" => vec![condition("host", "tracker.example"), condition("download-extension", ".zip")],
                    "extension-popup-before-publish" => vec![condition("popup-phrase", "send <secrets>")],
                    _ => Vec::new(),
                },
                mandatory: true,
            });
        }
        GatekeeperSettings {
            ruleset_version: "2026.09.23.1".to_string(),
            model_review_active: false,
            local_model: None,
            baseline_rules: vec![GatekeeperRuleInfo {
                id: "domain-rule".to_string(),
                category: "known-bad-domain".to_string(),
                description: "Blocks <unsafe> text.".to_string(),
                conditions: vec!["<unsafe>".to_string()],
                match_logic: "one <unsafe> condition".to_string(),
                workflow_steps: vec!["url-before-fetch".to_string()],
                mandatory: true,
            }],
            workflow,
            custom_blocked_hosts: vec!["tracker.example".to_string()],
            custom_blocked_phrases: vec!["ignore <instructions>".to_string()],
            custom_blocked_download_extensions: vec![".zip".to_string()],
            custom_blocked_popup_phrases: vec!["send <secrets>".to_string()],
        }
    }

    #[test]
    fn settings_page_shows_the_complete_policy_and_escaped_adjustment_controls() {
        let html = gatekeeper_settings_html(
            &GatekeeperSettingsView::Settings(settings()),
            "en",
            Some(SettingsNotice::Saved),
        );
        assert!(html.contains("AI Gatekeeper settings"));
        assert!(html.contains("2026.09.23.1"));
        assert!(html.contains("domain-rule") && html.contains("url-before-fetch"));
        assert!(html.contains("tracker.example"));
        assert!(html.contains("data-gatekeeper-action=\"add-host\""));
        assert!(html.contains("data-gatekeeper-action=\"configure-model\""));
        assert!(!html.contains("data-gatekeeper-action=\"disable-model\""));
        assert!(html.contains("data-gatekeeper-action=\"remove-host\""));
        assert!(html.contains("data-gatekeeper-action=\"add-phrase\""));
        assert!(html.contains("data-gatekeeper-action=\"remove-phrase\""));
        assert!(html.contains("data-gatekeeper-action=\"add-extension\""));
        assert!(html.contains("data-gatekeeper-action=\"remove-extension\""));
        assert!(html.contains("data-gatekeeper-action=\"add-popup-phrase\""));
        assert!(html.contains("data-gatekeeper-action=\"remove-popup-phrase\""));
        assert!(html.contains("Blocks &lt;unsafe&gt; text."));
        assert!(html.contains("&lt;unsafe&gt;"));
        assert!(html.contains("one &lt;unsafe&gt; condition"));
        assert!(html.contains("Block &lt;unsafe&gt; navigation."));
        assert!(html.contains("Active review order:"));
        assert!(html.contains("<li>Compiled deterministic rule base (required)</li><li>User &lt;unsafe&gt; rules</li>"));
        assert!(html.contains("Applied at workflow steps: url-before-fetch"));
        assert!(html.contains("Compiled rules at this step:</p><ul><li><code>domain-rule</code></li></ul>"));
        assert!(html.contains("A host also blocks its dot-boundary subdomains"));
        assert!(html.contains("ignore &lt;instructions&gt;"));
        assert!(html.contains("send &lt;secrets&gt;"));
        let step = |id: &str| {
            let marker = format!("data-gatekeeper-step=\"{id}\"");
            html.split(&marker).nth(1).unwrap().split("</div>").next().unwrap().to_string()
        };
        assert!(step("url-before-fetch").contains("Host: <code>tracker.example</code>"));
        assert!(step("content-before-parse").contains("HTML-text phrase: <code>ignore &lt;instructions&gt;</code>"));
        assert!(!step("content-before-parse").contains("tracker.example"));
        assert!(step("download-before-bytes").contains("Host: <code>tracker.example</code>"));
        assert!(step("download-before-bytes").contains("Download extension: <code>.zip</code>"));
        assert!(step("extension-popup-before-publish").contains("Extension-popup phrase: <code>send &lt;secrets&gt;</code>"));
        assert!(!step("extension-before-side-effect").contains("Your active blocking conditions"));
    }

    #[test]
    fn settings_page_shows_active_local_model_and_escaped_configuration() {
        let mut config = settings();
        config.model_review_active = true;
        config.local_model = Some(GatekeeperLocalModel {
            provider: "huggingface".into(),
            base_url: "http://127.0.0.1:8080/v1/".into(),
            model: "local<model>".into(),
        });
        let html = gatekeeper_settings_html(&GatekeeperSettingsView::Settings(config), "en", None);
        assert!(html.contains("value=\"huggingface\""));
        assert!(html.contains("value=\"local&lt;model&gt;\""));
        assert!(html.contains("data-gatekeeper-action=\"disable-model\""));
        assert!(html.contains("timeout, malformed reply, oversized input, or unavailable local service blocks"));
    }

    #[test]
    fn settings_page_localizes_the_enforced_review_order() {
        let mut config = settings();
        config.workflow[0].review_order = vec![
            "compiled-rule-base".into(),
            "user-blocked-hosts".into(),
            "local-model".into(),
        ];
        let html = gatekeeper_settings_html(&GatekeeperSettingsView::Settings(config), "zh-TW", None);
        assert!(html.contains("目前生效的審查順序："));
        assert!(html.contains("<li>編譯內建確定性規則（必要）</li><li>您封鎖的主機</li><li>選用的本機模型（無法審查時會阻擋）</li>"));
        assert!(html.contains("此步驟套用的內建規則：</p><ul><li><code>domain-rule</code></li></ul>"));
        assert!(html.contains("此步驟生效的自訂封鎖條件："));
    }

    #[test]
    fn rejected_model_configuration_shows_an_escaped_reason() {
        let html = gatekeeper_settings_html(
            &GatekeeperSettingsView::Settings(settings()), "en",
            Some(SettingsNotice::Rejected("invalid <remote> endpoint".into())),
        );
        assert!(html.contains("invalid &lt;remote&gt; endpoint"));
        assert!(!html.contains("<remote>"));
    }

    #[test]
    fn unavailable_settings_page_never_claims_a_policy_was_loaded() {
        let html = gatekeeper_settings_html(&GatekeeperSettingsView::Unavailable, "zh-TW", None);
        assert!(html.contains("AI 把關程式無法使用"));
        assert!(!html.contains("data-gatekeeper-action"));
    }

    #[test]
    fn source_reads_and_updates_the_private_gatekeeper_service() {
        use blueice_ai_gatekeeper::GatekeeperService;
        use std::os::unix::net::UnixListener;
        use std::sync::Arc;
        use std::thread;

        let socket = blueice_ipc::local_socket::default_socket_dir()
            .join(format!("gss-{}", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let service = Arc::new(GatekeeperService::new(None).unwrap());
        let worker = thread::spawn({
            let service = service.clone();
            move || {
                for _ in 0..2 {
                    let (mut stream, _) = listener.accept().unwrap();
                    service.handle_connection(&mut stream).unwrap();
                }
            }
        });
        let source = GatekeeperSettingsSource::without_default(&socket);
        let live = source.fetch().unwrap();
        assert!(live.custom_blocked_hosts.is_empty());
        assert!(live.baseline_rules.iter().all(|rule| {
            rule.mandatory && !rule.match_logic.is_empty() && !rule.workflow_steps.is_empty()
        }));
        assert!(live.workflow.iter().all(|step| {
            step.mandatory
                && !step.failure_behavior.is_empty()
                && step.review_order == ["compiled-rule-base"]
        }));
        let updated = source
                .update(GatekeeperSettingsChange::AddBlockedHost {
                    host: "tracker.example".to_string(),
                })
                .unwrap();
        assert_eq!(updated.custom_blocked_hosts, vec!["tracker.example"]);
        assert_eq!(updated.workflow[0].review_order, ["compiled-rule-base", "user-blocked-hosts"]);
        assert_eq!(updated.workflow[0].active_user_conditions, [GatekeeperUserCondition {
            kind: "host".to_string(), value: "tracker.example".to_string(),
        }]);
        worker.join().unwrap();
        let _ = std::fs::remove_file(socket);
    }
}
