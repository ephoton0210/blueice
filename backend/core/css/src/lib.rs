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

mod cascade;
mod parser;
mod selector;
mod tokenizer;
mod value;

pub use cascade::{cascade, ua_stylesheet, ComputedStyle, Origin};
pub use parser::{Declaration, Rule};
pub use selector::{ComplexSelector, Compound, SimpleSelector, Specificity};
pub use value::{Color, Length, Value};

/// A parsed CSS stylesheet: its rules, in source order. Not yet matched
/// against any DOM -- see [`cascade`] for that.
#[derive(Debug, Clone, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

pub fn parse(input: &str) -> Stylesheet {
    Stylesheet { rules: parser::parse(input) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_parse_api_produces_a_stylesheet() {
        let sheet = parse("p { color: red; }");
        assert_eq!(sheet.rules.len(), 1);
    }
}
