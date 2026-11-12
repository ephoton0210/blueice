// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The OpenType features a box's text asks for: `font-kerning`, `font-variant-*` and
//! `font-feature-settings`, as the list of `(tag, value)` settings the shaper takes. Features not
//! listed are left to the font's defaults (kerning and the common ligatures on).

use crate::cascade::ComputedStyle;
use crate::tokenizer::Token;
use crate::value::Value;

/// A four-letter OpenType feature tag and its value (0 is off, 1 on, more picks an alternate).
pub type Feature = ([u8; 4], u32);

/// The words of a property's value: a keyword, or the identifiers of a list.
fn words(value: &Value) -> Vec<String> {
    match value {
        Value::Keyword(k) => vec![k.to_ascii_lowercase()],
        Value::Unparsed(tokens) => tokens
            .iter()
            .filter_map(|t| match t {
                Token::Ident(w) => Some(w.to_ascii_lowercase()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn tag(name: &str) -> [u8; 4] {
    let mut out = *b"    ";
    for (slot, byte) in out.iter_mut().zip(name.bytes()) {
        *slot = byte;
    }
    out
}

/// `"tag" value, "tag" value`: the settings of `font-feature-settings`.
fn feature_settings(value: &Value) -> Vec<Feature> {
    let Value::Unparsed(tokens) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for setting in tokens.split(|t| matches!(t, Token::Comma)) {
        let mut name: Option<String> = None;
        let mut number: Option<u32> = None;
        let mut on = true;
        for token in setting {
            match token {
                Token::QuotedString(s) if name.is_none() && s.len() == 4 && s.is_ascii() => {
                    name = Some(s.clone());
                }
                Token::Number(n) if *n >= 0.0 => number = Some(*n as u32),
                Token::Ident(w) if w.eq_ignore_ascii_case("off") => on = false,
                Token::Ident(w) if w.eq_ignore_ascii_case("on") => on = true,
                _ => {}
            }
        }
        if let Some(name) = name {
            out.push((tag(&name), number.unwrap_or(u32::from(on))));
        }
    }
    out
}

/// The features `style`'s text asks for, in the order they apply (a later setting of the same
/// tag wins). Empty when it asks for nothing, which is almost always.
pub fn font_features(style: &ComputedStyle) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::new();
    let other = &style.other;
    let mut set = |name: &str, value: u32| out.push((tag(name), value));
    if let Some(value) = other.get("font-kerning") {
        if words(value).first().is_some_and(|w| w == "none") {
            set("kern", 0);
        }
    }
    // Letters spaced out are not joined into ligatures.
    let spaced = matches!(other.get("letter-spacing"),
        Some(Value::Length(l)) if !matches!(l, crate::value::Length::Zero | crate::value::Length::Px(0.0)));
    if spaced {
        set("liga", 0);
        set("clig", 0);
    }
    if let Some(value) = other.get("font-variant-ligatures") {
        for word in words(value) {
            match word.as_str() {
                "none" => {
                    for f in ["liga", "clig", "dlig", "hlig", "calt"] {
                        set(f, 0);
                    }
                }
                "common-ligatures" => {
                    set("liga", 1);
                    set("clig", 1);
                }
                "no-common-ligatures" => {
                    set("liga", 0);
                    set("clig", 0);
                }
                "discretionary-ligatures" => set("dlig", 1),
                "no-discretionary-ligatures" => set("dlig", 0),
                "historical-ligatures" => set("hlig", 1),
                "no-historical-ligatures" => set("hlig", 0),
                "contextual" => set("calt", 1),
                "no-contextual" => set("calt", 0),
                _ => {}
            }
        }
    }
    let caps = other
        .get("font-variant-caps")
        .map(words)
        .or_else(|| {
            // `font-variant: small-caps` is the shorthand for it.
            other.get("font-variant").map(words)
        })
        .unwrap_or_default();
    for word in caps {
        match word.as_str() {
            "small-caps" => set("smcp", 1),
            "all-small-caps" => {
                set("smcp", 1);
                set("c2sc", 1);
            }
            "petite-caps" => set("pcap", 1),
            "all-petite-caps" => {
                set("pcap", 1);
                set("c2pc", 1);
            }
            "unicase" => set("unic", 1),
            "titling-caps" => set("titl", 1),
            _ => {}
        }
    }
    if let Some(value) = other.get("font-variant-numeric") {
        for word in words(value) {
            match word.as_str() {
                "lining-nums" => set("lnum", 1),
                "oldstyle-nums" => set("onum", 1),
                "proportional-nums" => set("pnum", 1),
                "tabular-nums" => set("tnum", 1),
                "diagonal-fractions" => set("frac", 1),
                "stacked-fractions" => set("afrc", 1),
                "ordinal" => set("ordn", 1),
                "slashed-zero" => set("zero", 1),
                _ => {}
            }
        }
    }
    if let Some(value) = other.get("font-variant-position") {
        for word in words(value) {
            match word.as_str() {
                "sub" => set("subs", 1),
                "super" => set("sups", 1),
                _ => {}
            }
        }
    }
    if let Some(value) = other.get("font-variant-east-asian") {
        for word in words(value) {
            match word.as_str() {
                "jis78" => set("jp78", 1),
                "jis83" => set("jp83", 1),
                "jis90" => set("jp90", 1),
                "jis04" => set("jp04", 1),
                "simplified" => set("smpl", 1),
                "traditional" => set("trad", 1),
                "full-width" => set("fwid", 1),
                "proportional-width" => set("pwid", 1),
                "ruby" => set("ruby", 1),
                _ => {}
            }
        }
    }
    // The low-level property wins over every other.
    if let Some(value) = other.get("font-feature-settings") {
        out.extend(feature_settings(value));
    }
    // A later setting of a tag replaces an earlier one.
    let mut seen: Vec<Feature> = Vec::with_capacity(out.len());
    for (name, value) in out {
        match seen.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => seen.push((name, value)),
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn style(props: &[(&str, &str)]) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        for (name, value) in props {
            let tokens = tokenize(value);
            let value = match tokens.as_slice() {
                [Token::Ident(w)] => Value::Keyword(w.clone()),
                _ => Value::Unparsed(tokens),
            };
            style.other.insert((*name).to_string(), value);
        }
        style
    }

    fn feats(props: &[(&str, &str)]) -> Vec<(String, u32)> {
        font_features(&style(props))
            .into_iter()
            .map(|(t, v)| (String::from_utf8(t.to_vec()).unwrap().trim().to_string(), v))
            .collect()
    }

    #[test]
    fn a_style_that_asks_for_nothing_asks_for_no_features() {
        assert!(font_features(&ComputedStyle::initial()).is_empty());
        assert!(feats(&[("font-kerning", "auto")]).is_empty());
        assert!(feats(&[("font-variant-ligatures", "normal")]).is_empty());
    }

    #[test]
    fn kerning_and_ligatures_can_be_turned_off() {
        assert_eq!(feats(&[("font-kerning", "none")]), [("kern".to_string(), 0)]);
        let none = feats(&[("font-variant-ligatures", "none")]);
        assert!(["liga", "clig", "dlig", "hlig", "calt"].iter().all(|t| none.contains(&((*t).to_string(), 0))));
        assert_eq!(
            feats(&[("font-variant-ligatures", "no-common-ligatures discretionary-ligatures")]),
            [("liga".to_string(), 0), ("clig".to_string(), 0), ("dlig".to_string(), 1)]
        );
    }

    #[test]
    fn spaced_out_letters_are_not_ligated() {
        let mut s = ComputedStyle::initial();
        s.other.insert("letter-spacing".into(), Value::Length(crate::value::Length::Px(2.0)));
        let f = font_features(&s);
        assert!(f.contains(&(*b"liga", 0)) && f.contains(&(*b"clig", 0)));
        s.other.insert("letter-spacing".into(), Value::Length(crate::value::Length::Px(0.0)));
        assert!(font_features(&s).is_empty());
    }

    #[test]
    fn the_variant_properties_turn_on_their_features() {
        assert_eq!(feats(&[("font-variant-caps", "small-caps")]), [("smcp".to_string(), 1)]);
        assert_eq!(feats(&[("font-variant", "small-caps")]), [("smcp".to_string(), 1)]);
        assert_eq!(
            feats(&[("font-variant-numeric", "tabular-nums slashed-zero")]),
            [("tnum".to_string(), 1), ("zero".to_string(), 1)]
        );
        assert_eq!(feats(&[("font-variant-position", "super")]), [("sups".to_string(), 1)]);
        assert_eq!(feats(&[("font-variant-east-asian", "jis04 full-width")]), [("jp04".to_string(), 1), ("fwid".to_string(), 1)]);
    }

    #[test]
    fn font_feature_settings_has_the_last_word() {
        assert_eq!(
            feats(&[("font-feature-settings", "\"liga\" 0, \"ss01\", \"cv05\" 3, \"kern\" off")]),
            [
                ("liga".to_string(), 0),
                ("ss01".to_string(), 1),
                ("cv05".to_string(), 3),
                ("kern".to_string(), 0)
            ]
        );
        // it overrides a variant property that set the same tag
        assert_eq!(
            feats(&[("font-variant-caps", "small-caps"), ("font-feature-settings", "\"smcp\" 0")]),
            [("smcp".to_string(), 0)]
        );
        // a malformed tag is ignored
        assert!(feats(&[("font-feature-settings", "\"toolong\" 1")]).is_empty());
    }
}
