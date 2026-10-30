// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS parsing and cascade.
//!
//! See `development/browser_core/phase-2-mvp-scope/PLAN.md`'s "MVP CSS
//! scope" for exactly which selectors/properties/values are supported,
//! and `development/browser_core/research/css-cascade.md` for the
//! architecture this ports: Stylo's and Blink's independently-converged
//! packed-specificity / cascade-origin design, as a lightweight
//! from-scratch implementation rather than vendoring the `style` crate
//! directly (its `TElement` trait alone has 82 methods to implement
//! against a new DOM).
//!
//! Public surface, by pipeline stage:
//! - [`parse`] -- tokenize + parse CSS text into a [`Stylesheet`] (just
//!   the rules; not yet matched against any particular DOM).
//! - [`ua_stylesheet`] -- the built-in default stylesheet for the Phase
//!   2 HTML element list.
//! - [`cascade`] -- combine one or more stylesheets (in [`Origin`]
//!   order) with a `blueice_dom::Document` into a per-element
//!   [`ComputedStyle`] map, ready for `blueice-layout` once it exists.

mod animation;
mod calc;
mod cascade;
mod color;
mod content;
mod gradient;
mod media;
mod parser;
mod selector;
mod supports;
mod tokenizer;
mod value;
mod vars;

pub use calc::Calc;
pub use cascade::{
    cascade, cascade_incremental, cascade_with_viewport, is_pseudo_node, pseudo_kind,
    pseudo_node_id, pseudo_owner, pseudo_text_node_id, ua_stylesheet, ComputedStyle, Origin,
};
pub use content::{generated_text, url_of};
pub use gradient::{
    parse_gradient, parse_shadows, ColorStop, Gradient, GradientKind, LinearDirection, Shadow,
    StopPosition,
};
pub use media::{matches_media, MediaEnvironment};
pub use parser::{Declaration, Rule};
pub use selector::{
    matches, matches_pseudo_element, ComplexSelector, Compound, PseudoElement, SimpleSelector,
    Specificity,
};
pub use tokenizer::Token;

/// Parses a selector list (`a.b > c, d`); selectors the engine does not support
/// are dropped, like in a stylesheet.
pub fn parse_selectors(text: &str) -> Vec<ComplexSelector> {
    selector::parse_selector_list(&tokenizer::tokenize(text))
}
pub use value::{Color, Length, LengthContext, Value};

/// Parses one CSS colour (`#rgb`, `rgb()`, a name, ...) as a script gives it to
/// a canvas; `None` for anything that is not a colour or is `currentColor`.
pub fn parse_css_color(text: &str) -> Option<(u8, u8, u8, u8)> {
    match color::parse_color(&tokenizer::tokenize(text.trim())) {
        Some(Color::Rgba(r, g, b, a)) => Some((r, g, b, a)),
        _ => None,
    }
}

/// A parsed CSS stylesheet: its rules, in source order. Not yet matched
/// against any DOM -- see [`cascade`] for that.
#[derive(Debug, Clone, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

pub fn parse(input: &str) -> Stylesheet {
    Stylesheet {
        rules: parser::parse(input),
    }
}

/// Resolve conditional rules against the current screen environment while
/// retaining the legacy parser's at-rule-skipping API.
pub fn parse_with_environment(input: &str, environment: &MediaEnvironment) -> Stylesheet {
    Stylesheet {
        rules: parser::parse_with_environment(input, environment),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_colour_string_parses_or_is_refused() {
        assert_eq!(parse_css_color(" #ff8000 "), Some((255, 128, 0, 255)));
        assert_eq!(
            parse_css_color("rgba(0, 0, 255, 0.5)"),
            Some((0, 0, 255, 128))
        );
        assert_eq!(parse_css_color("red"), Some((255, 0, 0, 255)));
        assert_eq!(parse_css_color("not a colour"), None);
        assert_eq!(parse_css_color("currentColor"), None);
    }

    #[test]
    fn public_parse_api_produces_a_stylesheet() {
        let sheet = parse("p { color: red; }");
        assert_eq!(sheet.rules.len(), 1);
    }
}
