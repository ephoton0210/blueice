// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finished CSS animations.
//!
//! The engine does not run animations, but a very common pattern is
//! `opacity: 0; animation: fade-in 1s both` -- the element is hidden until a
//! script-free animation reveals it. Rendering the page as if every such
//! animation had already played to its end is what a viewer who waited would
//! see, so for each rule whose animation is finite and keeps its final frame
//! (`forwards` or `both`) this adds a rule with the same selectors that carries
//! that last keyframe's declarations, marked `!important` so it outranks the
//! rule's own starting value the way a running animation outranks normal styles.

use crate::parser::{parse_declarations, Declaration, Rule};
use crate::tokenizer::Token;
use std::collections::HashMap;

/// Words in an `animation` value that are never the animation's name.
const KEYWORDS: &[&str] = &[
    "none",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "linear",
    "step-start",
    "step-end",
    "normal",
    "reverse",
    "alternate",
    "alternate-reverse",
    "forwards",
    "backwards",
    "both",
    "infinite",
    "running",
    "paused",
];

/// The declarations of the last frame of each `@keyframes` block, by name.
fn final_frames(tokens: &[Token]) -> HashMap<String, Vec<Declaration>> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        let Token::AtKeyword(name) = &tokens[i] else {
            i += 1;
            continue;
        };
        let is_keyframes = name.to_ascii_lowercase().ends_with("keyframes");
        let Some(open) = tokens[i..]
            .iter()
            .position(|t| *t == Token::LeftBrace)
            .map(|p| i + p)
        else {
            break;
        };
        let Some(len) = matching(&tokens[open + 1..]) else {
            break;
        };
        let body = &tokens[open + 1..open + 1 + len];
        if is_keyframes {
            let keyframe_name = tokens[i + 1..open].iter().find_map(|t| match t {
                Token::Ident(n) | Token::QuotedString(n) => Some(n.clone()),
                _ => None,
            });
            if let (Some(keyframe_name), Some(frame)) = (keyframe_name, last_frame(body)) {
                out.insert(keyframe_name, frame);
            }
        }
        i = open + 1 + len + 1;
    }
    out
}

/// Index of the `}` closing a block whose `{` was just before `tokens`.
fn matching(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::LeftBrace => depth += 1,
            Token::RightBrace if depth == 0 => return Some(i),
            Token::RightBrace => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The declarations of the frame with the greatest offset (the later one on a tie).
fn last_frame(body: &[Token]) -> Option<Vec<Declaration>> {
    let mut best: Option<(f64, Vec<Declaration>)> = None;
    let mut i = 0;
    while i < body.len() {
        let Some(open) = body[i..]
            .iter()
            .position(|t| *t == Token::LeftBrace)
            .map(|p| i + p)
        else {
            break;
        };
        let len = matching(&body[open + 1..])?;
        let mut offset: Option<f64> = None;
        for t in &body[i..open] {
            let o = match t {
                Token::Ident(w) if w.eq_ignore_ascii_case("from") => Some(0.0),
                Token::Ident(w) if w.eq_ignore_ascii_case("to") => Some(100.0),
                Token::Percentage(p) => Some(*p),
                _ => None,
            };
            if let Some(o) = o {
                offset = Some(offset.map_or(o, |b: f64| b.max(o)));
            }
        }
        if let Some(offset) = offset {
            if best.as_ref().is_none_or(|(b, _)| offset >= *b) {
                best = Some((offset, parse_declarations(&body[open + 1..open + 1 + len])));
            }
        }
        i = open + 1 + len + 1;
    }
    best.map(|(_, d)| d)
}

/// The names of the animations in one `animation` value that end on their last frame.
fn finishing_names(value: &[Token]) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split(|t| *t == Token::Comma) {
        let (mut name, mut keeps_end, mut infinite, mut depth) = (None, false, false, 0usize);
        let mut previous_was_name_candidate = None;
        for (k, t) in part.iter().enumerate() {
            match t {
                Token::LeftParen => depth += 1,
                Token::RightParen => depth = depth.saturating_sub(1),
                Token::Ident(w) if depth == 0 => {
                    let lower = w.to_ascii_lowercase();
                    let is_function = part.get(k + 1) == Some(&Token::LeftParen);
                    if is_function {
                        previous_was_name_candidate = None;
                    } else if lower == "forwards" || lower == "both" {
                        keeps_end = true;
                    } else if lower == "infinite" {
                        infinite = true;
                    } else if !KEYWORDS.contains(&lower.as_str())
                        && previous_was_name_candidate.is_none()
                    {
                        previous_was_name_candidate = Some(w.clone());
                        name = Some(w.clone());
                    }
                }
                _ => {}
            }
        }
        if let (Some(name), true, false) = (name, keeps_end, infinite) {
            out.push(name);
        }
    }
    out
}

/// Adds, to `rules`, the end state of every finished animation they use.
pub(crate) fn append_finished_animations(tokens: &[Token], rules: &mut Vec<Rule>) {
    if !tokens
        .iter()
        .any(|t| matches!(t, Token::AtKeyword(n) if n.to_ascii_lowercase().ends_with("keyframes")))
    {
        return;
    }
    let frames = final_frames(tokens);
    if frames.is_empty() {
        return;
    }
    let mut extra = Vec::new();
    for rule in rules.iter() {
        for d in &rule.declarations {
            if d.property != "animation" {
                continue;
            }
            let crate::value::Value::Unparsed(value) = &d.value else {
                continue;
            };
            for name in finishing_names(value) {
                if let Some(frame) = frames.get(&name) {
                    extra.push(Rule {
                        selectors: rule.selectors.clone(),
                        declarations: frame
                            .iter()
                            .map(|f| Declaration {
                                important: true,
                                ..f.clone()
                            })
                            .collect(),
                    });
                }
            }
        }
    }
    rules.extend(extra);
}

#[cfg(test)]
mod tests {
    use crate::parser::parse;
    use crate::value::Value;

    fn finals(css: &str) -> Vec<(String, Value, bool)> {
        parse(css)
            .into_iter()
            .skip_while(|r| !r.declarations.iter().any(|d| d.important))
            .flat_map(|r| r.declarations)
            .map(|d| (d.property, d.value, d.important))
            .collect()
    }

    const FADE: &str = "@keyframes fade { from { opacity: 0 } to { opacity: 1 } }";

    #[test]
    fn a_both_fill_animation_ends_on_its_last_keyframe() {
        let css = format!("canvas {{ opacity: 0; animation: 1s ease-out .2s both fade }} {FADE}");
        let end = finals(&css);
        assert_eq!(end.len(), 1, "{end:?}");
        assert_eq!(end[0].0, "opacity");
        assert_eq!(end[0].1, Value::Number(1.0));
        assert!(end[0].2, "it outranks the starting value");
    }

    #[test]
    fn the_keyframes_may_come_before_the_rule_and_forwards_counts() {
        let css = format!("{FADE} .a {{ animation: fade 2s linear forwards }}");
        assert_eq!(finals(&css).len(), 1);
    }

    #[test]
    fn the_greatest_offset_wins_whatever_the_source_order() {
        let css = "@keyframes k { 100% { opacity: 1 } 50% { opacity: 0.5 } 0% { opacity: 0 } } p { animation: k 1s both }";
        let end = finals(css);
        assert_eq!(end[0].1, Value::Number(1.0));
        let css =
            "@keyframes k { to { opacity: 1 } from { opacity: 0 } } p { animation: k 1s both }";
        assert_eq!(finals(css)[0].1, Value::Number(1.0));
    }

    #[test]
    fn an_animation_that_does_not_keep_its_end_or_never_ends_adds_nothing() {
        for value in [
            "fade 1s",
            "fade 1s backwards",
            "fade 1s infinite both",
            "fade 1s both infinite",
            "none",
        ] {
            let css = format!("{FADE} p {{ opacity: 0; animation: {value} }}");
            assert_eq!(parse(&css).len(), 1, "{value}");
        }
    }

    #[test]
    fn an_unknown_name_and_a_missing_block_add_nothing() {
        assert_eq!(parse("p { animation: ghost 1s both }").len(), 1);
        assert_eq!(
            parse(&format!("{FADE} p {{ animation: ghost 1s both }}")).len(),
            1
        );
    }

    #[test]
    fn a_list_of_animations_finishes_each_one_that_qualifies() {
        let css = "@keyframes a { to { opacity: 1 } } @keyframes b { to { color: red } } p { animation: a 1s both, b 1s infinite both, nope 1s }";
        let end = finals(css);
        assert_eq!(end.len(), 1, "{end:?}");
        assert_eq!(end[0].0, "opacity");
    }

    #[test]
    fn timing_functions_and_numbers_are_not_mistaken_for_the_name() {
        let css =
            format!("{FADE} p {{ animation: 300ms cubic-bezier(0.1, 0.7, 1, 0.1) 1 fade both }}");
        assert_eq!(finals(&css).len(), 1);
    }

    #[test]
    fn a_keyframes_prefix_is_accepted() {
        let css = "@-webkit-keyframes fade { to { opacity: 1 } } p { animation: fade 1s both }";
        assert_eq!(finals(css).len(), 1);
    }
}
