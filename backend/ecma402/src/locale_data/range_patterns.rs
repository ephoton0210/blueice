// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Raw CLDR `miscPatterns.range` records used by NumberFormat ranges.

use super::NumberRangePattern;

/// Selects the pinned CLDR 48.2.1 range connector and the full-endpoint form.
/// ICU4X does not generate this `miscPatterns-numberSystem-*.range` field.
pub(super) fn number_range_pattern(locale: &str) -> NumberRangePattern {
    let language = locale.split('-').next().unwrap_or(locale);
    let (collapsed_separator, uncollapsed_separator) = match language {
        "ja" => ("～", " ～ "),
        "ko" => ("~", " ~ "),
        "to" => ("—", " — "),
        "et" => ("‒", " ‒ "),
        "mk" => ("\u{2009}–\u{2009}", " \u{2009}–\u{2009} "),
        "bs" if locale.split('-').any(|subtag| subtag == "Cyrl") => ("–", " – "),
        "oc" if !locale.split('-').any(|subtag| subtag == "ES") => ("–", " – "),
        "zh" if locale.split('-').any(|subtag| subtag == "Latn") => ("–", " – "),
        "bg" | "bs" | "hr" | "jv" | "kea" | "sk" => (" – ", " – "),
        "my" | "ro" => (" - ", " - "),
        "pt" if locale.split('-').any(|subtag| {
            matches!(
                subtag,
                "AO" | "CH" | "CV" | "GQ" | "GW" | "LU" | "MO" | "MZ" | "PT" | "ST" | "TL"
            )
        }) =>
        {
            (" - ", " - ")
        }
        "ca" | "da" | "es" | "eu" | "fil" | "fy" | "gu" | "it" | "ka" | "lij" | "ml" | "nl"
        | "oc" | "sq" | "th" | "tt" | "vec" | "vi" | "yue" | "zh" => ("-", " - "),
        _ => ("–", " – "),
    };
    NumberRangePattern {
        collapsed_separator,
        uncollapsed_separator,
    }
}

#[cfg(test)]
mod tests {
    use super::number_range_pattern;

    fn assert_pattern(locale: &str, collapsed: &str, uncollapsed: &str) {
        let actual = number_range_pattern(locale);
        assert_eq!(
            actual.collapsed_separator, collapsed,
            "collapsed connector for {locale}"
        );
        assert_eq!(
            actual.uncollapsed_separator, uncollapsed,
            "full-endpoint connector for {locale}"
        );
    }

    #[test]
    fn retains_every_pinned_cldr_range_connector_class() {
        // The 765 CLDR 48.2.1 raw records compress into these connector and
        // region/script classes. Keep every member of a multi-locale class in
        // this contract so changing a branch cannot silently lose one record.
        assert_pattern("en", "–", " – ");
        assert_pattern("ja", "～", " ～ ");
        assert_pattern("ko", "~", " ~ ");
        assert_pattern("to", "—", " — ");
        assert_pattern("et", "‒", " ‒ ");
        assert_pattern("mk", "\u{2009}–\u{2009}", " \u{2009}–\u{2009} ");

        assert_pattern("bs-Cyrl", "–", " – ");
        assert_pattern("bs-Latn", " – ", " – ");
        assert_pattern("oc", "–", " – ");
        assert_pattern("oc-ES", "-", " - ");
        assert_pattern("zh-Latn", "–", " – ");
        assert_pattern("zh-Hans", "-", " - ");

        for locale in ["bg", "bs", "hr", "jv", "kea", "sk"] {
            assert_pattern(locale, " – ", " – ");
        }
        for locale in ["my", "ro"] {
            assert_pattern(locale, " - ", " - ");
        }
        assert_pattern("pt", "–", " – ");
        for locale in [
            "pt-AO", "pt-CH", "pt-CV", "pt-GQ", "pt-GW", "pt-LU", "pt-MO", "pt-MZ", "pt-PT",
            "pt-ST", "pt-TL",
        ] {
            assert_pattern(locale, " - ", " - ");
        }
        for locale in [
            "ca", "da", "es", "eu", "fil", "fy", "gu", "it", "ka", "lij", "ml", "nl", "sq", "th",
            "tt", "vec", "vi", "yue", "zh",
        ] {
            assert_pattern(locale, "-", " - ");
        }
    }
}
