// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic, locally-versioned safety rules for `ai-gatekeeper`.
//!
//! This module intentionally has no model, prompt, network request, or shared
//! mutable state. It is the independent rule-base layer Phase 7 requires: a
//! malicious page cannot influence how these checks interpret it by writing
//! instructions for an LLM. Rules ship with the binary under a stable version
//! identifier; updating them is an ordinary signed/reviewed BlueIce release,
//! not a live remote policy download that an attacker could replace.

use blueice_ipc::gatekeeper::{
    GatekeeperLocalModel, GatekeeperReply, GatekeeperRequest, GatekeeperRuleInfo, GatekeeperSettings,
    GatekeeperWorkflowStep, GatekeeperUserCondition,
};

/// Version carried in diagnostics and release notes for this compiled rule
/// set. Keep it monotonic whenever a detection decision changes.
pub const RULESET_VERSION: &str = "2026.09.24.6";

const KNOWN_MALICIOUS_HOSTS: &[&str] = &["malware.test", "phishing.test"];
const BIDI_OVERRIDE_CODEPOINTS: &[&str] = &["U+202A–U+202E", "U+2066–U+2069"];
const ZERO_WIDTH_CODEPOINTS: &[&str] = &["U+200B–U+200D", "U+2060", "U+FEFF"];
const PROMPT_INJECTION_PHRASES: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "disregard previous instructions",
    "override the previous instructions",
];
const DANGEROUS_EXTENSIONS: &[&str] = &[".exe", ".msi", ".bat", ".cmd", ".ps1", ".scr", ".apk"];
const DANGEROUS_CONTENT_TYPES: &[&str] = &[
    "application/x-msdownload",
    "application/x-dosexec",
    "application/vnd.microsoft.portable-executable",
    "application/x-msi",
];
const SENSITIVE_INPUT_TYPES: &[&str] = &["password", "credit-card", "payment"];
const POPUP_BLOCKED_PHRASES: &[&str] = &[
    "password", "seed phrase", "recovery phrase", "one-time code", "credit card",
    "disable gatekeeper", "http://", "https://",
];
// Toolbar labels are only 20 characters, so use high-precision phrases that
// can actually fit in that constrained native UI surface. A generic word
// like "password" would wrongly reject a legitimate password-manager button.
const TOOLBAR_BLOCKED_PHRASES: &[&str] = &[
    "enter password", "seed phrase", "recovery phrase", "one-time code",
    "disable gatekeeper",
];
const HIDDEN_CONTENT_MARKERS: &[&str] = &[
    "aria-hidden=\"true\"", "aria-hidden='true'", "display:none",
    "visibility:hidden", "opacity:0", "left:-999",
];

fn signatures(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// The complete compiled rule manifest. It is deliberately returned as data
/// for `about:settings`, rather than duplicating prose in the UI: the page and
/// the enforcement process therefore disclose the same release-owned policy.
pub fn baseline_rules() -> Vec<GatekeeperRuleInfo> {
    vec![
        GatekeeperRuleInfo {
            id: "known-malicious-domain".to_string(),
            category: "known-bad-domain".to_string(),
            description: "Blocks the compiled local denylist, including malware.test and phishing.test, before a network fetch.".to_string(),
            conditions: signatures(KNOWN_MALICIOUS_HOSTS),
            match_logic: "Exact host or dot-boundary subdomain match, case-insensitive; any listed host rejects.".to_string(),
            workflow_steps: vec!["url-before-fetch".to_string(), "download-before-bytes".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "url-unicode-obfuscation".to_string(),
            category: "unicode-bidi-override / url-obfuscation".to_string(),
            description: "Blocks bidirectional overrides and invisible Unicode characters in a URL.".to_string(),
            conditions: [BIDI_OVERRIDE_CODEPOINTS, ZERO_WIDTH_CODEPOINTS].concat().iter().map(|item| (*item).to_string()).collect(),
            match_logic: "Any listed character in a URL rejects. The content stage also rechecks both URL bidi overrides and invisible characters after redirects.".to_string(),
            workflow_steps: vec!["url-before-fetch".to_string(), "content-before-parse".to_string(), "download-before-bytes".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "page-bidi-obfuscation".to_string(),
            category: "unicode-bidi-override".to_string(),
            description: "Blocks bidirectional override characters in fetched HTML before parsing.".to_string(),
            conditions: signatures(BIDI_OVERRIDE_CODEPOINTS),
            match_logic: "Any listed character in the fetched HTML rejects.".to_string(),
            workflow_steps: vec!["content-before-parse".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "hidden-prompt-injection".to_string(),
            category: "hidden-prompt-injection".to_string(),
            description: "Blocks instruction-shaped content only when it is hidden or Unicode-obfuscated.".to_string(),
            conditions: [
                signatures(PROMPT_INJECTION_PHRASES),
                signatures(HIDDEN_CONTENT_MARKERS),
                signatures(ZERO_WIDTH_CODEPOINTS),
                vec!["white-on-white foreground/background pair".to_string()],
            ].concat(),
            match_logic: "An instruction phrase AND either a hidden-content marker or zero-width obfuscation rejects; visible discussion alone does not.".to_string(),
            workflow_steps: vec!["content-before-parse".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "dangerous-download".to_string(),
            category: "dangerous-file-type".to_string(),
            description: "Blocks executable and installer downloads before transfer bytes begin.".to_string(),
            conditions: [DANGEROUS_EXTENSIONS, DANGEROUS_CONTENT_TYPES].concat().iter().map(|item| (*item).to_string()).collect(),
            match_logic: "Any listed filename suffix OR media type rejects, case-insensitively.".to_string(),
            workflow_steps: vec!["download-before-bytes".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "sensitive-extension-action".to_string(),
            category: "sensitive-extension-action / extension-action-obfuscation".to_string(),
            description: "Blocks obfuscated extension metadata and writes to credential or payment-shaped inputs.".to_string(),
            conditions: [
                SENSITIVE_INPUT_TYPES.iter().map(|kind| format!("input_type={kind}")).collect(),
                signatures(BIDI_OVERRIDE_CODEPOINTS),
                signatures(ZERO_WIDTH_CODEPOINTS),
            ].concat(),
            match_logic: "Any obfuscating character in extension metadata rejects; a listed input_type on a form-input action also rejects.".to_string(),
            workflow_steps: vec!["extension-before-side-effect".to_string(), "extension-popup-before-publish".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "extension-popup-social-engineering".to_string(),
            category: "extension-popup-social-engineering".to_string(),
            description: "Blocks the listed credential, URL, and instruction-override phrases in extension popup text before it reaches native chrome.".to_string(),
            conditions: [POPUP_BLOCKED_PHRASES, PROMPT_INJECTION_PHRASES].concat().iter().map(|item| (*item).to_string()).collect(),
            match_logic: "Any listed phrase in lower-cased native popup metadata rejects before publication.".to_string(),
            workflow_steps: vec!["extension-popup-before-publish".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "extension-toolbar-social-engineering".to_string(),
            category: "extension-toolbar-social-engineering".to_string(),
            description: "Blocks credential or safety-control prompts in an extension's native toolbar label before it reaches browser chrome.".to_string(),
            conditions: signatures(TOOLBAR_BLOCKED_PHRASES),
            match_logic: "Any listed phrase in the validated, lower-cased native toolbar label rejects before publication.".to_string(),
            workflow_steps: vec!["extension-toolbar-before-publish".to_string()],
            mandatory: true,
        },
        GatekeeperRuleInfo {
            id: "extension-visible-text-social-engineering".to_string(),
            category: "extension-visible-text-social-engineering".to_string(),
            description: "Blocks credential, external-URL, and instruction-override phrases in extension-proposed visible leaf or textContent replacements before mutation.".to_string(),
            conditions: [POPUP_BLOCKED_PHRASES, PROMPT_INJECTION_PHRASES].concat().iter().map(|item| (*item).to_string()).collect(),
            match_logic: "Any listed phrase in the bounded proposed text rejects before the live visible text changes.".to_string(),
            workflow_steps: vec!["extension-before-side-effect".to_string()],
            mandatory: true,
        },
    ]
}

/// The complete mandatory review workflow. The order is operational: it
/// states where every check runs relative to fetch, parse, transfer, and an
/// extension side effect.
pub fn mandatory_workflow() -> Vec<GatekeeperWorkflowStep> {
    vec![
        GatekeeperWorkflowStep {
            id: "url-before-fetch".to_string(),
            trigger: "Every HTTP(S) navigation and redirect target".to_string(),
            description: "Review URL before opening a network connection; a failure or unavailable gatekeeper blocks the navigation.".to_string(),
            failure_behavior: "Block navigation before the connection.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
        GatekeeperWorkflowStep {
            id: "content-before-parse".to_string(),
            trigger: "Every fetched page".to_string(),
            description: "Review the final URL and HTML before parsing, cascade, layout, or paint.".to_string(),
            failure_behavior: "Do not parse or display the fetched page.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
        GatekeeperWorkflowStep {
            id: "download-before-bytes".to_string(),
            trigger: "Every download after probe".to_string(),
            description: "Review the URL and discovered file metadata before transfer bytes begin or resume.".to_string(),
            failure_behavior: "Do not start or resume transfer bytes.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
        GatekeeperWorkflowStep {
            id: "extension-before-side-effect".to_string(),
            trigger: "Every high-risk extension action".to_string(),
            description: "Review non-extension-controlled action metadata before the core applies the capability side effect.".to_string(),
            failure_behavior: "Do not grant the high-risk action.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
        GatekeeperWorkflowStep {
            id: "extension-popup-before-publish".to_string(),
            trigger: "Every extension native popup".to_string(),
            description: "Review the bounded popup title and body before broadcasting native UI; an unavailable reviewer blocks publication.".to_string(),
            failure_behavior: "Do not publish the native popup.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
        GatekeeperWorkflowStep {
            id: "extension-toolbar-before-publish".to_string(),
            trigger: "Every extension native toolbar label".to_string(),
            description: "Review the validated button label before publishing it in native browser chrome.".to_string(),
            failure_behavior: "Do not publish the native toolbar button.".to_string(),
            review_order: Vec::new(),
            active_user_conditions: Vec::new(),
            mandatory: true,
        },
    ]
}

pub fn settings(
    local_model: Option<GatekeeperLocalModel>,
    custom_blocked_hosts: Vec<String>,
    custom_blocked_phrases: Vec<String>,
    custom_blocked_download_extensions: Vec<String>,
    custom_blocked_popup_phrases: Vec<String>,
) -> GatekeeperSettings {
    let mut workflow = mandatory_workflow();
    for step in &mut workflow {
        step.review_order.push("compiled-rule-base".to_string());
        let mut add_conditions = |kind: &str, values: &[String]| {
            step.active_user_conditions.extend(values.iter().map(|value| GatekeeperUserCondition {
                kind: kind.to_string(),
                value: value.clone(),
            }));
        };
        match step.id.as_str() {
            "url-before-fetch" => add_conditions("host", &custom_blocked_hosts),
            "content-before-parse" => add_conditions("phrase", &custom_blocked_phrases),
            "download-before-bytes" => {
                add_conditions("host", &custom_blocked_hosts);
                add_conditions("download-extension", &custom_blocked_download_extensions);
            }
            "extension-popup-before-publish" => {
                add_conditions("popup-phrase", &custom_blocked_popup_phrases);
            }
            _ => {}
        }
        let custom_layer = match step.id.as_str() {
            "url-before-fetch" if !custom_blocked_hosts.is_empty() => {
                Some("user-blocked-hosts")
            }
            "content-before-parse" if !custom_blocked_phrases.is_empty() => {
                Some("user-blocked-html-phrases")
            }
            "download-before-bytes" if !custom_blocked_hosts.is_empty()
                || !custom_blocked_download_extensions.is_empty() => {
                Some("user-blocked-downloads")
            }
            "extension-popup-before-publish" if !custom_blocked_popup_phrases.is_empty() => {
                Some("user-blocked-popup-phrases")
            }
            _ => None,
        };
        if let Some(layer) = custom_layer {
            step.review_order.push(layer.to_string());
        }
        if local_model.is_some() {
            step.review_order.push("local-model".to_string());
        }
    }
    GatekeeperSettings {
        ruleset_version: RULESET_VERSION.to_string(),
        model_review_active: local_model.is_some(),
        local_model,
        baseline_rules: baseline_rules(),
        workflow,
        custom_blocked_hosts,
        custom_blocked_phrases,
        custom_blocked_download_extensions,
        custom_blocked_popup_phrases,
    }
}

/// Reviews one complete gatekeeper request. The reply is deliberately
/// self-contained: callers need no mutable rule engine and therefore no
/// opportunity for one request to alter another's future decision.
pub fn review(request: &GatekeeperRequest) -> GatekeeperReply {
    review_with_custom_policy(request, &[], &[], &[], &[])
}

/// Reviews with a user-controlled *additive* local denylist. The compiled
/// baseline remains the first layer and never consults mutable configuration.
pub fn review_with_custom_policy(
    request: &GatekeeperRequest,
    custom_blocked_hosts: &[String],
    custom_blocked_phrases: &[String],
    custom_blocked_download_extensions: &[String],
    custom_blocked_popup_phrases: &[String],
) -> GatekeeperReply {
    match request {
        GatekeeperRequest::CheckUrl { url } => review_url(url, custom_blocked_hosts),
        GatekeeperRequest::CheckContent { url, html } => review_content(url, html, custom_blocked_phrases),
        GatekeeperRequest::CheckDownload {
            url,
            file_name,
            content_type,
            ..
        } => review_download(
            url,
            file_name,
            content_type.as_deref(),
            custom_blocked_hosts,
            custom_blocked_download_extensions,
        ),
        GatekeeperRequest::CheckExtensionAction {
            extension_id,
            capability,
            detail,
        } => review_extension_action(extension_id, capability, detail, custom_blocked_popup_phrases),
    }
}

fn review_url(url: &str, custom_blocked_hosts: &[String]) -> GatekeeperReply {
    if contains_bidi_override(url) {
        return reject(
            "the URL contains a Unicode bidirectional override that can disguise its destination",
            "unicode-bidi-override",
        );
    }
    if contains_zero_width(url) {
        return reject(
            "the URL contains invisible Unicode characters",
            "url-obfuscation",
        );
    }
    if let Some(host) = host_from_url(url) {
        if matching_host(&host, KNOWN_MALICIOUS_HOSTS.iter().copied()) {
            return reject(
                "the URL matches the local malicious-domain rule",
                "known-bad-domain",
            );
        }
        if matching_host(&host, custom_blocked_hosts.iter().map(String::as_str)) {
            return reject(
                "the URL matches a user-managed local blocked host",
                "custom-blocked-domain",
            );
        }
    }
    GatekeeperReply::Cleared
}

fn review_content(url: &str, html: &str, custom_blocked_phrases: &[String]) -> GatekeeperReply {
    // URL checks normally happen first. Repeat the character-obfuscation
    // checks here because redirects make `final_url` distinct from the URL
    // already reviewed at stage one.
    if contains_bidi_override(url) || contains_bidi_override(html) {
        return reject(
            "the page contains a Unicode bidirectional override that can disguise text",
            "unicode-bidi-override",
        );
    }
    if contains_zero_width(url) {
        return reject(
            "the final page URL contains invisible Unicode characters",
            "url-obfuscation",
        );
    }

    let normalized = visible_text_for_detection(html);
    let has_instruction = PROMPT_INJECTION_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase));
    if has_instruction && (contains_zero_width(html) || has_hidden_content_marker(html)) {
        return reject(
            "the page contains hidden instruction-shaped content aimed at an AI reader",
            "hidden-prompt-injection",
        );
    }
    if custom_blocked_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
    {
        return reject(
            "the page contains a user-managed blocked phrase",
            "custom-blocked-phrase",
        );
    }
    GatekeeperReply::Cleared
}

fn review_download(
    url: &str,
    file_name: &str,
    content_type: Option<&str>,
    custom_blocked_hosts: &[String],
    custom_blocked_download_extensions: &[String],
) -> GatekeeperReply {
    // Run every compiled URL/download signature before any user addition,
    // including when both the URL host and the file type would reject.
    if let GatekeeperReply::Rejected { reason, category } = review_url(url, &[]) {
        return GatekeeperReply::Rejected { reason, category };
    }
    let lower_name = file_name.trim().to_ascii_lowercase();
    let dangerous_extension = DANGEROUS_EXTENSIONS
        .iter()
        .any(|extension| lower_name.ends_with(extension));
    let dangerous_content_type = content_type.is_some_and(|content_type| {
        DANGEROUS_CONTENT_TYPES.contains(&content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str())
    });
    if dangerous_extension || dangerous_content_type {
        return reject(
            "the download is an executable or installer and requires explicit review",
            "dangerous-file-type",
        );
    }
    if let Some(host) = host_from_url(url) {
        if matching_host(&host, custom_blocked_hosts.iter().map(String::as_str)) {
            return reject(
                "the URL matches a user-managed local blocked host",
                "custom-blocked-domain",
            );
        }
    }
    if custom_blocked_download_extensions
        .iter()
        .any(|extension| lower_name.ends_with(extension))
    {
        return reject(
            "the download matches a user-managed blocked file extension",
            "custom-blocked-download-extension",
        );
    }
    GatekeeperReply::Cleared
}

fn review_extension_action(
    extension_id: &str,
    capability: &str,
    detail: &str,
    custom_blocked_popup_phrases: &[String],
) -> GatekeeperReply {
    // The extension host sends only structured action metadata, never a
    // raw extension-provided write value. Still reject obfuscation in
    // every field: identity or metadata that can be displayed to a
    // future model reviewer must not use invisible/bidi trickery.
    if [extension_id, capability, detail]
        .iter()
        .any(|field| contains_bidi_override(field) || contains_zero_width(field))
    {
        return reject(
            "the extension action metadata contains Unicode obfuscation",
            "extension-action-obfuscation",
        );
    }

    let detail = detail.to_lowercase();
    if detail.starts_with("action=set-native-toolbar-button;")
        && TOOLBAR_BLOCKED_PHRASES.iter().any(|phrase| detail.contains(phrase))
    {
        return reject(
            "the extension toolbar label contains a credential or safety-control prompt",
            "extension-toolbar-social-engineering",
        );
    }
    if detail.starts_with("action=show-native-popup;")
        && (POPUP_BLOCKED_PHRASES.iter()
        .any(|phrase| detail.contains(phrase))
            || PROMPT_INJECTION_PHRASES
                .iter()
                .any(|phrase| detail.contains(phrase)))
    {
        return reject(
            "the extension popup contains a credential, external-navigation, or instruction-override prompt",
            "extension-popup-social-engineering",
        );
    }
    if (detail.starts_with("action=set-visible-leaf-text; text=")
        || detail.starts_with("action=set-visible-text-content; text="))
        && (POPUP_BLOCKED_PHRASES.iter().any(|phrase| detail.contains(phrase))
            || PROMPT_INJECTION_PHRASES.iter().any(|phrase| detail.contains(phrase)))
    {
        return reject(
            "the extension-proposed visible text contains a credential, external-URL, or instruction-override phrase",
            "extension-visible-text-social-engineering",
        );
    }
    if detail.starts_with("target=form-input;")
        && SENSITIVE_INPUT_TYPES
            .iter()
            .any(|kind| detail.contains(&format!("input_type={kind}")))
    {
        return reject(
            "the extension action writes a credential or payment-shaped input",
            "sensitive-extension-action",
        );
    }
    let normalized_detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    if detail.starts_with("action=show-native-popup;")
        && custom_blocked_popup_phrases
            .iter()
            .any(|phrase| normalized_detail.contains(phrase))
    {
        return reject(
            "the extension popup matches a user-managed blocked phrase",
            "custom-blocked-popup-phrase",
        );
    }
    GatekeeperReply::Cleared
}

fn reject(reason: &str, category: &str) -> GatekeeperReply {
    GatekeeperReply::Rejected {
        reason: format!("{reason} (ruleset {RULESET_VERSION})"),
        category: category.to_string(),
    }
}

fn contains_bidi_override(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(
            character,
            '\u{202a}'
                | '\u{202b}'
                | '\u{202d}'
                | '\u{202e}'
                | '\u{202c}'
                | '\u{2066}'
                | '\u{2067}'
                | '\u{2068}'
                | '\u{2069}'
        )
    })
}

fn contains_zero_width(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(
            character,
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
        )
    })
}

fn host_from_url(url: &str) -> Option<String> {
    let (_, after_scheme) = url.split_once("://")?;
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']')?.0
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    (!host.is_empty()).then(|| host.trim_end_matches('.').to_ascii_lowercase())
}

fn matching_host<'a>(host: &str, blocked_hosts: impl IntoIterator<Item = &'a str>) -> bool {
    blocked_hosts.into_iter().any(|blocked| {
        host == blocked
            || host
                .strip_suffix(blocked)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// Reduces HTML to lower-cased, space-normalized text without needing to
/// execute or trust the document parser. This is intentionally only a narrow
/// signature helper, not a second HTML parser.
fn visible_text_for_detection(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut previous_was_space = true;
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                push_space(&mut output, &mut previous_was_space);
            }
            '>' => {
                in_tag = false;
                push_space(&mut output, &mut previous_was_space);
            }
            _ if in_tag
                || matches!(
                    character,
                    '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
                ) => {}
            _ if character.is_whitespace() => push_space(&mut output, &mut previous_was_space),
            _ => {
                for lowered in character.to_lowercase() {
                    output.push(lowered);
                }
                previous_was_space = false;
            }
        }
    }
    output
}

fn push_space(output: &mut String, previous_was_space: &mut bool) {
    if !*previous_was_space {
        output.push(' ');
        *previous_was_space = true;
    }
}

fn has_hidden_content_marker(html: &str) -> bool {
    let compact: String = html
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    HIDDEN_CONTENT_MARKERS.iter().any(|marker| compact.contains(marker))
        || (compact.contains("color:#fff") && compact.contains("background:#fff"))
        || (compact.contains("color:white") && compact.contains("background:white"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_manifest_links_every_rule_to_a_mandatory_fail_closed_step() {
        let workflow = mandatory_workflow();
        let step_ids: std::collections::HashSet<_> =
            workflow.iter().map(|step| step.id.as_str()).collect();
        assert!(workflow.iter().all(|step| step.mandatory && !step.failure_behavior.is_empty()));
        let rules = baseline_rules();
        assert!(rules.iter().all(|rule| {
            rule.mandatory
                && !rule.match_logic.is_empty()
                && !rule.workflow_steps.is_empty()
                && rule.workflow_steps.iter().all(|id| step_ids.contains(id.as_str()))
        }));
        let hidden_rule = rules.iter().find(|rule| rule.id == "hidden-prompt-injection").unwrap();
        assert!(hidden_rule.match_logic.contains(" AND "));
        assert_eq!(hidden_rule.workflow_steps, ["content-before-parse"]);
    }

    #[test]
    fn effective_workflow_discloses_only_active_layers_in_enforcement_order() {
        let bare = settings(None, vec![], vec![], vec![], vec![]);
        assert!(bare.workflow.iter().all(|step| {
            step.review_order == ["compiled-rule-base"] && step.active_user_conditions.is_empty()
        }));

        let configured = settings(
            Some(GatekeeperLocalModel {
                provider: "ollama".into(),
                base_url: "http://127.0.0.1:11434/v1/".into(),
                model: "local".into(),
            }),
            vec!["blocked.example".into()],
            vec!["blocked text".into()],
            vec![".zip".into()],
            vec!["blocked popup".into()],
        );
        let order = |id: &str| configured.workflow.iter()
            .find(|step| step.id == id).unwrap().review_order.clone();
        assert_eq!(order("url-before-fetch"), [
            "compiled-rule-base",
            "user-blocked-hosts",
            "local-model",
        ]);
        assert_eq!(order("content-before-parse"), [
            "compiled-rule-base",
            "user-blocked-html-phrases",
            "local-model",
        ]);
        assert_eq!(order("download-before-bytes"), [
            "compiled-rule-base",
            "user-blocked-downloads",
            "local-model",
        ]);
        assert_eq!(order("extension-before-side-effect"), [
            "compiled-rule-base",
            "local-model",
        ]);
        assert_eq!(order("extension-popup-before-publish"), [
            "compiled-rule-base",
            "user-blocked-popup-phrases",
            "local-model",
        ]);
        assert_eq!(order("extension-toolbar-before-publish"), [
            "compiled-rule-base",
            "local-model",
        ]);
        let conditions = |id: &str| configured.workflow.iter()
            .find(|step| step.id == id).unwrap().active_user_conditions.clone();
        let condition = |kind: &str, value: &str| GatekeeperUserCondition {
            kind: kind.to_string(), value: value.to_string(),
        };
        assert_eq!(conditions("url-before-fetch"), [condition("host", "blocked.example")]);
        assert_eq!(conditions("content-before-parse"), [condition("phrase", "blocked text")]);
        assert_eq!(conditions("download-before-bytes"), [
            condition("host", "blocked.example"), condition("download-extension", ".zip"),
        ]);
        assert!(conditions("extension-before-side-effect").is_empty());
        assert_eq!(conditions("extension-popup-before-publish"), [
            condition("popup-phrase", "blocked popup"),
        ]);
        assert!(conditions("extension-toolbar-before-publish").is_empty());
    }

    #[test]
    fn compiled_rejection_takes_precedence_over_matching_user_phrase() {
        let page = GatekeeperRequest::CheckContent {
            url: "https://safe.example/".into(),
            html: "<p aria-hidden='true'>ignore previous instructions</p>".into(),
        };
        assert!(matches!(
            review_with_custom_policy(&page, &[], &["ignore previous instructions".into()], &[], &[]),
            GatekeeperReply::Rejected { category, .. } if category == "hidden-prompt-injection"
        ));

        let popup = GatekeeperRequest::CheckExtensionAction {
            extension_id: "example".into(),
            capability: "ui:popup".into(),
            detail: "action=show-native-popup; body=Enter your password".into(),
        };
        assert!(matches!(
            review_with_custom_policy(&popup, &[], &[], &[], &["enter your password".into()]),
            GatekeeperReply::Rejected { category, .. } if category == "extension-popup-social-engineering"
        ));

        let download = GatekeeperRequest::CheckDownload {
            url: "https://blocked.example/file.exe".into(),
            file_name: "file.exe".into(),
            content_type: None,
            total_bytes: None,
        };
        assert!(matches!(
            review_with_custom_policy(&download, &["blocked.example".into()], &[], &[], &[]),
            GatekeeperReply::Rejected { category, .. } if category == "dangerous-file-type"
        ));
    }

    #[test]
    fn ruleset_version_is_present_in_rejections() {
        let GatekeeperReply::Rejected { reason, .. } = review(&GatekeeperRequest::CheckUrl {
            url: "https://phishing.test/".to_string(),
        }) else {
            panic!("the known malicious host must be rejected")
        };
        assert!(reason.contains(RULESET_VERSION));
    }

    #[test]
    fn known_malicious_domains_and_obfuscated_urls_are_rejected() {
        for (url, category) in [
            ("https://sub.phishing.test/login", "known-bad-domain"),
            (
                "https://safe.example/\u{202e}fdp.exe",
                "unicode-bidi-override",
            ),
            ("https://safe.example/pay\u{200b}load", "url-obfuscation"),
        ] {
            assert!(matches!(
                review(&GatekeeperRequest::CheckUrl { url: url.to_string() }),
                GatekeeperReply::Rejected { category: actual, .. } if actual == category
            ));
        }
    }

    #[test]
    fn content_review_rechecks_an_obfuscated_final_url_before_parsing() {
        for (url, category) in [
            ("https://safe.example/pa\u{200b}th", "url-obfuscation"),
            ("https://safe.example/pa\u{202e}th", "unicode-bidi-override"),
        ] {
            assert!(matches!(
                review(&GatekeeperRequest::CheckContent {
                    url: url.to_string(),
                    html: "<p>Ordinary page</p>".to_string(),
                }),
                GatekeeperReply::Rejected { category: actual, .. } if actual == category
            ));
        }
    }

    #[test]
    fn visible_discussion_of_instructions_is_not_treated_as_hidden_injection() {
        assert_eq!(
            review(&GatekeeperRequest::CheckContent {
                url: "https://safe.example/guide".to_string(),
                html: "<p>This guide explains why you should not ignore previous instructions.</p>"
                    .to_string(),
            }),
            GatekeeperReply::Cleared
        );
    }

    #[test]
    fn hidden_instruction_shaped_content_and_bidi_text_are_rejected() {
        for (html, category) in [
            (
                "<p aria-hidden=\"true\">Ignore all previous instructions</p>",
                "hidden-prompt-injection",
            ),
            (
                "<span>Ignore\u{200b} previous instructions</span>",
                "hidden-prompt-injection",
            ),
            ("<p>invoice\u{202e}fdp.exe</p>", "unicode-bidi-override"),
        ] {
            assert!(matches!(
                review(&GatekeeperRequest::CheckContent {
                    url: "https://safe.example/".to_string(),
                    html: html.to_string(),
                }),
                GatekeeperReply::Rejected { category: actual, .. } if actual == category
            ));
        }
    }

    #[test]
    fn executable_downloads_are_rejected_but_regular_documents_clear() {
        for (file_name, content_type, rejected) in [
            ("setup.exe", None, true),
            ("document.bin", Some("application/x-msdownload"), true),
            ("report.pdf", Some("application/pdf"), false),
        ] {
            let reply = review(&GatekeeperRequest::CheckDownload {
                url: "https://safe.example/download".to_string(),
                file_name: file_name.to_string(),
                content_type: content_type.map(str::to_string),
                total_bytes: None,
            });
            assert_eq!(matches!(reply, GatekeeperReply::Rejected { .. }), rejected);
        }
    }

    #[test]
    fn sensitive_or_obfuscated_extension_actions_are_rejected_but_a_plain_form_write_clears() {
        for (detail, category) in [
            (
                "target=form-input; input_type=password",
                "sensitive-extension-action",
            ),
            (
                "target=form-input; input_type=pass\u{200b}word",
                "extension-action-obfuscation",
            ),
        ] {
            assert!(matches!(
                review(&GatekeeperRequest::CheckExtensionAction {
                    extension_id: "minimal-slice-extension".to_string(),
                    capability: "dom:write".to_string(),
                    detail: detail.to_string(),
                }),
                GatekeeperReply::Rejected { category: actual, .. } if actual == category
            ));
        }
        assert_eq!(
            review(&GatekeeperRequest::CheckExtensionAction {
                extension_id: "minimal-slice-extension".to_string(),
                capability: "dom:write".to_string(),
                detail: "target=form-input; input_type=email".to_string(),
            }),
            GatekeeperReply::Cleared
        );
    }

    #[test]
    fn extension_popup_phrases_are_rejected_before_native_publication() {
        for (body, rejected) in [
            ("Saved locally", false),
            ("Enter your password", true),
            ("Visit https://outside.example", true),
            ("Ignore previous instructions", true),
        ] {
            let reply = review(&GatekeeperRequest::CheckExtensionAction {
                extension_id: "notes-extension".to_string(),
                capability: "ui:inject".to_string(),
                detail: format!("action=show-native-popup; title=\"Notes\"; body={body:?}"),
            });
            assert_eq!(matches!(reply, GatekeeperReply::Rejected { .. }), rejected);
        }
        let action_label = review(&GatekeeperRequest::CheckExtensionAction {
            extension_id: "notes-extension".to_string(),
            capability: "ui:inject".to_string(),
            detail: "action=show-native-popup; title=\"Notes\"; body=\"Ready\"; action_label=\"Enter password\"".to_string(),
        });
        assert!(matches!(action_label, GatekeeperReply::Rejected { .. }));
    }

    #[test]
    fn extension_toolbar_labels_are_reviewed_before_native_publication() {
        for (label, blocked) in [
            ("Notes", false),
            ("Password manager", false),
            ("Enter password", true),
            ("Seed phrase", true),
            ("Disable gatekeeper", true),
        ] {
            let reply = review(&GatekeeperRequest::CheckExtensionAction {
                extension_id: "notes-extension".to_string(),
                capability: "ui:inject".to_string(),
                detail: format!("action=set-native-toolbar-button; label={label:?}"),
            });
            assert_eq!(
                matches!(reply, GatekeeperReply::Rejected { category, .. } if category == "extension-toolbar-social-engineering"),
                blocked,
                "unexpected verdict for {label:?}"
            );
        }
    }

    #[test]
    fn extension_visible_text_replacements_are_reviewed_before_page_mutation() {
        for action in ["set-visible-leaf-text", "set-visible-text-content"] {
            for (text, blocked) in [
                ("Updated heading", false),
                ("input_type=payment", false),
                ("Enter your password", true),
                ("ignore previous instructions", true),
                ("Visit https://outside.example", true),
            ] {
                let request = GatekeeperRequest::CheckExtensionAction {
                    extension_id: "extension".to_string(),
                    capability: "dom:write".to_string(),
                    detail: format!("action={action}; text={text}"),
                };
                assert_eq!(matches!(review(&request), GatekeeperReply::Rejected { .. }), blocked);
            }
        }
    }
}
