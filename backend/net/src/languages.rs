// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The languages the person reads, as a browser reports them: `navigator.languages` and the
//! `Accept-Language` request header. Taken from the operating system (on macOS the
//! `AppleLanguages` preference, elsewhere `LANG`), or from `BLUEICE_LANGUAGES` (comma
//! separated) when that is set.

use std::sync::OnceLock;

/// A language tag as a browser writes it: a script subtag that a region follows is dropped
/// (`zh-Hant-TW` is `zh-TW`), and the separator is a hyphen. `None` for the unset `C` locale.
pub fn browser_tag(tag: &str) -> Option<String> {
    let tag = tag.split(['.', '@']).next().unwrap_or("").trim().replace('_', "-");
    if tag.is_empty() || tag.eq_ignore_ascii_case("c") || tag.eq_ignore_ascii_case("posix") {
        return None;
    }
    let parts: Vec<&str> = tag.split('-').collect();
    let kept: Vec<&str> = match parts.as_slice() {
        [lang, script, region] if script.len() == 4 && region.len() <= 3 => vec![*lang, *region],
        other => other.to_vec(),
    };
    Some(kept.join("-"))
}

/// The tags in the macOS `defaults read -g AppleLanguages` property list text.
pub fn parse_apple_languages(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_end_matches(',').trim_matches('"'))
        .filter(|l| !l.is_empty() && *l != "(" && *l != ")")
        .filter_map(browser_tag)
        .collect()
}

/// The languages from a `LANG`-style value, or from a comma separated list.
pub fn parse_list(value: &str) -> Vec<String> {
    value.split([',', ':']).filter_map(browser_tag).collect()
}

fn detect() -> Vec<String> {
    if let Ok(over) = std::env::var("BLUEICE_LANGUAGES") {
        let list = parse_list(&over);
        if !list.is_empty() {
            return list;
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    {
        let list = parse_apple_languages(&String::from_utf8_lossy(&output.stdout));
        if !list.is_empty() {
            return list;
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            let list = parse_list(&value);
            if !list.is_empty() {
                return list;
            }
        }
    }
    vec!["en-US".to_string()]
}

/// The person's languages, most preferred first; never empty.
pub fn system_languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(detect)
}

/// An `Accept-Language` value for `languages`: each tag, then its base language, with falling
/// quality (`zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7`).
pub fn accept_language(languages: &[String]) -> String {
    let mut tags: Vec<String> = Vec::new();
    for tag in languages {
        for candidate in [tag.as_str(), tag.split('-').next().unwrap_or(tag)] {
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(candidate)) {
                tags.push(candidate.to_string());
            }
        }
    }
    tags.iter()
        .enumerate()
        .map(|(i, t)| if i == 0 { t.clone() } else { format!("{t};q={:.1}", (1.0 - 0.1 * i as f64).max(0.1)) })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_subtags_are_dropped_when_a_region_follows() {
        assert_eq!(browser_tag("zh-Hant-TW").as_deref(), Some("zh-TW"));
        assert_eq!(browser_tag("zh-Hans-CN").as_deref(), Some("zh-CN"));
        assert_eq!(browser_tag("en_US.UTF-8").as_deref(), Some("en-US"));
        assert_eq!(browser_tag("zh-Hant").as_deref(), Some("zh-Hant"));
        assert_eq!(browser_tag("fr").as_deref(), Some("fr"));
        assert_eq!(browser_tag("C"), None);
        assert_eq!(browser_tag("POSIX"), None);
        assert_eq!(browser_tag(""), None);
    }

    #[test]
    fn the_apple_languages_list_is_read() {
        assert_eq!(parse_apple_languages("(\n    \"zh-Hant-TW\",\n    \"en-US\"\n)\n"), ["zh-TW", "en-US"]);
        assert!(parse_apple_languages("(\n)\n").is_empty());
    }

    #[test]
    fn a_lang_value_and_a_list_are_read() {
        assert_eq!(parse_list("en_US.UTF-8"), ["en-US"]);
        assert_eq!(parse_list("ja-JP, en"), ["ja-JP", "en"]);
        assert!(parse_list("C").is_empty());
    }

    #[test]
    fn accept_language_adds_each_base_language_with_falling_quality() {
        assert_eq!(accept_language(&["zh-TW".into()]), "zh-TW,zh;q=0.9");
        assert_eq!(accept_language(&["en-US".into(), "fr".into()]), "en-US,en;q=0.9,fr;q=0.8");
        assert_eq!(accept_language(&["en".into()]), "en");
    }

    #[test]
    fn the_system_languages_are_never_empty() {
        assert!(!system_languages().is_empty());
    }
}
