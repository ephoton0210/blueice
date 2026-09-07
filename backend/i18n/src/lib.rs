// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueIce's i18n namespace/key lookup (`phase-14-i18n-localization/PLAN.md`).
//! Every user-visible UI string goes through [`translate`] rather than
//! being a hardcoded literal in the crate that displays it -- the
//! skeleton this phase exists to build, ready to grow past the two
//! locales shipped today (`en`, the fallback every other locale is
//! checked against; `zh-TW`) toward covering the language lists
//! Windows/macOS ship, without another redesign.
//!
//! Backed by Fluent (`fluent-bundle`), the same localization system
//! Gecko/Firefox itself uses -- reused rather than hand-rolled for the
//! same reason `blueice-net` uses `ureq` and `blueice-frontend-
//! reference` uses `winit`: message formatting (plural rules, argument
//! substitution, eventually bidi) is solved infrastructure, not the
//! parsing/layout/paint domain this project is written from scratch
//! for.
//!
//! Resources are namespaced `.ftl` files under `locales/<locale>/
//! <namespace>.ftl`, embedded into the binary via `include_str!` (see
//! [`resource_text`]) rather than read from disk at runtime -- BlueIce
//! ships one binary per platform, not a package with loose data files
//! next to it.

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// The locale every other locale falls back to. Must carry every key
/// any namespace ever defines -- [`translate`] panics if it doesn't,
/// since a missing fallback key is a bug in the resource files, not a
/// runtime condition callers should have to handle.
pub const DEFAULT_LOCALE: &str = "en";

/// Locales with at least one namespace translated today. Callers
/// choosing a locale (from an OS setting, a URL parameter, ...) should
/// check membership here and fall back to [`DEFAULT_LOCALE`] themselves
/// for a locale with no resources at all, though [`translate`] also
/// degrades gracefully (via its own fallback) if they don't.
pub const SUPPORTED_LOCALES: &[&str] = &["en", "zh-TW"];

fn resource_text(locale: &str, namespace: &str) -> Option<&'static str> {
    match (locale, namespace) {
        ("en", "credits") => Some(include_str!("../locales/en/credits.ftl")),
        ("en", "frontend") => Some(include_str!("../locales/en/frontend.ftl")),
        ("zh-TW", "credits") => Some(include_str!("../locales/zh-TW/credits.ftl")),
        ("zh-TW", "frontend") => Some(include_str!("../locales/zh-TW/frontend.ftl")),
        // Test-only fixtures for the two defensive panics in
        // `bundle_for` below, which no real bundled `.ftl` file should
        // ever trigger -- these exist purely so those panics are
        // exercised by a test rather than left as dead code no one
        // ever runs.
        #[cfg(test)]
        ("und", "malformed") => Some("this = { $unterminated"),
        #[cfg(test)]
        ("und", "duplicate") => Some("dup-key = one\ndup-key = two\n"),
        _ => None,
    }
}

fn bundle_for(locale: &str, namespace: &str) -> Option<FluentBundle<FluentResource>> {
    let text = resource_text(locale, namespace)?;
    let langid: LanguageIdentifier = locale.parse().expect("locale tags in resource_text must be valid BCP-47");
    let resource = FluentResource::try_new(text.to_string()).unwrap_or_else(|(_, errors)| {
        panic!("bundled {locale}/{namespace}.ftl failed to parse: {errors:?}");
    });
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle.add_resource(resource).unwrap_or_else(|errors| {
        panic!("bundled {locale}/{namespace}.ftl has duplicate message ids: {errors:?}");
    });
    Some(bundle)
}

fn format_message(bundle: &FluentBundle<FluentResource>, key: &str, args: &[(&str, &str)]) -> Option<String> {
    let msg = bundle.get_message(key)?;
    let pattern = msg.value()?;
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, FluentValue::from(*value));
    }
    let mut errors = Vec::new();
    let value = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
    Some(value.into_owned())
}

/// Looks up `namespace`/`key` in `locale`, substituting `args` (Fluent
/// `{ $name }` placeholders), falling back to [`DEFAULT_LOCALE`] if
/// `locale` has no resources for `namespace` or is simply missing this
/// one key -- a partially-translated locale must never render
/// blank/missing UI text for the keys it hasn't gotten to yet. Panics
/// only if [`DEFAULT_LOCALE`] itself lacks the key, since that's a
/// resource-file bug, not something a caller can meaningfully recover
/// from at the call site.
pub fn translate(locale: &str, namespace: &str, key: &str, args: &[(&str, &str)]) -> String {
    if let Some(bundle) = bundle_for(locale, namespace) {
        if let Some(text) = format_message(&bundle, key, args) {
            return text;
        }
    }
    let bundle = bundle_for(DEFAULT_LOCALE, namespace).unwrap_or_else(|| panic!("no {DEFAULT_LOCALE} resource for namespace {namespace:?}"));
    format_message(&bundle, key, args).unwrap_or_else(|| panic!("missing key {key:?} in {DEFAULT_LOCALE}/{namespace} (the fallback locale must define every key)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_a_plain_key_in_the_default_locale() {
        assert_eq!(translate("en", "credits", "about-title", &[]), "About BlueIce");
    }

    #[test]
    fn translates_the_same_key_in_a_supported_non_default_locale() {
        assert_eq!(translate("zh-TW", "credits", "about-title", &[]), "關於 BlueIce");
    }

    // Fluent wraps a substituted argument in FSI/PDI (U+2068/U+2069)
    // bidi-isolation marks by default -- deliberately left on rather
    // than disabled, since it's exactly the safety net a URL/name
    // embedded in an RTL translation (Arabic, Hebrew -- both on the
    // eventual Microsoft/Apple language-coverage list this phase is
    // building toward) needs to keep its own left-to-right run from
    // corrupting the surrounding text's direction.
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';

    #[test]
    fn substitutes_a_named_argument() {
        let title = translate("en", "frontend", "window-title-navigated", &[("url", "https://example.com")]);
        assert_eq!(title, format!("BlueIce -- {FSI}https://example.com{PDI}"));
    }

    #[test]
    fn substitutes_a_named_argument_in_a_non_default_locale_too() {
        let title = translate("zh-TW", "frontend", "window-title-navigated", &[("url", "https://example.com")]);
        assert_eq!(title, format!("BlueIce —— {FSI}https://example.com{PDI}"));
    }

    #[test]
    fn an_unsupported_locale_falls_back_to_the_default_locale() {
        assert_eq!(translate("fr", "credits", "about-title", &[]), translate("en", "credits", "about-title", &[]));
    }

    const CREDITS_KEYS: &[&str] = &[
        "about-title",
        "about-intro",
        "technical-references-heading",
        "technical-references-body",
        "chromium-heading",
        "chromium-copyright",
        "chromium-conditions-intro",
        "chromium-condition-1",
        "chromium-condition-2",
        "chromium-condition-3",
        "chromium-disclaimer",
        "gecko-heading",
        "gecko-body",
        "fonts-heading",
        "fonts-intro",
        "fonts-copyright",
        "fonts-permission",
        "translation-notice",
    ];
    const FRONTEND_KEYS: &[&str] = &["window-title-default", "window-title-navigated"];

    #[test]
    fn every_supported_locale_defines_every_key_the_default_locale_does() {
        // guards against the exact failure mode `translate`'s panic
        // documents -- a resource file silently missing a key it
        // should have, discovered here at test time rather than by a
        // user seeing English leak into an otherwise-translated screen.
        for (namespace, keys) in [("credits", CREDITS_KEYS), ("frontend", FRONTEND_KEYS)] {
            for locale in SUPPORTED_LOCALES {
                let bundle = bundle_for(locale, namespace).unwrap_or_else(|| panic!("{locale}/{namespace} has no resource at all"));
                for key in keys {
                    assert!(bundle.has_message(key), "{locale}/{namespace} is missing key {key:?} that {DEFAULT_LOCALE} defines");
                }
            }
        }
    }

    #[test]
    fn supported_locales_lists_at_least_english_and_traditional_chinese() {
        assert!(SUPPORTED_LOCALES.contains(&"en"));
        assert!(SUPPORTED_LOCALES.contains(&"zh-TW"));
    }

    #[test]
    fn a_key_missing_from_a_supported_locale_falls_back_to_default_locale_text() {
        // "only-in-english" exists in en/frontend.ftl but is
        // deliberately absent from zh-TW/frontend.ftl -- unlike
        // `an_unsupported_locale_falls_back_to_the_default_locale`
        // (which exercises a locale with no bundle at all), this
        // exercises a locale that *has* a real bundle for the
        // namespace but is still missing this one key.
        let en_text = translate(DEFAULT_LOCALE, "frontend", "only-in-english", &[]);
        let zh_text = translate("zh-TW", "frontend", "only-in-english", &[]);
        assert_eq!(zh_text, en_text);
    }

    #[test]
    #[should_panic(expected = "failed to parse")]
    fn a_malformed_bundled_resource_panics_with_a_clear_message() {
        bundle_for("und", "malformed");
    }

    #[test]
    #[should_panic(expected = "duplicate message ids")]
    fn a_bundled_resource_with_duplicate_message_ids_panics_with_a_clear_message() {
        bundle_for("und", "duplicate");
    }
}
