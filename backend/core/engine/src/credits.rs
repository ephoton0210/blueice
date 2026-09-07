// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The built-in Help/About/Credits page (`phase-4-human-rendering-path/PLAN.md`'s
//! last checklist item). BSD-3-Clause's binary-distribution clause
//! requires Chromium's copyright notice, the redistribution
//! conditions, and the disclaimer to be reproduced "in the
//! documentation and/or other materials provided with the
//! distribution" -- the per-file source headers from Phase 0 cover
//! *source* distribution, not this. Gecko (MPL-2.0) and the bundled
//! DejaVu Sans font (Bitstream Vera license, `blueice-font`) are
//! credited alongside it for disclosure consistency, per Phase 0's
//! PLAN.md.
//!
//! Rendered through the normal HTML/CSS/layout/paint pipeline like any
//! other page -- reached by navigating to [`CREDITS_URL`] -- rather
//! than a separate hardcoded drawing path in `frontend`, so it goes
//! through the same one render pass `CLAUDE.md`'s core goal requires
//! for everything else.
//!
//! Localized through `blueice-i18n` (`phase-14-i18n-localization/PLAN.md`)
//! rather than hardcoded English -- see [`credits_html`]. The two
//! actual license texts this page exists to reproduce (Chromium's
//! BSD-3-Clause notice, the DejaVu/Bitstream Vera notice) are always
//! shown in English first, since that's the text BSD-3-Clause's
//! "reproduce the above copyright notice... verbatim" requirement
//! actually refers to -- a translation is appended underneath for
//! locales other than English, clearly labeled non-authoritative,
//! never in place of the original.

/// The well-known URL [`Page::navigate`](crate::Page::navigate)
/// recognizes as a request for the built-in credits page instead of a
/// network fetch. An optional `?lang=<locale>` query parameter selects
/// a locale from `blueice_i18n::SUPPORTED_LOCALES` (see
/// [`locale_from_url`]); `blueice-frontend` sends this in response to
/// its `credits` stdin command. It doesn't share this constant
/// directly (it's a different process, possibly not even Rust) --
/// like any other URL, it's just a string carried over
/// `ClientMessage::Navigate`.
pub const CREDITS_URL: &str = "about:credits";

/// Picks the locale a `CREDITS_URL` request asked for out of an
/// optional `?lang=<locale>` suffix, falling back to
/// [`blueice_i18n::DEFAULT_LOCALE`] if absent or not in
/// [`blueice_i18n::SUPPORTED_LOCALES`] -- an unsupported locale must
/// degrade to the default page, never fail navigation outright.
pub fn locale_from_url(url: &str) -> &str {
    url.split_once("?lang=")
        .map(|(_, rest)| rest.split('&').next().unwrap_or(rest))
        .filter(|candidate| blueice_i18n::SUPPORTED_LOCALES.contains(candidate))
        .unwrap_or(blueice_i18n::DEFAULT_LOCALE)
}

const CHROMIUM_CONDITION_KEYS: [&str; 3] = ["chromium-condition-1", "chromium-condition-2", "chromium-condition-3"];

/// Builds the credits page HTML for `locale`, pulling every string
/// through `blueice-i18n`'s `credits` namespace instead of embedding
/// English literals directly.
pub fn credits_html(locale: &str) -> String {
    let t = |key: &str| blueice_i18n::translate(locale, "credits", key, &[]);
    let en = |key: &str| blueice_i18n::translate(blueice_i18n::DEFAULT_LOCALE, "credits", key, &[]);
    let is_translated = locale != blueice_i18n::DEFAULT_LOCALE;

    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1><p>{}</p>", t("about-title"), t("about-intro")));
    body.push_str(&format!("<h2>{}</h2><p>{}</p>", t("technical-references-heading"), t("technical-references-body")));

    body.push_str(&format!("<h2>{}</h2>", en("chromium-heading")));
    push_license_block(&mut body, &en);
    if is_translated {
        body.push_str(&format!("<p>{}</p>", t("translation-notice")));
        push_license_block(&mut body, &t);
    }

    body.push_str(&format!("<h2>{}</h2><p>{}</p>", en("gecko-heading"), t("gecko-body")));

    body.push_str(&format!("<h2>{}</h2><p>{}</p>", en("fonts-heading"), en("fonts-intro")));
    body.push_str(&format!("<p>{}</p><p>{}</p>", en("fonts-copyright"), en("fonts-permission")));
    if is_translated {
        body.push_str(&format!("<p>{}</p>", t("translation-notice")));
        body.push_str(&format!("<p>{}</p><p>{}</p>", t("fonts-copyright"), t("fonts-permission")));
    }

    format!("<html><head><title>{}</title></head><body>{}</body></html>", t("about-title"), body)
}

/// Appends the Chromium copyright/conditions/disclaimer block using
/// whichever lookup closure (English or the active locale's
/// translation) `credits_html` passes in -- factored out so the
/// English-original and translated renderings can never drift apart
/// in shape, only in which strings they pull.
fn push_license_block(body: &mut String, lookup: &dyn Fn(&str) -> String) {
    body.push_str(&format!("<p>{}</p><p>{}</p><ul>", lookup("chromium-copyright"), lookup("chromium-conditions-intro")));
    for key in CHROMIUM_CONDITION_KEYS {
        body.push_str(&format!("<li>{}</li>", lookup(key)));
    }
    body.push_str(&format!("</ul><p>{}</p>", lookup("chromium-disclaimer")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_url_uses_the_about_scheme() {
        assert_eq!(CREDITS_URL, "about:credits");
    }

    #[test]
    fn locale_from_url_defaults_when_no_query_is_present() {
        assert_eq!(locale_from_url(CREDITS_URL), blueice_i18n::DEFAULT_LOCALE);
    }

    #[test]
    fn locale_from_url_reads_a_supported_lang_parameter() {
        assert_eq!(locale_from_url("about:credits?lang=zh-TW"), "zh-TW");
    }

    #[test]
    fn locale_from_url_falls_back_for_an_unsupported_locale() {
        assert_eq!(locale_from_url("about:credits?lang=klingon"), blueice_i18n::DEFAULT_LOCALE);
    }

    #[test]
    fn default_locale_credits_html_reproduces_the_required_notices_in_english_only() {
        let html = credits_html(blueice_i18n::DEFAULT_LOCALE);
        assert!(html.contains("Copyright 2015 The Chromium Authors"));
        assert!(html.contains("Redistributions of source code must retain"));
        assert!(html.contains("THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS"));
        assert!(html.contains("Mozilla Public License"));
        assert!(html.contains("Bitstream"));
        assert!(!html.contains("非正式翻譯"), "the English-only page must not carry a translation notice");
    }

    #[test]
    fn zh_tw_credits_html_carries_both_the_english_original_and_the_translation() {
        let html = credits_html("zh-TW");
        // the authoritative English legal text is still present...
        assert!(html.contains("Copyright 2015 The Chromium Authors"));
        assert!(html.contains("THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS"));
        // ...alongside a labeled Traditional Chinese translation
        assert!(html.contains("關於 BlueIce"));
        assert!(html.contains("非正式翻譯"));
        assert!(html.contains("版權所有 2015 The Chromium Authors"));
    }
}
