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
//! A list with only translations is applied to positions as above. Any `rotate`, `scale`,
//! `skew` or `matrix` (function or individual property) makes the whole list a 2D matrix
//! about `transform-origin` instead, kept on the fragment ([`Fragment::matrix`]) for painting
//! to apply: layout geometry stays that of the untransformed box, as in CSS. `perspective`
//! and 3D functions are treated as the identity.

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

/// A 2D affine matrix `[a, b, c, d, e, f]` as in CSS `matrix()`: `x' = a*x + c*y + e`,
/// `y' = b*x + d*y + f`.
pub type Matrix = [f64; 6];

pub(crate) const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` applied after `b` (`a * b`).
pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

/// One step of a transform list, with translations still relative to the box.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Translate(Amount, Amount),
    Rotate(f64),
    Scale(f64, f64),
    Skew(f64, f64),
    Matrix(Matrix),
}

/// A non-translation transform not yet resolved against its box.
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    ops: Vec<Op>,
    origin: (Amount, Amount),
}

impl Transform {
    /// The matrix for a box of `width` x `height`, in the box's own coordinates (its top-left
    /// is 0,0), with the origin folded in.
    pub fn matrix(&self, width: f64, height: f64) -> Matrix {
        let mut m = IDENTITY;
        for op in &self.ops {
            let step = match op {
                Op::Translate(x, y) => [1.0, 0.0, 0.0, 1.0, x.0 + x.1 * width, y.0 + y.1 * height],
                Op::Rotate(a) => [a.cos(), a.sin(), -a.sin(), a.cos(), 0.0, 0.0],
                Op::Scale(x, y) => [*x, 0.0, 0.0, *y, 0.0, 0.0],
                Op::Skew(x, y) => [1.0, y.tan(), x.tan(), 1.0, 0.0, 0.0],
                Op::Matrix(m) => *m,
            };
            m = multiply(m, step);
        }
        let ox = self.origin.0 .0 + self.origin.0 .1 * width;
        let oy = self.origin.1 .0 + self.origin.1 .1 * height;
        multiply(multiply([1.0, 0.0, 0.0, 1.0, ox, oy], m), [1.0, 0.0, 0.0, 1.0, -ox, -oy])
    }
}

/// What `style` asks for: a plain translation (applied to positions), a full 2D transform
/// (kept for painting), or nothing.
pub(crate) fn resolve(style: &Style<'_>) -> (Option<Translation>, Option<Transform>) {
    let mut ops: Vec<Op> = Vec::new();
    let mut full = false;
    // `translate`, `rotate` and `scale` apply before `transform`, in that order.
    if let Some(Value::Unparsed(tokens)) = style.raw("translate") {
        let args = split_values(tokens);
        let x = args.first().and_then(|a| axis(style, a));
        let y = args.get(1).and_then(|a| axis(style, a));
        if x.is_some() || y.is_some() {
            ops.push(Op::Translate(x.unwrap_or((0.0, 0.0)), y.unwrap_or((0.0, 0.0))));
        }
    }
    if let Some(Value::Unparsed(tokens)) = style.raw("rotate") {
        let args = split_values(tokens);
        if let Some(a) = args.last().and_then(|a| angle(a)) {
            ops.push(Op::Rotate(a));
            full = true;
        }
    }
    if let Some(Value::Unparsed(tokens)) = style.raw("scale") {
        let args = split_values(tokens);
        let n = |i: usize| args.get(i).and_then(|a| number(a));
        if let Some(x) = n(0) {
            ops.push(Op::Scale(x, n(1).unwrap_or(x)));
            full = true;
        }
    }
    if let Some(Value::Unparsed(tokens)) = style.raw("transform") {
        for (name, args) in functions(tokens) {
            let arg = |i: usize| args.get(i).and_then(|a| axis(style, a));
            let ang = |i: usize| args.get(i).and_then(|a| angle(a));
            let num = |i: usize| args.get(i).and_then(|a| number(a));
            let op = match name.as_str() {
                "translate" | "translate3d" => {
                    Op::Translate(arg(0).unwrap_or((0.0, 0.0)), arg(1).unwrap_or((0.0, 0.0)))
                }
                "translatex" => Op::Translate(arg(0).unwrap_or((0.0, 0.0)), (0.0, 0.0)),
                "translatey" => Op::Translate((0.0, 0.0), arg(0).unwrap_or((0.0, 0.0))),
                "rotate" | "rotatez" => match ang(0) {
                    Some(a) => Op::Rotate(a),
                    None => continue,
                },
                "scale" | "scale3d" => match num(0) {
                    Some(x) => Op::Scale(x, num(1).unwrap_or(x)),
                    None => continue,
                },
                "scalex" => match num(0) {
                    Some(x) => Op::Scale(x, 1.0),
                    None => continue,
                },
                "scaley" => match num(0) {
                    Some(y) => Op::Scale(1.0, y),
                    None => continue,
                },
                "skew" => Op::Skew(ang(0).unwrap_or(0.0), ang(1).unwrap_or(0.0)),
                "skewx" => Op::Skew(ang(0).unwrap_or(0.0), 0.0),
                "skewy" => Op::Skew(0.0, ang(0).unwrap_or(0.0)),
                "matrix" if args.len() == 6 => {
                    let v: Vec<f64> = (0..6).filter_map(num).collect();
                    if v.len() != 6 {
                        continue;
                    }
                    Op::Matrix([v[0], v[1], v[2], v[3], v[4], v[5]])
                }
                _ => continue,
            };
            full |= !matches!(op, Op::Translate(..));
            ops.push(op);
        }
    }
    if ops.is_empty() {
        return (None, None);
    }
    if !full {
        return (translation(style), None);
    }
    (None, Some(Transform { ops, origin: origin(style) }))
}

/// `transform-origin`: one or two of keywords, lengths and percentages; the middle by default.
fn origin(style: &Style<'_>) -> (Amount, Amount) {
    let mut x: Amount = (0.0, 0.5);
    let mut y: Amount = (0.0, 0.5);
    if let Some(Value::Unparsed(tokens)) = style.raw("transform-origin") {
        let parts = split_values(tokens);
        let keyword = |t: &[Token]| match t {
            [Token::Ident(k)] => match k.to_ascii_lowercase().as_str() {
                "left" => Some((Some(true), (0.0, 0.0))),
                "right" => Some((Some(true), (0.0, 1.0))),
                "top" => Some((Some(false), (0.0, 0.0))),
                "bottom" => Some((Some(false), (0.0, 1.0))),
                "center" => Some((None, (0.0, 0.5))),
                _ => None,
            },
            _ => None,
        };
        let mut resolved: Vec<(Option<bool>, Amount)> = Vec::new();
        for part in parts.iter().take(2) {
            if let Some(k) = keyword(part) {
                resolved.push(k);
            } else if let Some(a) = axis(style, part) {
                resolved.push((None, a));
            }
        }
        match resolved.as_slice() {
            [(h, a)] => match h {
                Some(false) => y = *a,
                _ => x = *a,
            },
            [(h0, a0), (h1, a1)] => {
                // `top left` puts the vertical keyword first.
                if *h0 == Some(false) || *h1 == Some(true) {
                    y = *a0;
                    x = *a1;
                } else {
                    x = *a0;
                    y = *a1;
                }
            }
            _ => {}
        }
    }
    (x, y)
}

fn trimmed(tokens: &[Token]) -> &[Token] {
    let start = tokens.iter().position(|t| !matches!(t, Token::Whitespace)).unwrap_or(tokens.len());
    let end = tokens.iter().rposition(|t| !matches!(t, Token::Whitespace)).map_or(start, |e| e + 1);
    &tokens[start..end]
}

/// An angle in radians from `45deg`, `0.5turn`, `1rad`, `100grad` (or a bare `0`).
fn angle(tokens: &[Token]) -> Option<f64> {
    match trimmed(tokens) {
        [Token::Number(n)] if *n == 0.0 => Some(0.0),
        [Token::Dimension(n, unit)] => Some(match unit.to_ascii_lowercase().as_str() {
            "deg" => n.to_radians(),
            "rad" => *n,
            "turn" => n * std::f64::consts::TAU,
            "grad" => n * std::f64::consts::PI / 200.0,
            _ => return None,
        }),
        _ => None,
    }
}

/// A plain number, or a percentage as its fraction (`scale: 50%`).
fn number(tokens: &[Token]) -> Option<f64> {
    match trimmed(tokens) {
        [Token::Number(n)] => Some(*n),
        [Token::Percentage(p)] => Some(p / 100.0),
        _ => None,
    }
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
pub type Amount = (f64, f64);

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
    let mut values: Vec<Vec<Token>> = Vec::new();
    let mut depth = 0usize;
    let mut open = false;
    for token in tokens {
        match token {
            Token::Whitespace if depth == 0 => open = false,
            other => {
                if !open {
                    values.push(Vec::new());
                    open = true;
                }
                match other {
                    Token::LeftParen => depth += 1,
                    Token::RightParen => depth = depth.saturating_sub(1),
                    _ => {}
                }
                values.last_mut().expect("a value is open").push(other.clone());
            }
        }
    }
    values
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
    if let Some(t) = fragment.transform.take() {
        let m = t.matrix(fragment.width, fragment.height);
        fragment.matrix = Some(m);
    }
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
        let (x, y, _) = placed(html, css, id);
        (x, y)
    }

    fn placed(html: &str, css: &str, id: &str) -> (f64, f64, Option<Matrix>) {
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
        (found.x, found.y, found.matrix)
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
    fn none_and_three_d_functions_leave_the_box_where_it_was() {
        assert_eq!(moved("transform: none"), (0.0, 0.0));
        assert_eq!(moved("transform: perspective(100px)"), (0.0, 0.0));
        assert_eq!(matrix_of("transform: perspective(100px)"), None);
        assert_eq!(moved("transform: rotate(45deg)"), (0.0, 0.0), "a rotation does not move the layout box");
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

    fn matrix_of(css: &str) -> Option<Matrix> {
        placed(PAGE, &format!("#a{{width:100px;height:40px;font-size:10px;{css}}}"), "a").2
    }

    fn map(m: Matrix, x: f64, y: f64) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    fn close(a: (f64, f64), b: (f64, f64)) {
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{a:?} vs {b:?}");
    }

    #[test]
    fn a_rotation_turns_the_box_about_its_centre() {
        let m = matrix_of("transform: rotate(90deg)").expect("a matrix");
        // The centre (50, 20) stays; the top-left corner swings to the top-right.
        close(map(m, 50.0, 20.0), (50.0, 20.0));
        close(map(m, 0.0, 0.0), (70.0, -30.0));
    }

    #[test]
    fn a_translation_alone_still_moves_positions_and_has_no_matrix() {
        assert_eq!(matrix_of("transform: translate(5px, 5px)"), None);
    }

    #[test]
    fn scale_grows_the_box_about_its_origin() {
        let centred = matrix_of("transform: scale(2)").unwrap();
        close(map(centred, 0.0, 0.0), (-50.0, -20.0));
        let corner = matrix_of("transform: scale(2, 3); transform-origin: 0 0").unwrap();
        close(map(corner, 10.0, 10.0), (20.0, 30.0));
    }

    #[test]
    fn transform_origin_keywords_and_percentages_pick_the_pivot() {
        let m = matrix_of("transform: rotate(180deg); transform-origin: left top").unwrap();
        close(map(m, 10.0, 10.0), (-10.0, -10.0));
        let m = matrix_of("transform: rotate(180deg); transform-origin: 100% 100%").unwrap();
        close(map(m, 100.0, 40.0), (100.0, 40.0));
        close(map(m, 0.0, 0.0), (200.0, 80.0));
    }

    #[test]
    fn the_individual_rotate_and_scale_properties_apply_before_transform() {
        let m = matrix_of("rotate: 0.25turn; scale: 2").unwrap();
        close(map(m, 50.0, 20.0), (50.0, 20.0));
        let both = matrix_of("rotate: 90deg; transform: translate(10px, 0)").unwrap();
        // Rotation first (outermost), then the translation inside it: +x becomes +y.
        close(map(both, 50.0, 20.0), (50.0, 30.0));
    }

    #[test]
    fn skew_and_matrix_functions_are_understood() {
        let skew = matrix_of("transform: skewX(45deg); transform-origin: 0 0").unwrap();
        close(map(skew, 0.0, 10.0), (10.0, 10.0));
        let m = matrix_of("transform: matrix(1, 0, 0, 1, 7, 9)").unwrap();
        close(map(m, 0.0, 0.0), (7.0, 9.0));
    }

    #[test]
    fn a_transform_does_not_move_its_neighbours() {
        let base = position(PAGE, "#a{width:100px;height:40px}", "b");
        let with = position(PAGE, "#a{width:100px;height:40px;transform:rotate(30deg) scale(3)}", "b");
        assert_eq!(base, with);
    }
}
