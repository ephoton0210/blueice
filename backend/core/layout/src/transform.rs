// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS `transform` and `translate`, translation only.
//!
//! A translation moves a box and everything inside it without changing the layout of
//! anything else, and a [`Fragment`]'s `x`/`y` are relative to its parent, so applying one is
//! adding to those two numbers: painting, hit-testing and the AI representation, which all
//! read fragment bounds, follow without knowing transforms exist. It is applied once, after
//! the whole tree is placed (a parent assigns its children's positions after they are laid
//! out), and percentages are of the box's own border box.
//!
//! Not supported, and treated as the identity: `rotate`, `scale`, `skew`, `matrix`,
//! `perspective` and `transform-origin` (which only matters to those). A transform list with
//! a function this does not know still applies the translations in it.

use crate::ctx::Style;
use crate::fragment::Fragment;
use blueice_css::{Token, Value};

/// A translation not yet applied: `px + fraction * (the box's own size)` on each axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Translation {
    pub x_px: f64,
    pub x_fraction: f64,
    pub y_px: f64,
    pub y_fraction: f64,
}

/// The translation `style` asks for, or `None` when it is the identity.
pub(crate) fn translation(style: &Style<'_>) -> Option<Translation> {
    let mut total = Translation::default();
    let mut any = false;
    // `translate` applies before `transform`.
    if let Some(Value::Unparsed(tokens)) = style.raw("translate") {
        let args = split_values(tokens);
        let x = args.first().and_then(|a| axis(style, a));
        let y = args.get(1).and_then(|a| axis(style, a));
        any |= add(&mut total, x, y);
    }
    if let Some(Value::Unparsed(tokens)) = style.raw("transform") {
        for (name, args) in functions(tokens) {
            let arg = |i: usize| args.get(i).and_then(|a| axis(style, a));
            let (x, y) = match name.as_str() {
                "translate" | "translate3d" => (arg(0), arg(1)),
                "translatex" => (arg(0), None),
                "translatey" => (None, arg(0)),
                _ => continue,
            };
            any |= add(&mut total, x, y);
        }
    }
    any.then_some(total)
}

/// One axis's amount as `(px, fraction of the box's own size)`.
type Amount = (f64, f64);

fn add(total: &mut Translation, x: Option<Amount>, y: Option<Amount>) -> bool {
    if let Some((px, fraction)) = x {
        total.x_px += px;
        total.x_fraction += fraction;
    }
    if let Some((px, fraction)) = y {
        total.y_px += px;
        total.y_fraction += fraction;
    }
    x.is_some() || y.is_some()
}

/// A length, percentage or `calc()` as pixels plus a fraction of the box's size. Resolving it
/// against a base of 0 and again against 100 separates the two, since both are linear in the
/// base (which also covers `calc(100% - 4px)`).
fn axis(style: &Style<'_>, tokens: &[Token]) -> Option<Amount> {
    let value = blueice_css::parse_value(tokens)?;
    let at = |base: f64| value.to_px(&style.length_context(base));
    let px = at(0.0)?;
    Some((px, (at(100.0)? - px) / 100.0))
}

/// The space-separated values of an individual `translate` property.
fn split_values(tokens: &[Token]) -> Vec<Vec<Token>> {
    tokens
        .iter()
        .filter(|t| **t != Token::Whitespace)
        .map(|t| vec![t.clone()])
        .collect()
}

/// The function calls in a transform list, as lowercased name and comma-separated arguments.
fn functions(tokens: &[Token]) -> Vec<(String, Vec<Vec<Token>>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let (Token::Ident(name), Some(Token::LeftParen)) = (&tokens[i], tokens.get(i + 1)) else {
            i += 1;
            continue;
        };
        let mut depth = 0usize;
        let mut args: Vec<Vec<Token>> = vec![Vec::new()];
        let mut j = i + 1;
        while j < tokens.len() {
            match &tokens[j] {
                Token::LeftParen => {
                    depth += 1;
                    if depth > 1 {
                        args.last_mut().unwrap().push(Token::LeftParen);
                    }
                }
                Token::RightParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    args.last_mut().unwrap().push(Token::RightParen);
                }
                Token::Comma if depth == 1 => args.push(Vec::new()),
                other => args.last_mut().unwrap().push(other.clone()),
            }
            j += 1;
        }
        out.push((name.to_ascii_lowercase(), args));
        i = j + 1;
    }
    out
}

/// Applies every pending translation in the tree under (and including) `fragment`.
pub(crate) fn apply(fragment: &mut Fragment) {
    if let Some(t) = fragment.translate.take() {
        fragment.x += t.x_px + t.x_fraction * fragment.width;
        fragment.y += t.y_px + t.y_fraction * fragment.height;
    }
    for child in &mut fragment.children {
        apply(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout, Constraints};
    use blueice_dom::{Document, NodeData, NodeId};

    /// The position of the element with `id` after a whole layout, relative to its parent.
    fn position(html: &str, css: &str, id: &str) -> (f64, f64) {
        let doc = blueice_html::parse(html);
        let ua = blueice_css::ua_stylesheet();
        let author = blueice_css::parse(css).rules;
        let styles = blueice_css::cascade(
            &doc,
            &[
                (blueice_css::Origin::Ua, &ua),
                (blueice_css::Origin::Author, &author),
            ],
        );
        let root = layout(
            &doc,
            doc.root(),
            &styles,
            Constraints {
                available_width: 800.0,
                viewport_height: 600.0,
            },
        );
        let wanted = find(&doc, doc.root(), id).expect("an element with that id");
        let found = locate(&root, wanted).expect("a fragment for it");
        (found.x, found.y)
    }

    fn find(doc: &Document, from: NodeId, id: &str) -> Option<NodeId> {
        if let NodeData::Element { attributes, .. } = doc.data(from) {
            if attributes.iter().any(|(k, v)| k == "id" && v == id) {
                return Some(from);
            }
        }
        doc.children(from).find_map(|c| find(doc, c, id))
    }

    fn locate(f: &Fragment, node: NodeId) -> Option<&Fragment> {
        if f.node == Some(node) {
            return Some(f);
        }
        f.children.iter().find_map(|c| locate(c, node))
    }

    const PAGE: &str = "<div id=a>x</div><div id=b>y</div>";

    fn moved(css: &str) -> (f64, f64) {
        let base = position(PAGE, "#a{width:100px;height:40px;font-size:10px}", "a");
        let with = position(
            PAGE,
            &format!("#a{{width:100px;height:40px;font-size:10px;{css}}}"),
            "a",
        );
        (with.0 - base.0, with.1 - base.1)
    }

    #[test]
    fn translate_moves_the_box_by_its_lengths() {
        assert_eq!(moved("transform: translate(10px, 5px)"), (10.0, 5.0));
        assert_eq!(moved("transform: translate(-7px, 0)"), (-7.0, 0.0));
    }

    #[test]
    fn a_translate_with_one_argument_has_no_vertical_part() {
        assert_eq!(moved("transform: translate(8px)"), (8.0, 0.0));
    }

    #[test]
    fn the_axis_and_three_dimensional_forms() {
        assert_eq!(moved("transform: translateX(6px)"), (6.0, 0.0));
        assert_eq!(moved("transform: translateY(-3px)"), (0.0, -3.0));
        assert_eq!(moved("transform: translate3d(1px, 2px, 99px)"), (1.0, 2.0));
    }

    #[test]
    fn percentages_are_of_the_boxs_own_border_box() {
        assert_eq!(moved("transform: translate(-50%, -50%)"), (-50.0, -20.0));
        assert_eq!(moved("transform: translateX(25%)"), (25.0, 0.0));
    }

    #[test]
    fn other_units_resolve_as_they_do_elsewhere() {
        assert_eq!(moved("transform: translate(2em, 1em)"), (20.0, 10.0));
    }

    #[test]
    fn a_list_of_translations_adds_up() {
        assert_eq!(
            moved("transform: translateX(5px) translateY(7px) translate(1px, 1px)"),
            (6.0, 8.0)
        );
    }

    #[test]
    fn the_individual_translate_property_works_too() {
        assert_eq!(moved("translate: 3px 4px"), (3.0, 4.0));
        assert_eq!(moved("translate: 3px"), (3.0, 0.0));
        assert_eq!(moved("translate: 10% 50%"), (10.0, 20.0));
    }

    #[test]
    fn none_and_functions_not_supported_leave_the_box_where_it_was() {
        assert_eq!(moved("transform: none"), (0.0, 0.0));
        assert_eq!(moved("transform: rotate(45deg)"), (0.0, 0.0));
        assert_eq!(moved("transform: scale(2)"), (0.0, 0.0));
        assert_eq!(
            moved("transform: rotate(45deg) translateX(9px)"),
            (9.0, 0.0),
            "the translation in a list with an unsupported function still applies"
        );
    }

    #[test]
    fn siblings_and_the_parents_size_do_not_move() {
        let (_, b_before) = position(PAGE, "#a{height:40px}", "b");
        let (_, b_after) = position(PAGE, "#a{height:40px;transform:translate(30px,30px)}", "b");
        assert_eq!(b_before, b_after);
    }

    #[test]
    fn a_boxs_children_keep_their_place_inside_it() {
        let html = "<div id=a><p id=c>t</p></div>";
        let css = "#a{width:100px;height:40px}";
        let (cx, cy) = position(html, css, "c");
        let shifted = format!("{css}#a{{transform:translate(20px,10px)}}");
        assert_eq!(
            position(html, &shifted, "c"),
            (cx, cy),
            "relative to the parent"
        );
        let (ax, ay) = position(html, css, "a");
        assert_eq!(
            position(html, &shifted, "a"),
            (ax + 20.0, ay + 10.0),
            "the parent moved"
        );
    }

    #[test]
    fn a_centered_absolutely_positioned_box_the_way_a_tooltip_is() {
        let html = "<div id=box><div id=card>c</div></div>";
        let css = "#box{position:relative;width:200px;height:100px} \
                   #card{position:absolute;top:50%;left:50%;width:100px;height:40px;\
                   transform:translate(-50%,-50%)}";
        assert_eq!(position(html, css, "card"), (50.0, 30.0));
    }

    #[test]
    fn a_flex_item_can_be_translated() {
        let html = "<div id=f><i id=a>a</i><i id=b>b</i></div>";
        let css = "#f{display:flex} i{width:30px;height:10px}";
        let (ax, ay) = position(html, css, "a");
        let shifted = format!("{css}#a{{transform:translate(4px,5px)}}");
        assert_eq!(position(html, &shifted, "a"), (ax + 4.0, ay + 5.0));
        assert_eq!(position(html, &shifted, "b"), position(html, css, "b"));
    }

    #[test]
    fn applying_twice_moves_a_fragment_once() {
        let mut f = Fragment::empty_block();
        f.width = 10.0;
        f.translate = Some(Translation {
            x_px: 2.0,
            x_fraction: 0.5,
            ..Translation::default()
        });
        apply(&mut f);
        apply(&mut f);
        assert_eq!((f.x, f.y), (7.0, 0.0));
        assert_eq!(f.translate, None);
    }
}
