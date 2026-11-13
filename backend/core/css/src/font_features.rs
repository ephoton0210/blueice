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

/// The features one `font-variant-*` keyword turns on or off.
fn variant_keyword(word: &str) -> &'static [(&'static str, u32)] {
    match word {
        // ligatures
        "none" => &[("liga", 0), ("clig", 0), ("dlig", 0), ("hlig", 0), ("calt", 0)],
        "common-ligatures" => &[("liga", 1), ("clig", 1)],
        "no-common-ligatures" => &[("liga", 0), ("clig", 0)],
        "discretionary-ligatures" => &[("dlig", 1)],
        "no-discretionary-ligatures" => &[("dlig", 0)],
        "historical-ligatures" => &[("hlig", 1)],
        "no-historical-ligatures" => &[("hlig", 0)],
        "contextual" => &[("calt", 1)],
        "no-contextual" => &[("calt", 0)],
        // caps
        "small-caps" => &[("smcp", 1)],
        "all-small-caps" => &[("smcp", 1), ("c2sc", 1)],
        "petite-caps" => &[("pcap", 1)],
        "all-petite-caps" => &[("pcap", 1), ("c2pc", 1)],
        "unicase" => &[("unic", 1)],
        "titling-caps" => &[("titl", 1)],
        // numeric
        "lining-nums" => &[("lnum", 1)],
        "oldstyle-nums" => &[("onum", 1)],
        "proportional-nums" => &[("pnum", 1)],
        "tabular-nums" => &[("tnum", 1)],
        "diagonal-fractions" => &[("frac", 1)],
        "stacked-fractions" => &[("afrc", 1)],
        "ordinal" => &[("ordn", 1)],
        "slashed-zero" => &[("zero", 1)],
        // position
        "sub" => &[("subs", 1)],
        "super" => &[("sups", 1)],
        // east asian
        "jis78" => &[("jp78", 1)],
        "jis83" => &[("jp83", 1)],
        "jis90" => &[("jp90", 1)],
        "jis04" => &[("jp04", 1)],
        "simplified" => &[("smpl", 1)],
        "traditional" => &[("trad", 1)],
        "full-width" => &[("fwid", 1)],
        "proportional-width" => &[("pwid", 1)],
        "ruby" => &[("ruby", 1)],
        _ => &[],
    }
}

/// `font-variant-alternates`: `historical-forms` needs no name. The functions that pick a feature
/// by name (`stylistic()`, `styleset()`...) are resolved against `@font-feature-values` into
/// `-blueice-alternates` (see [`crate::FeatureValues::resolve`]).
fn alternates(value: &Value) -> Vec<(String, u32)> {
    let mentions_historical = match value {
        Value::Keyword(k) => k.eq_ignore_ascii_case("historical-forms"),
        Value::Unparsed(tokens) => tokens
            .iter()
            .any(|t| matches!(t, Token::Ident(w) if w.eq_ignore_ascii_case("historical-forms"))),
        _ => false,
    };
    if mentions_historical {
        vec![("hist".to_string(), 1)]
    } else {
        Vec::new()
    }
}

/// The kind of capitals a style asks for with `font-variant-caps` (or the `font-variant`
/// shorthand).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapsKind {
    Small,
    AllSmall,
    Petite,
    AllPetite,
    Unicase,
    Titling,
}

/// The capitals `style` asks for, `None` for `normal` or none.
pub fn caps_kind(style: &ComputedStyle) -> Option<CapsKind> {
    let other = &style.other;
    let mut kind = None;
    for name in ["font-variant", "font-variant-caps"] {
        if let Some(value) = other.get(name) {
            for word in words(value) {
                match word.as_str() {
                    "small-caps" => kind = Some(CapsKind::Small),
                    "all-small-caps" => kind = Some(CapsKind::AllSmall),
                    "petite-caps" => kind = Some(CapsKind::Petite),
                    "all-petite-caps" => kind = Some(CapsKind::AllPetite),
                    "unicase" => kind = Some(CapsKind::Unicase),
                    "titling-caps" => kind = Some(CapsKind::Titling),
                    "normal" | "none" => kind = None,
                    _ => {}
                }
            }
        }
    }
    kind
}

/// Whether `font-synthesis` lets small capitals be made when a face has none.
fn small_caps_synthesis_allowed(style: &ComputedStyle) -> bool {
    let other = &style.other;
    if other.get("font-synthesis-small-caps").is_some_and(|v| words(v).first().is_some_and(|w| w == "none")) {
        return false;
    }
    if let Some(value) = other.get("font-synthesis") {
        let listed = words(value);
        // `none`, or a list that does not name small caps, refuses it.
        if !listed.is_empty() && !listed.iter().any(|w| w == "small-caps") {
            return false;
        }
    }
    true
}

/// Capitals made by drawing letters as scaled capitals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SynthesizedCaps {
    /// Capitals shrink too (`all-small-caps`), not only lowercase letters.
    pub all: bool,
    /// Only the capitals are drawn small and lowercase stays as it is (`unicase`).
    pub unicase: bool,
}

/// The features a style's text asks for given what its face supports, and the capitals to make
/// by scaling where it cannot supply them (the spec's fallbacks: small capitals without `pcap`,
/// capitals as small capitals for `unicase` without `unic`; none for titling caps).
/// `supports(tag)` says whether the face has an OpenType feature.
pub struct TextFeatures {
    pub features: Vec<Feature>,
    pub synthesized_caps: Option<SynthesizedCaps>,
}

pub fn text_features(style: &ComputedStyle, supports: &dyn Fn([u8; 4]) -> bool) -> TextFeatures {
    let mut features = font_features(style);
    let Some(kind) = caps_kind(style) else {
        return TextFeatures { features, synthesized_caps: None };
    };
    // `font-feature-settings: "smcp" 0` refuses small capitals of any origin.
    let refused = features.iter().any(|(t, v)| *t == *b"smcp" && *v == 0);
    let allowed = small_caps_synthesis_allowed(style) && !refused;
    let mut add = |tag: &[u8; 4]| {
        if !features.iter().any(|(t, _)| t == tag) {
            features.push((*tag, 1));
        }
    };
    let synthesized = match kind {
        CapsKind::Small | CapsKind::AllSmall => {
            let all = kind == CapsKind::AllSmall;
            (!supports(*b"smcp") && allowed).then_some(SynthesizedCaps { all, unicase: false })
        }
        CapsKind::Petite | CapsKind::AllPetite => {
            let all = kind == CapsKind::AllPetite;
            if supports(*b"pcap") {
                None
            } else if supports(*b"smcp") {
                // Petite capitals without `pcap` are small capitals.
                add(b"smcp");
                if all {
                    add(b"c2sc");
                }
                None
            } else {
                allowed.then_some(SynthesizedCaps { all, unicase: false })
            }
        }
        CapsKind::Unicase => {
            (!supports(*b"unic") && allowed).then_some(SynthesizedCaps { all: false, unicase: true })
        }
        CapsKind::Titling => None,
    };
    TextFeatures { features, synthesized_caps: synthesized }
}

/// The features `style`'s text asks for, in the order they apply (a later setting of the same
/// tag wins). Empty when it asks for nothing, which is almost always.
pub fn font_features(style: &ComputedStyle) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::new();
    let other = &style.other;
    let mut set = |name: &str, value: u32| out.push((tag(name), value));
    // Text that is to be drawn fast is not kerned or ligated; the properties below say otherwise.
    if other.get("text-rendering").is_some_and(|v| words(v).first().is_some_and(|w| w == "optimizespeed")) {
        for f in ["kern", "liga", "clig", "calt"] {
            set(f, 0);
        }
    }
    if let Some(value) = other.get("font-kerning") {
        match words(value).first().map(String::as_str) {
            Some("none") => set("kern", 0),
            // Asked for outright, it overrides what `text-rendering` leaves off.
            Some("normal") => set("kern", 1),
            _ => {}
        }
    }
    // Letters spaced out are not joined into ligatures.
    let spaced = matches!(other.get("letter-spacing"),
        Some(Value::Length(l)) if !matches!(l, crate::value::Length::Zero | crate::value::Length::Px(0.0)));
    if spaced {
        set("liga", 0);
        set("clig", 0);
    }
    // The `font-variant` shorthand and its longhands take the same keywords, so one table reads them.
    for name in [
        "font-variant",
        "font-variant-ligatures",
        "font-variant-caps",
        "font-variant-numeric",
        "font-variant-position",
        "font-variant-east-asian",
    ] {
        if let Some(value) = other.get(name) {
            for word in words(value) {
                for (tag_name, setting) in variant_keyword(&word) {
                    set(tag_name, *setting);
                }
            }
        }
    }
    if let Some(value) = other.get("font-variant-alternates") {
        for (tag_name, setting) in alternates(value) {
            if tag_name.len() == 4 {
                set(&tag_name, setting);
            }
        }
    }
    // Names resolved against `@font-feature-values`.
    if let Some(value) = other.get("-blueice-alternates") {
        out.extend(feature_settings(value));
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

/// The features for properties given as `(name, value)` text, as a stylesheet would set them: for
/// callers that have no computed style, such as a canvas.
pub fn font_features_for(properties: &[(&str, &str)]) -> Vec<Feature> {
    let mut style = ComputedStyle::initial();
    for (name, value) in properties {
        let tokens = crate::tokenizer::tokenize(value);
        let value = match tokens.as_slice() {
            [Token::Ident(w)] => Value::Keyword(w.clone()),
            _ => Value::Unparsed(tokens),
        };
        style.other.insert((*name).to_string(), value);
    }
    font_features(&style)
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
    fn optimize_speed_turns_kerning_and_ligatures_off_but_the_other_settings_win() {
        let off = feats(&[("text-rendering", "optimizeSpeed")]);
        assert!(["kern", "liga", "clig", "calt"].iter().all(|t| off.contains(&((*t).to_string(), 0))));
        assert!(feats(&[("text-rendering", "optimizeLegibility")]).is_empty());
        assert!(feats(&[("text-rendering", "geometricPrecision")]).is_empty());
        let back = feats(&[("text-rendering", "optimizeSpeed"), ("font-kerning", "normal"), ("font-feature-settings", "\"liga\" 1")]);
        assert!(back.contains(&("liga".to_string(), 1)));
        assert!(back.contains(&("kern".to_string(), 1)), "font-kerning: normal turns it back on: {back:?}");
        assert!(back.contains(&("calt".to_string(), 0)));
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
    fn the_font_variant_shorthand_takes_the_keywords_of_all_its_longhands() {
        assert_eq!(
            feats(&[("font-variant", "small-caps tabular-nums")]),
            [("smcp".to_string(), 1), ("tnum".to_string(), 1)]
        );
        let none = feats(&[("font-variant", "none")]);
        assert!(["liga", "clig", "dlig", "hlig", "calt"].iter().all(|t| none.contains(&((*t).to_string(), 0))));
        assert!(feats(&[("font-variant", "normal")]).is_empty());
    }

    #[test]
    fn only_historical_forms_needs_no_names() {
        assert_eq!(feats(&[("font-variant-alternates", "historical-forms")]), [("hist".to_string(), 1)]);
        // names are resolved against @font-feature-values, not read here
        assert!(feats(&[("font-variant-alternates", "stylistic(fancy) swash(2)")]).is_empty());
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
