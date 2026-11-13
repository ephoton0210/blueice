// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inline formatting: a run of inline-level content (text, inline elements,
//! atomic inline boxes such as `inline-block` and replaced elements, `<br>`)
//! becomes a stack of line boxes.
//!
//! Text is cut into words and collapsible spaces following `white-space`;
//! lines break greedily at spaces and between CJK characters; each line sits
//! on one baseline shared by text of every size and by atomic boxes (with
//! `vertical-align`), its height the tallest ascent plus the deepest descent
//! including the container's strut; `text-align` and `text-indent` place the
//! content on the line.

use crate::bidi::{self, Base};
use crate::block::{layout_block_level, Constraint, Forced, PendingAbs};
use crate::ctx::{Ctx, Display, Style};
use crate::float::{self, FloatRect};
use crate::fragment::{Fragment, FragmentKind};
use blueice_dom::{NodeData, NodeId};

/// One piece of an inline run, before line breaking.
enum Piece {
    Word {
        text: String,
        width: f64,
        style_node: NodeId,
        /// A break is allowed between the previous piece and this one even
        /// without a space (CJK text).
        can_break_before: bool,
        wrap: bool,
        /// How much of its width may hang outside a line.
        hang: Hang,
        /// The share of the font size it is drawn at (synthesized small capitals).
        scale: f64,
        /// Fullwidth punctuation drawn with the font's `halt`/`vhal` alternates, its blank half
        /// trimmed (`text-spacing-trim`).
        trim: bool,
    },
    Space {
        width: f64,
        style_node: NodeId,
        wrap: bool,
    },
    /// Horizontal padding, border or margin of an inline box: unbreakable.
    Gap(f64),
    /// The same for an inline box that paints something of its own (a
    /// background, border, radius or shadow): `outside` of `width` is margin,
    /// the rest is the box's own border and padding.
    Edge {
        node: NodeId,
        open: bool,
        width: f64,
        outside: f64,
    },
    Atomic(Box<Atomic>),
    Break,
    /// A bidi formatting character (embedding, isolate or override boundary) of a styled
    /// inline box: it takes no room and is never placed, but it shapes the levels around it.
    Mark(char),
}

/// An atomic inline: laid out as a block-level box and placed whole on a line.
struct Atomic {
    fragment: Fragment,
    margin_left: f64,
    margin_right: f64,
    margin_top: f64,
    margin_bottom: f64,
    /// Distance from the top of the border box to the baseline it aligns by.
    baseline: f64,
    align: Align,
    pending: Vec<PendingAbs>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Align {
    /// Raised above the baseline by this much (negative: lowered).
    Baseline(f64),
    Top,
    Bottom,
}

/// What laying out a run produced.
pub(crate) struct InlineResult {
    /// Line fragments, `y` from the top of the run.
    pub lines: Vec<Fragment>,
    pub height: f64,
    pub pending_abs: Vec<PendingAbs>,
    /// The farthest right edge any line reaches.
    pub extent: f64,
}

struct State {
    /// The last piece was a collapsible space (or the run just began), so
    /// another collapsible space is dropped.
    last_space: bool,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

/// What of a word may hang outside the line it is on (`hanging-punctuation`), in pixels.
#[derive(Clone, Copy, Default)]
struct Hang {
    /// A stop or comma at its end, hanging past the end of a line.
    end: f64,
    /// Whether `end` hangs even when it fits (`force-end`).
    force: bool,
    /// A closing bracket or quote at its end, hanging past the end of the last line.
    last: f64,
    /// An opening bracket or quote at its start, hanging before the start of the first line.
    first: f64,
    /// The blank half of an opening mark at its start (`text-spacing-trim: trim-start`),
    /// hanging before the start of any line.
    start_trim: f64,
    /// The blank half of a closing mark at its end, hanging past the end of a line: always for
    /// `trim-both`, only when it does not otherwise fit for the others.
    end_trim: f64,
    end_trim_force: bool,
}

/// The kinds of punctuation `text-spacing-trim` tells apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrimClass {
    Opening,
    Closing,
    MiddleDot,
    IdeographicSpace,
    /// An opening bracket of any width, which a fullwidth opening mark after it makes half.
    OtherOpen,
    /// A closing bracket of any width.
    OtherClose,
}

fn trim_class(c: char) -> Option<TrimClass> {
    Some(match c {
        '\u{3008}' | '\u{300A}' | '\u{300C}' | '\u{300E}' | '\u{3010}' | '\u{3014}' | '\u{3016}'
        | '\u{3018}' | '\u{301A}' | '\u{301D}' | '\u{FF08}' | '\u{FF3B}' | '\u{FF5B}' | '\u{FF5F}'
        | '\u{2018}' | '\u{201C}' => TrimClass::Opening,
        '\u{3009}' | '\u{300B}' | '\u{300D}' | '\u{300F}' | '\u{3011}' | '\u{3015}' | '\u{3017}'
        | '\u{3019}' | '\u{301B}' | '\u{301E}' | '\u{301F}' | '\u{FF09}' | '\u{FF3D}' | '\u{FF5D}'
        | '\u{FF60}' | '\u{2019}' | '\u{201D}'
        // colons and dots, which sit to one side of their cell like closing marks
        | '\u{FF1A}' | '\u{FF1B}' | '\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}' => TrimClass::Closing,
        '\u{00B7}' | '\u{2027}' | '\u{30FB}' => TrimClass::MiddleDot,
        '\u{3000}' => TrimClass::IdeographicSpace,
        '(' | '[' | '{' => TrimClass::OtherOpen,
        ')' | ']' | '}' => TrimClass::OtherClose,
        _ => return None,
    })
}

/// Opening brackets and quotes, which `hanging-punctuation: first` lets hang before a line.
fn is_opening(c: char) -> bool {
    no_line_end(c)
}

/// Closing brackets and quotes, which `hanging-punctuation: last` lets hang after a line.
fn is_closing(c: char) -> bool {
    matches!(
        c,
        '）' | '」' | '』' | '》' | '〉' | '】' | '〕' | '］' | '｝' | '”' | '’' | '〞' | ')' | ']'
    )
}

/// The size synthesized small capitals are drawn at, as a share of the font size.
const SMALL_CAPS_SCALE: f64 = 0.7;

/// Stops and commas, which `hanging-punctuation` lets hang past the end of a line.
fn hangs_at_line_end(c: char) -> bool {
    matches!(c, '，' | '。' | '、' | '．' | ',' | '.' | '､' | '｡')
}

/// Opening punctuation that must not end a line.
fn no_line_end(c: char) -> bool {
    matches!(
        c,
        '（' | '「' | '『' | '《' | '〈' | '【' | '〔' | '［' | '｛' | '“' | '‘' | '〝' | '(' | '['
    )
}

/// Punctuation that must not start a line.
fn no_line_start(c: char) -> bool {
    matches!(
        c,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '！'
            | '？'
            | '）'
            | '」'
            | '』'
            | '》'
            | '〉'
            | '】'
            | '〕'
            | '…'
    )
}

fn is_collapsible_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn transform_text(text: &str, transform: Option<&str>) -> String {
    match transform {
        Some("uppercase") => text.to_uppercase(),
        Some("lowercase") => text.to_lowercase(),
        Some("capitalize") => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for c in text.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        _ => text.to_string(),
    }
}

#[derive(Clone)]
struct TextProps {
    font_size: f64,
    /// Capitals the face cannot supply are made of capitals at 70% of the size: which letters
    /// shrink depends on the kind (`small-caps`, `all-small-caps`, `unicase`).
    synthesized_small_caps: Option<blueice_css::SynthesizedCaps>,
    bold: bool,
    italic: bool,
    face: Option<blueice_font::web::WebFontId>,
    letter_spacing: f64,
    word_spacing: f64,
    /// In a vertical line, which characters are set side by side in one em.
    combine: Option<crate::ctx::Combine>,
    /// In a vertical line, which characters stand upright (`text-orientation`).
    vertical: Option<String>,
    /// The OpenType features the text asks for.
    features: blueice_font::FeaturesId,
    /// The same with the font's half-width alternates (`halt`, or `vhal` in a vertical line).
    trim_features: blueice_font::FeaturesId,
    trim_mode: crate::ctx::SpacingTrim,
    hanging: crate::ctx::Hanging,
}

impl TextProps {
    fn of(style: &Style<'_>) -> TextProps {
        let (bold, italic, face) = (
            style.style.is_some_and(|s| s.is_bold()),
            style.style.is_some_and(|s| s.is_italic()),
            style.web_face(),
        );
        // What the style asks for given what its face supports: the OpenType features, and
        // capitals made by scaling where the face has none (small, petite and unicase capitals).
        let supports = |tag: [u8; 4]| blueice_font::face_supports_feature(bold, italic, face, tag);
        let text_features = style.style.map(|s| blueice_css::text_features(s, &supports));
        let synthesized_small_caps = text_features.as_ref().and_then(|t| t.synthesized_caps);
        TextProps {
            font_size: style.font_size(),
            synthesized_small_caps,
            bold: style.style.is_some_and(|s| s.is_bold()),
            italic: style.style.is_some_and(|s| s.is_italic()),
            face: style.web_face(),
            letter_spacing: style.px("letter-spacing", f64::NAN).unwrap_or(0.0),
            word_spacing: style.px("word-spacing", f64::NAN).unwrap_or(0.0),
            combine: style.combine_upright(),
            vertical: style.vertical_orientation().map(str::to_string),
            features: text_features
                .as_ref()
                .map(|t| blueice_font::register_features(&t.features))
                .unwrap_or_default(),
            trim_features: {
                let mut list = text_features.as_ref().map(|t| t.features.clone()).unwrap_or_default();
                let tag = if style.vertical_orientation().is_some() { *b"vhal" } else { *b"halt" };
                list.retain(|(t, _)| *t != tag);
                list.push((tag, 1));
                blueice_font::register_features(&list)
            },
            trim_mode: style.text_spacing_trim(),
            hanging: style.hanging(),
        }
    }

    fn width(&self, text: &str) -> f64 {
        let plain = |t: &str| {
            blueice_font::measure_text_width_features(
                t,
                self.font_size,
                self.bold,
                self.italic,
                self.face,
                self.features,
            ) + self.letter_spacing * t.chars().count() as f64
        };
        // In a vertical line an upright character is as long as the font's vertical advance.
        let upright_aware = |t: &str| match &self.vertical {
            Some(orientation) => {
                // Runs of upright characters are set as wholes, so pairs can be kerned.
                let (mut total, mut sideways, mut upright) = (0.0, String::new(), String::new());
                let end_upright = |upright: &mut String, total: &mut f64| {
                    if !upright.is_empty() {
                        let run = std::mem::take(upright);
                        *total += crate::vertical::upright_length(
                            &run,
                            self.font_size,
                            self.bold,
                            self.italic,
                            self.face,
                            self.features,
                        ) + self.letter_spacing * run.chars().count() as f64;
                    }
                };
                for c in t.chars() {
                    if crate::vertical::orient_of(c, Some(orientation))
                        == crate::fragment::TextOrient::Upright
                    {
                        if !sideways.is_empty() {
                            total += plain(&std::mem::take(&mut sideways));
                        }
                        upright.push(c);
                    } else {
                        end_upright(&mut upright, &mut total);
                        sideways.push(c);
                    }
                }
                end_upright(&mut upright, &mut total);
                total + plain(&sideways)
            }
            None => plain(t),
        };
        match self.combine {
            // Combined characters take one em along the line between them.
            Some(rule) => crate::vertical::combine_runs(text, rule)
                .iter()
                .map(|(run, combined)| if *combined { self.font_size } else { upright_aware(run) })
                .sum(),
            None => upright_aware(text),
        }
    }

    /// This text with the half-width alternates of punctuation.
    fn trimmed(&self) -> TextProps {
        TextProps { features: self.trim_features, ..self.clone() }
    }

    /// This text at `scale` times its size.
    fn scaled(&self, scale: f64) -> TextProps {
        TextProps {
            font_size: self.font_size * scale,
            letter_spacing: self.letter_spacing * scale,
            ..self.clone()
        }
    }

    /// `word` cut into the stretches synthesized small caps draw at different sizes: the
    /// capitals made of lowercase letters (and, for `all-small-caps`, of capitals) at 70% of the
    /// size, the rest at full size.
    fn small_cap_runs(&self, word: &str) -> Vec<(String, f64)> {
        let Some(caps) = self.synthesized_small_caps else {
            return vec![(word.to_string(), 1.0)];
        };
        let mut runs: Vec<(String, f64)> = Vec::new();
        for c in word.chars() {
            // Unicase draws the capitals small and leaves the lowercase letters as they are.
            let small = if caps.unicase {
                c.is_uppercase()
            } else {
                c.is_lowercase() || (caps.all && c.is_uppercase())
            };
            let (text, scale) = if small {
                (c.to_uppercase().collect::<String>(), SMALL_CAPS_SCALE)
            } else {
                (c.to_string(), 1.0)
            };
            match runs.last_mut() {
                Some((run, s)) if *s == scale => run.push_str(&text),
                _ => runs.push((text, scale)),
            }
        }
        runs
    }

    /// What of `word` may hang outside a line.
    fn hang(&self, word: &str) -> Hang {
        use crate::ctx::HangEnd;
        let width = |c: char| self.width(&c.to_string());
        let mut hang = Hang::default();
        if let Some(c) = word.chars().last() {
            if self.hanging.end != HangEnd::None && hangs_at_line_end(c) {
                hang.end = width(c);
                hang.force = self.hanging.end == HangEnd::Force;
            }
            if self.hanging.last && is_closing(c) {
                hang.last = width(c);
            }
        }
        if let Some(c) = word.chars().next() {
            if self.hanging.first && is_opening(c) {
                hang.first = width(c);
            }
        }
        // The blank half of fullwidth marks at the edges of a line (`text-spacing-trim`).
        use crate::ctx::SpacingTrim as T;
        let blank = |c: char| {
            let (full, half) = (self.width(&c.to_string()), self.trimmed().width(&c.to_string()));
            (full - half).max(0.0)
        };
        if let Some(c) = word.chars().next() {
            if trim_class(c) == Some(TrimClass::Opening) && matches!(self.trim_mode, T::TrimStart | T::TrimBoth) {
                hang.start_trim = blank(c);
            }
        }
        if let Some(c) = word.chars().last() {
            if trim_class(c) == Some(TrimClass::Closing)
                && matches!(self.trim_mode, T::Normal | T::SpaceFirst | T::TrimStart | T::TrimBoth)
            {
                hang.end_trim = blank(c);
                hang.end_trim_force = self.trim_mode == T::TrimBoth;
            }
        }
        hang
    }

    fn space(&self) -> f64 {
        self.width(" ") + self.word_spacing
    }
}

fn push_text(
    ctx: &Ctx<'_>,
    data: &str,
    style_node: NodeId,
    pieces: &mut Vec<Piece>,
    state: &mut State,
) {
    let style = ctx.style(style_node);
    let white_space = style.keyword("white-space").unwrap_or("normal");
    let (collapse, newlines, wrap) = match white_space {
        "nowrap" => (true, false, false),
        "pre" => (false, true, false),
        "pre-wrap" | "break-spaces" => (false, true, true),
        "pre-line" => (true, true, true),
        _ => (true, false, true),
    };
    let text = transform_text(data, style.keyword("text-transform"));
    let props = TextProps::of(&style);
    let mut word = String::new();
    let mut previous_cjk = false;
    let flush = |word: &mut String, pieces: &mut Vec<Piece>, can_break: bool| {
        if !word.is_empty() {
            let whole = std::mem::take(word);
            for (k, (text, scale)) in props.small_cap_runs(&whole).into_iter().enumerate() {
                let sized = props.scaled(scale);
                let (width, hang) = (sized.width(&text), sized.hang(&text));
                pieces.push(Piece::Word {
                    text,
                    width,
                    style_node,
                    // Within a word the sizes change but a line cannot break.
                    can_break_before: can_break && k == 0,
                    wrap,
                    hang,
                    scale,
                    trim: false,
                });
            }
        }
    };
    let mut break_before_next_word = false;
    for c in text.chars() {
        if c == '\u{200b}' || c == '\u{feff}' || c == '\u{ad}' {
            continue;
        }
        if c == '\n' && newlines {
            flush(&mut word, pieces, break_before_next_word);
            break_before_next_word = false;
            pieces.push(Piece::Break);
            state.last_space = false;
            previous_cjk = false;
        } else if is_collapsible_space(c) {
            flush(&mut word, pieces, break_before_next_word);
            break_before_next_word = false;
            previous_cjk = false;
            if collapse {
                if !state.last_space {
                    pieces.push(Piece::Space {
                        width: props.space(),
                        style_node,
                        wrap,
                    });
                    state.last_space = true;
                }
            } else {
                let columns = if c == '\t' { 8.0 } else { 1.0 };
                pieces.push(Piece::Space {
                    width: props.space() * columns,
                    style_node,
                    wrap,
                });
                state.last_space = false;
            }
        } else {
            state.last_space = false;
            let cjk = is_cjk(c);
            // Break between CJK characters (not before closing punctuation).
            let boundary = wrap
                && (cjk || previous_cjk)
                && !word.is_empty()
                && !no_line_start(c)
                && !word.chars().last().is_some_and(no_line_end);
            if boundary || (cjk && !word.is_empty()) {
                flush(&mut word, pieces, break_before_next_word);
                break_before_next_word = boundary;
            }
            word.push(c);
            previous_cjk = cjk;
        }
    }
    flush(&mut word, pieces, break_before_next_word);
}

/// Intrinsic size of a replaced element, from CSS then attributes: width and height as the
/// layout frame sees them, so a picture on a vertical line has its sides traded.
pub(crate) fn replaced_size(
    ctx: &Ctx<'_>,
    node: NodeId,
    style: &Style<'_>,
    cb_width: f64,
) -> Option<(f64, f64)> {
    if !ctx.writing.get().is_vertical() {
        return replaced_page_size(ctx, node, style, cb_width);
    }
    let turned = ctx.writing.replace(crate::ctx::Writing::Horizontal);
    let size = replaced_page_size(ctx, node, style, cb_width);
    ctx.writing.set(turned);
    size.map(|(w, h)| (h, w))
}

/// [`replaced_size`] in the page's own frame.
fn replaced_page_size(
    ctx: &Ctx<'_>,
    node: NodeId,
    style: &Style<'_>,
    cb_width: f64,
) -> Option<(f64, f64)> {
    let tag = ctx.tag(node)?;
    let default = match tag {
        "img" | "image" => (0.0, 0.0),
        "svg" | "canvas" | "iframe" | "video" | "embed" | "object" => (300.0, 150.0),
        _ => return None,
    };
    let NodeData::Element { attributes, .. } = ctx.node(node) else {
        return None;
    };
    let attr = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.trim().trim_end_matches("px").parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let (attr_w, attr_h) = (attr("width"), attr("height"));
    // What the engine learned from the image itself, when it has loaded.
    let intrinsic = match style.raw("-blueice-intrinsic-size") {
        Some(blueice_css::Value::List(parts)) => match parts.as_slice() {
            [blueice_css::Value::Number(w), blueice_css::Value::Number(h)]
                if *w > 0.0 && *h > 0.0 =>
            {
                Some((*w, *h))
            }
            _ => None,
        },
        _ => None,
    };
    let natural = intrinsic.unwrap_or(default);
    // An explicit CSS `auto` overrides the attribute (a presentational hint).
    let css_width = style.px("width", cb_width);
    let css_height = style.px("height", f64::NAN);
    let width = css_width.or(attr_w.filter(|_| !style.is_auto("width")));
    let height = css_height.or(attr_h.filter(|_| !style.is_auto("height")));
    // A single known dimension keeps the aspect ratio of the attributes, or of
    // the image itself.
    let ratio = match (attr_w, attr_h) {
        (Some(w), Some(h)) if h > 0.0 && w > 0.0 => Some(w / h),
        _ => intrinsic.map(|(w, h)| w / h),
    };
    let (width, height) = match (width, height) {
        (Some(w), Some(h)) => (w, Some(h)),
        (Some(w), None) => (w, ratio.map(|r| w / r)),
        (None, Some(h)) => (ratio.map_or(natural.0, |r| h * r), Some(h)),
        (None, None) => (natural.0, Some(natural.1)),
    };
    let clamped = style
        .px("max-width", cb_width)
        .map_or(width, |m| width.min(m))
        .max(style.px("min-width", cb_width).unwrap_or(0.0));
    let height = match height {
        // A width squeezed by `max-width` takes the height with it unless the
        // height was set outright.
        Some(h) if clamped != width && css_height.is_none() && ratio.is_some() => {
            let _ = h;
            clamped / ratio.unwrap_or(1.0)
        }
        Some(h) => h,
        None => natural.1,
    };
    Some((clamped, height))
}

fn vertical_align(style: &Style<'_>, line_height: f64) -> Align {
    let font_size = style.font_size();
    match style.raw("vertical-align") {
        Some(blueice_css::Value::Keyword(k)) => match k.as_str() {
            "top" | "text-top" => Align::Top,
            "bottom" | "text-bottom" => Align::Bottom,
            "sub" => Align::Baseline(-font_size * 0.2),
            "super" => Align::Baseline(font_size * 0.33),
            "middle" => Align::Baseline(font_size * 0.25),
            _ => Align::Baseline(0.0),
        },
        Some(blueice_css::Value::Percentage(p)) => Align::Baseline(line_height * p / 100.0),
        Some(other) => Align::Baseline(
            other
                .to_px(&style.length_context(line_height))
                .unwrap_or(0.0),
        ),
        None => Align::Baseline(0.0),
    }
}

/// An atomic inline built from `node`, or `None` when it is not one.
fn atomic_for(ctx: &Ctx<'_>, node: NodeId, cb_width: f64) -> Atomic {
    let style = ctx.style(node);
    let padding = style.padding(cb_width);
    let border = style.border(cb_width);
    let pb_h = padding.horizontal() + border.horizontal();
    let margin = |name: &str| {
        if style.is_auto(name) {
            0.0
        } else {
            style.side(name, cb_width)
        }
    };
    let (margin_left, margin_right) = (margin("margin-left"), margin("margin-right"));
    let (margin_top, margin_bottom) = (margin("margin-top"), margin("margin-bottom"));
    let constraint = Constraint {
        cb_width,
        cb_height: None,
        root: false,
    };
    let mut pending = Vec::new();
    let mut fragment;
    let mut line_baseline = None;
    if let Some((width, height)) = replaced_size(ctx, node, &style, cb_width) {
        fragment = Fragment {
            node: Some(node),
            width: width + pb_h,
            height: height + padding.vertical() + border.vertical(),
            ..Fragment::empty_block()
        };
        fragment.positioned = style.position() != crate::ctx::Position::Static;
        fragment.hidden = !style.is_visible();
    } else if ctx.writing.get().is_vertical() && style.writing_mode() != ctx.writing.get() {
        // A horizontal box on a vertical line: its width is the width it has across the page, so
        // it is worked out in the page's own frame and handed over as the height of the turned
        // frame, which is how `layout_orthogonal` takes it.
        let turned = ctx.writing.replace(crate::ctx::Writing::Horizontal);
        let (page_padding, page_border) = (style.padding(cb_width), style.border(cb_width));
        let page_pb_h = page_padding.horizontal() + page_border.horizontal();
        let room = ctx.orth_room.get();
        let specified = style
            .px("width", room)
            .map(|w| if style.is_border_box() { w } else { w + page_pb_h });
        let width = specified.unwrap_or_else(|| {
            let (min_content, max_content) = crate::intrinsic::preferred_widths(ctx, node);
            max_content.min(room.max(min_content))
        });
        ctx.writing.set(turned);
        let placed = layout_block_level(
            ctx,
            node,
            &constraint,
            Forced {
                width: None,
                height: Some(width),
            },
        );
        pending = placed.pending_abs;
        line_baseline = last_baseline(&placed.fragment);
        fragment = placed.fragment;
    } else {
        let specified =
            style
                .px("width", cb_width)
                .map(|w| if style.is_border_box() { w } else { w + pb_h });
        let width = specified.unwrap_or_else(|| {
            let (min_content, max_content) = crate::intrinsic::preferred_widths(ctx, node);
            let available = (cb_width - margin_left - margin_right).max(0.0);
            max_content.min(available.max(min_content))
        });
        let placed = layout_block_level(
            ctx,
            node,
            &constraint,
            Forced {
                width: Some(width),
                height: None,
            },
        );
        pending = placed.pending_abs;
        line_baseline = last_baseline(&placed.fragment);
        fragment = placed.fragment;
    }
    let baseline = match line_baseline {
        Some(b) if !style.clips_overflow() => b,
        _ => fragment.height + margin_bottom,
    };
    let own_line_height = fragment.height + margin_top + margin_bottom;
    Atomic {
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        baseline,
        align: vertical_align(&style, own_line_height),
        fragment,
        pending,
    }
}

/// The baseline of the first line box inside `fragment`, from its top edge.
pub(crate) fn first_baseline(fragment: &Fragment) -> Option<f64> {
    for child in &fragment.children {
        if matches!(child.kind, FragmentKind::Line) {
            if let Some(b) = child.baseline {
                return Some(child.y + b);
            }
        }
        if matches!(child.kind, FragmentKind::Block) && !child.positioned {
            if let Some(b) = first_baseline(child) {
                return Some(child.y + b);
            }
        }
    }
    None
}

/// The baseline of the last line box inside `fragment`, from its top edge.
fn last_baseline(fragment: &Fragment) -> Option<f64> {
    for child in fragment.children.iter().rev() {
        if matches!(child.kind, FragmentKind::Line) {
            if let Some(b) = child.baseline {
                return Some(child.y + b);
            }
        }
        if matches!(child.kind, FragmentKind::Block) && !child.positioned {
            if let Some(b) = last_baseline(child) {
                return Some(child.y + b);
            }
        }
    }
    None
}

/// Whether an inline-level node should be treated as one atomic box.
fn is_atomic(ctx: &Ctx<'_>, node: NodeId, style: &Style<'_>) -> bool {
    style.display().is_atomic_inline()
        || matches!(
            ctx.tag(node),
            Some("img" | "svg" | "canvas" | "video" | "iframe" | "embed" | "object")
        )
        || !matches!(
            style.display(),
            Display::Inline | Display::None | Display::Contents
        )
}

fn collect(
    ctx: &Ctx<'_>,
    node: NodeId,
    cb_width: f64,
    pieces: &mut Vec<Piece>,
    abs: &mut Vec<PendingAbs>,
    state: &mut State,
) {
    match ctx.node(node) {
        NodeData::Text { data } => {
            if let Some(parent) = ctx.parent(node) {
                push_text(ctx, data, parent, pieces, state);
            }
        }
        NodeData::Document => {}
        NodeData::Element { tag_name, .. } => {
            let style = ctx.style(node);
            let display = style.display();
            if display == Display::None || display == Display::TableColumn {
                return;
            }
            if style.is_out_of_flow() {
                abs.push(PendingAbs {
                    node,
                    x: 0.0,
                    y: 0.0,
                });
                return;
            }
            if tag_name == "br" {
                pieces.push(Piece::Break);
                state.last_space = false;
                return;
            }
            if display == Display::Contents {
                for child in ctx.children(node) {
                    collect(ctx, child, cb_width, pieces, abs, state);
                }
                return;
            }
            if is_atomic(ctx, node, &style) {
                pieces.push(Piece::Atomic(Box::new(atomic_for(ctx, node, cb_width))));
                state.last_space = false;
                return;
            }
            // A plain inline box: its horizontal margin, border and padding
            // take room before and after its content.
            let padding = style.padding(cb_width);
            let border = style.border(cb_width);
            let side = |name: &str| {
                if style.is_auto(name) {
                    0.0
                } else {
                    style.side(name, cb_width)
                }
            };
            // The box's start is the side its own direction begins at.
            let (start_side, end_side) = if style.is_rtl() {
                ("right", "left")
            } else {
                ("left", "right")
            };
            let edge = |side_name: &str, border_width: f64, pad: f64| {
                (
                    side(&format!("margin-{side_name}")) + border_width + pad,
                    side(&format!("margin-{side_name}")),
                )
            };
            let sizes = |name: &str| match name {
                "left" => (border.left, padding.left),
                _ => (border.right, padding.right),
            };
            let (before, before_outside) = {
                let (b, p) = sizes(start_side);
                edge(start_side, b, p)
            };
            let (after, after_outside) = {
                let (b, p) = sizes(end_side);
                edge(end_side, b, p)
            };
            let paints = paints_own_box(&style);
            let (opening, closing) = bidi_marks(ctx, node, &style);
            pieces.extend(opening.into_iter().map(Piece::Mark));
            if paints {
                pieces.push(Piece::Edge {
                    node,
                    open: true,
                    width: before,
                    outside: before_outside,
                });
            } else if before != 0.0 {
                pieces.push(Piece::Gap(before));
            }
            if style.combine_upright() == Some(crate::ctx::Combine::All) {
                // `text-combine-upright: all` sets the element's whole text in one cell, however
                // many words and inline boxes it holds.
                let text = element_text(ctx, node);
                if !text.is_empty() {
                    let width = TextProps::of(&style).width(&text);
                    pieces.push(Piece::Word {
                        text,
                        width,
                        style_node: node,
                        can_break_before: false,
                        wrap: false,
                        hang: Hang::default(),
                        scale: 1.0,
                        trim: false,
                    });
                    state.last_space = false;
                }
            } else {
                for child in ctx.children(node) {
                    collect(ctx, child, cb_width, pieces, abs, state);
                }
            }
            if paints {
                pieces.push(Piece::Edge {
                    node,
                    open: false,
                    width: after,
                    outside: after_outside,
                });
            } else if after != 0.0 {
                pieces.push(Piece::Gap(after));
            }
            pieces.extend(closing.into_iter().map(Piece::Mark));
        }
    }
}

/// The text inside `node`, whitespace collapsed to single spaces and trimmed.
fn element_text(ctx: &Ctx<'_>, node: NodeId) -> String {
    fn gather(ctx: &Ctx<'_>, node: NodeId, out: &mut String) {
        for child in ctx.children(node) {
            match ctx.node(child) {
                NodeData::Text { data } => out.push_str(data),
                NodeData::Element { .. } => gather(ctx, child, out),
                NodeData::Document => {}
            }
        }
    }
    let mut raw = String::new();
    gather(ctx, node, &mut raw);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `node` asks for its direction to be found from its content (`dir=auto`, or `<bdi>`
/// without a `dir`).
fn direction_is_auto(ctx: &Ctx<'_>, node: NodeId) -> bool {
    let NodeData::Element {
        tag_name,
        attributes,
        ..
    } = ctx.node(node)
    else {
        return false;
    };
    match attributes.iter().find(|(k, _)| k == "dir") {
        Some((_, v)) => v.trim().eq_ignore_ascii_case("auto"),
        None => tag_name == "bdi",
    }
}

/// The bidi formatting characters an inline box's `unicode-bidi` and `direction` stand for:
/// what opens before its content and what closes after it, innermost last.
fn bidi_marks(ctx: &Ctx<'_>, node: NodeId, style: &Style<'_>) -> (Vec<char>, Vec<char>) {
    use crate::bidi::{FSI, LRE, LRI, LRO, PDF, PDI, RLE, RLI, RLO};
    let rtl = style.is_rtl();
    let auto = direction_is_auto(ctx, node);
    let isolate = if auto {
        FSI
    } else if rtl {
        RLI
    } else {
        LRI
    };
    match style.keyword("unicode-bidi") {
        Some("embed") => (vec![if rtl { RLE } else { LRE }], vec![PDF]),
        Some("isolate") => (vec![isolate], vec![PDI]),
        Some("bidi-override") => (vec![if rtl { RLO } else { LRO }], vec![PDF]),
        Some("isolate-override") => (
            vec![isolate, if rtl { RLO } else { LRO }],
            vec![PDF, PDI],
        ),
        Some("plaintext") => (vec![FSI], vec![PDI]),
        _ => (Vec::new(), Vec::new()),
    }
}

/// Whether an inline box has anything of its own to paint.
fn paints_own_box(style: &Style<'_>) -> bool {
    let has = |name: &str| {
        style.raw(name).is_some_and(
            |v| !matches!(v, blueice_css::Value::Keyword(k) if k == "none" || k == "transparent"),
        )
    };
    has("background-color")
        || has("background-image")
        || has("box-shadow")
        || ["top", "right", "bottom", "left"].iter().any(|side| {
            has(&format!("border-{side}-style"))
                && style.border(0.0).horizontal() + style.border(0.0).vertical() > 0.0
        })
}

/// `line-height` in pixels for text in `style`: a number scales the font
/// size, a length or percentage is used as is, and `normal` is 1.2 em.
fn line_height_of(style: &Style<'_>) -> f64 {
    let font_size = style.font_size();
    match style.style.and_then(|s| s.line_height.as_ref()) {
        Some(blueice_css::Value::Number(n)) => n * font_size,
        Some(blueice_css::Value::Keyword(_)) | None => crate::text::default_line_height(font_size),
        Some(other) => other
            .to_px(&style.length_context(font_size))
            .unwrap_or_else(|| crate::text::default_line_height(font_size)),
    }
}

/// How far text of `style` reaches above and below its baseline, with the
/// line-height's half-leading split evenly around the font's own extent.
fn text_extent(style: &Style<'_>) -> (f64, f64) {
    let line_height = line_height_of(style);
    let (ascent, descent) = blueice_font::ascent_descent_in(
        style.font_size(),
        style.style.is_some_and(|s| s.is_bold()),
        style.style.is_some_and(|s| s.is_italic()),
        style.web_face(),
    );
    let half_leading = (line_height - (ascent + descent)) / 2.0;
    (ascent + half_leading, descent + half_leading)
}

struct Chunk {
    /// Collapsible space before the chunk (dropped at a line start).
    lead: f64,
    /// The last space piece that makes up `lead`.
    lead_piece: usize,
    items: Vec<usize>,
    width: f64,
    forced_break_after: bool,
}

fn atomic_width(a: &Atomic) -> f64 {
    a.margin_left + a.fragment.width + a.margin_right
}

fn into_chunks(pieces: &[Piece]) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current: Option<Chunk> = None;
    let mut lead = 0.0;
    let mut lead_piece = 0;
    let mut need_new = false;
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Space { width, wrap, .. } => {
                if *wrap {
                    lead += width;
                    lead_piece = i;
                    need_new = true;
                } else if let Some(chunk) = current.as_mut() {
                    chunk.items.push(i);
                    chunk.width += width;
                } else {
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        lead_piece,
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                }
            }
            Piece::Word {
                width,
                can_break_before,
                wrap,
                ..
            } => {
                let start_new = current.is_none() || need_new || (*can_break_before && *wrap);
                if start_new {
                    chunks.extend(current.take());
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        lead_piece,
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                } else if let Some(chunk) = current.as_mut() {
                    chunk.items.push(i);
                    chunk.width += width;
                }
                need_new = false;
            }
            Piece::Gap(width) | Piece::Edge { width, .. } => {
                if let Some(chunk) = current.as_mut().filter(|_| !need_new) {
                    chunk.items.push(i);
                    chunk.width += width;
                } else {
                    chunks.extend(current.take());
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        lead_piece,
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                    need_new = false;
                }
            }
            Piece::Atomic(atomic) => {
                chunks.extend(current.take());
                current = Some(Chunk {
                    lead: std::mem::take(&mut lead),
                    lead_piece,
                    items: vec![i],
                    width: atomic_width(atomic),
                    forced_break_after: false,
                });
                // A break is allowed after an atomic box.
                need_new = true;
            }
            Piece::Mark(_) => {}
            Piece::Break => {
                let mut chunk = current.take().unwrap_or(Chunk {
                    lead: 0.0,
                    lead_piece,
                    items: Vec::new(),
                    width: 0.0,
                    forced_break_after: false,
                });
                chunk.forced_break_after = true;
                chunks.push(chunk);
                lead = 0.0;
                need_new = false;
            }
        }
    }
    chunks.extend(current);
    chunks
}

enum LineItem {
    Piece(usize),
    /// A collapsed space between two chunks, with the space piece it stands for.
    Space(f64, usize),
}

struct Line {
    items: Vec<LineItem>,
    width: f64,
    forced_end: bool,
    /// Inset from the left edge and width left by floats.
    left: f64,
    avail: f64,
    /// Where a line pushed below a float starts, from the run's top.
    start_y: Option<f64>,
}

impl Line {
    fn new(room: Room, start_y: Option<f64>) -> Line {
        Line {
            items: Vec::new(),
            width: 0.0,
            forced_end: false,
            left: room.left,
            avail: room.avail,
            start_y,
        }
    }
}

/// The room a line has at some height: how far floats push its left edge in,
/// how wide it then is, and where the next float ends below that height.
#[derive(Clone, Copy)]
struct Room {
    left: f64,
    avail: f64,
    next: Option<f64>,
}

/// Floats a run of inline content flows around; `top` is where the run starts
/// in the same coordinates as the floats.
pub(crate) struct FloatView<'a> {
    pub floats: &'a [FloatRect],
    pub top: f64,
}

fn break_lines(
    pieces: &[Piece],
    first_indent: f64,
    room: &dyn Fn(f64) -> Room,
    est_height: f64,
    first_in_block: bool,
) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut y_est = 0.0f64;
    let mut line = Line::new(room(0.0), None);
    let chunks = into_chunks(pieces);
    let chunk_count = chunks.len();
    for (chunk_index, chunk) in chunks.into_iter().enumerate() {
        // A line with nothing on it drops below floats that leave it too
        // narrow for the next word.
        macro_rules! skip_floats {
            () => {
                if line.items.is_empty() {
                    let indent = if lines.is_empty() { first_indent } else { 0.0 };
                    loop {
                        let r = room(y_est);
                        match r.next {
                            Some(next) if chunk.width > r.avail - indent + 0.01 && next > y_est => {
                                y_est = next;
                                line = Line::new(room(y_est), Some(y_est));
                            }
                            _ => break,
                        }
                    }
                }
            };
        }
        skip_floats!();
        let first = lines.is_empty();
        let limit = line.avail - if first { first_indent } else { 0.0 };
        let has_content = !line.items.is_empty();
        // What of the chunk may hang outside the line: a stop or comma at its end, a closing
        // bracket at the end of the last chunk, an opening bracket at the start of the first.
        let hung = |item: Option<&usize>, pick: fn(&Hang) -> f64| match item.map(|i| &pieces[*i]) {
            Some(Piece::Word { hang, .. }) => pick(hang),
            _ => 0.0,
        };
        let mut hang = hung(chunk.items.last(), |h| h.end).max(hung(chunk.items.last(), |h| h.end_trim));
        if chunk_index + 1 == chunk_count {
            hang = hang.max(hung(chunk.items.last(), |h| h.last));
        }
        if first_in_block && chunk_index == 0 {
            hang += hung(chunk.items.first(), |h| h.first);
        }
        let fits = !has_content || line.width + chunk.lead + chunk.width - hang <= limit + 0.01;
        if !fits {
            y_est += est_height;
            let next = Line::new(room(y_est), None);
            lines.push(std::mem::replace(&mut line, next));
            skip_floats!();
        }
        if !line.items.is_empty() && chunk.lead > 0.0 {
            line.items.push(LineItem::Space(chunk.lead, chunk.lead_piece));
            line.width += chunk.lead;
        }
        for i in &chunk.items {
            line.items.push(LineItem::Piece(*i));
        }
        line.width += chunk.width;
        if chunk.forced_break_after {
            line.forced_end = true;
            y_est += est_height;
            let next = Line::new(room(y_est), None);
            lines.push(std::mem::replace(&mut line, next));
        }
    }
    if !line.items.is_empty() {
        lines.push(line);
    }
    lines
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
    Center,
}

/// Which way `text-align` puts a line, given the direction its paragraph runs: `start` and
/// `end` follow it, `left` and `right` do not, and `justify` (on the lines it does not stretch)
/// falls back to `start`.
fn align_side(text_align: &str, rtl: bool) -> Side {
    let (start, end) = if rtl {
        (Side::Right, Side::Left)
    } else {
        (Side::Left, Side::Right)
    };
    match text_align {
        "left" | "-webkit-left" | "-moz-left" => Side::Left,
        "right" | "-webkit-right" | "-moz-right" => Side::Right,
        "end" => end,
        "center" | "-webkit-center" | "-moz-center" => Side::Center,
        _ => start,
    }
}

/// The shift `text-align` gives a line of `width` in `available` room.
fn align_offset(side: Side, width: f64, available: f64) -> f64 {
    let free = (available - width).max(0.0);
    match side {
        Side::Right => free,
        Side::Center => free / 2.0,
        Side::Left => 0.0,
    }
}

fn piece_style<'a>(ctx: &'a Ctx<'a>, piece: &Piece) -> Option<Style<'a>> {
    match piece {
        Piece::Word { style_node, .. } | Piece::Space { style_node, .. } => {
            Some(ctx.style(*style_node))
        }
        _ => None,
    }
}

/// `text-spacing-trim`: the fullwidth punctuation of a run whose blank half is trimmed because
/// of its neighbours (CSS Text 4, 8.5.1), cut out of its words into pieces drawn with the font's
/// half-width alternates. An opening mark after another opening mark, a middle dot, an ideographic
/// space, a closing mark of at least its size or any other opening bracket is trimmed; so is a
/// closing mark before the mirror of that list; `trim-all` trims every opening, closing and
/// middle-dot mark; `space-all` trims none. A face without the alternates declines.
fn apply_trimming(ctx: &Ctx<'_>, pieces: Vec<Piece>) -> Vec<Piece> {
    // Fast out: most runs have no fullwidth punctuation at all.
    let has_marks = pieces.iter().any(|p| {
        matches!(p, Piece::Word { text, .. } if text.chars().any(|c| {
            matches!(trim_class(c), Some(TrimClass::Opening | TrimClass::Closing | TrimClass::MiddleDot))
        }))
    });
    if !has_marks {
        return pieces;
    }
    use crate::ctx::SpacingTrim as T;
    // The characters of the run in order: a word's, or a break in the sequence where something
    // else (a space, an atomic box) stands between two words.
    struct Slot {
        piece: usize,
        index: usize,
        c: char,
        size: f64,
    }
    let mut slots: Vec<Option<Slot>> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Word { text, style_node, scale, .. } => {
                let size = ctx.style(*style_node).font_size() * scale;
                for (index, c) in text.chars().enumerate() {
                    slots.push(Some(Slot { piece: i, index, c, size }));
                }
            }
            Piece::Space { .. } | Piece::Break | Piece::Atomic(_) => slots.push(None),
            Piece::Gap(_) | Piece::Edge { .. } | Piece::Mark(_) => {}
        }
    }
    let class_at = |k: usize| slots.get(k).and_then(|s| s.as_ref()).and_then(|s| trim_class(s.c));
    let mut trimmed: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut verdicts: std::collections::HashMap<(NodeId, char), bool> = std::collections::HashMap::new();
    for (k, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        let Some(class) = trim_class(slot.c) else { continue };
        if !matches!(class, TrimClass::Opening | TrimClass::Closing | TrimClass::MiddleDot) {
            continue;
        }
        let Piece::Word { style_node, scale, .. } = &pieces[slot.piece] else { continue };
        let mode = ctx.style(*style_node).text_spacing_trim();
        if mode == T::SpaceAll {
            continue;
        }
        let neighbour = |k: Option<usize>| k.and_then(|k| slots.get(k)).and_then(|s| s.as_ref());
        let wanted = mode == T::TrimAll
            || match class {
                TrimClass::Opening => {
                    let before = neighbour(k.checked_sub(1));
                    before.is_some_and(|p| match trim_class(p.c) {
                        Some(TrimClass::Opening | TrimClass::MiddleDot | TrimClass::IdeographicSpace | TrimClass::OtherOpen) => true,
                        Some(TrimClass::Closing) => p.size >= slot.size,
                        _ => false,
                    })
                }
                TrimClass::Closing => {
                    let after = neighbour(Some(k + 1));
                    after.is_some_and(|n| match trim_class(n.c) {
                        Some(TrimClass::Closing | TrimClass::MiddleDot | TrimClass::IdeographicSpace | TrimClass::OtherClose) => true,
                        Some(TrimClass::Opening) => n.size > slot.size,
                        _ => false,
                    })
                }
                _ => false,
            };
        let _ = class_at;
        if !wanted {
            continue;
        }
        // The face has to have the alternate: the mark must really get narrower.
        let ok = *verdicts.entry((*style_node, slot.c)).or_insert_with(|| {
            let props = TextProps::of(&ctx.style(*style_node)).scaled(*scale);
            let text = slot.c.to_string();
            props.trimmed().width(&text) < props.width(&text) - 0.01
        });
        if ok {
            trimmed.insert((slot.piece, slot.index));
        }
    }
    if trimmed.is_empty() {
        return pieces;
    }
    // Words are cut at the trimmed characters into runs of one kind.
    let mut out = Vec::with_capacity(pieces.len() + trimmed.len() * 2);
    for (i, piece) in pieces.into_iter().enumerate() {
        let Piece::Word { text, style_node, can_break_before, wrap, hang, scale, trim, width } = piece else {
            out.push(piece);
            continue;
        };
        if !text.chars().enumerate().any(|(index, _)| trimmed.contains(&(i, index))) {
            out.push(Piece::Word { text, width, style_node, can_break_before, wrap, hang, scale, trim });
            continue;
        }
        let base = TextProps::of(&ctx.style(style_node)).scaled(scale);
        let mut parts: Vec<(String, bool)> = Vec::new();
        for (index, c) in text.chars().enumerate() {
            let cut = trimmed.contains(&(i, index));
            match parts.last_mut() {
                Some((run, flag)) if *flag == cut => run.push(c),
                _ => parts.push((c.to_string(), cut)),
            }
        }
        let count = parts.len();
        for (k, (part, cut)) in parts.into_iter().enumerate() {
            let props = if cut { base.trimmed() } else { base.clone() };
            // What may hang belongs to the ends of the word; a trimmed mark has no blank half left.
            let mut h = Hang::default();
            if k == 0 && !cut {
                h.first = hang.first;
                h.start_trim = hang.start_trim;
            }
            if k + 1 == count && !cut {
                h.end = hang.end;
                h.force = hang.force;
                h.last = hang.last;
                h.end_trim = hang.end_trim;
                h.end_trim_force = hang.end_trim_force;
            }
            out.push(Piece::Word {
                width: props.width(&part),
                text: part,
                style_node,
                can_break_before: can_break_before && k == 0,
                wrap,
                hang: h,
                scale,
                trim: cut,
            });
        }
    }
    out
}

/// The bidi levels of a run's pieces. Each piece stands for some characters of one paragraph
/// string; a word whose characters fall on different levels is cut into a word per level.
/// Returns the pieces (words possibly cut), a level for each, and the paragraph's own level.
fn resolve_bidi(ctx: &Ctx<'_>, pieces: Vec<Piece>, base: Base) -> (Vec<Piece>, Vec<u8>, u8) {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        starts.push(text.len());
        match piece {
            Piece::Word { text: t, .. } => text.push_str(t),
            Piece::Space { .. } => text.push(' '),
            Piece::Gap(_) | Piece::Edge { .. } | Piece::Atomic(_) => text.push('\u{fffc}'),
            Piece::Break => text.push('\u{2028}'),
            Piece::Mark(c) => text.push(*c),
        }
    }
    if base != Base::Rtl && !bidi::needs_bidi(&text) {
        let levels = vec![0; pieces.len()];
        return (pieces, levels, 0);
    }
    let (byte_levels, paragraph) = bidi::levels(&text, base);
    let mut out = Vec::with_capacity(pieces.len());
    let mut levels = Vec::with_capacity(pieces.len());
    for (piece, start) in pieces.into_iter().zip(starts) {
        let Piece::Word {
            text: word,
            style_node,
            can_break_before,
            wrap,
            width,
            hang,
            scale,
            trim,
        } = piece
        else {
            levels.push(byte_levels.get(start).copied().unwrap_or(paragraph));
            out.push(piece);
            continue;
        };
        let level_at = |offset: usize| byte_levels.get(start + offset).copied().unwrap_or(paragraph);
        // Where the level changes inside the word.
        let mut cuts = vec![0usize];
        let mut last = level_at(0);
        for (offset, _) in word.char_indices().skip(1) {
            let level = level_at(offset);
            if level != last {
                cuts.push(offset);
                last = level;
            }
        }
        if cuts.len() == 1 {
            levels.push(level_at(0));
            out.push(Piece::Word {
                text: word,
                width,
                style_node,
                can_break_before,
                wrap,
                hang,
                scale,
                trim,
            });
            continue;
        }
        cuts.push(word.len());
        let props = if trim {
            TextProps::of(&ctx.style(style_node)).scaled(scale).trimmed()
        } else {
            TextProps::of(&ctx.style(style_node)).scaled(scale)
        };
        for (k, pair) in cuts.windows(2).enumerate() {
            let part = &word[pair[0]..pair[1]];
            levels.push(level_at(pair[0]));
            out.push(Piece::Word {
                text: part.to_string(),
                width: props.width(part),
                style_node,
                can_break_before: can_break_before && k == 0,
                wrap,
                hang: {
                    let mut h = props.hang(part);
                    // Only the first part of a word starts it, only the last ends it.
                    if k != 0 {
                        h.first = 0.0;
                    }
                    if k + 2 != cuts.len() {
                        h.end = 0.0;
                        h.last = 0.0;
                    }
                    h
                },
                scale,
                trim,
            });
        }
    }
    (out, levels, paragraph)
}

/// Lays out one run of inline-level `nodes` inside `container`.
pub(crate) fn layout_run(
    ctx: &Ctx<'_>,
    container: NodeId,
    nodes: &[NodeId],
    content_width: f64,
    first_in_block: bool,
) -> InlineResult {
    layout_run_around(ctx, container, nodes, content_width, first_in_block, None)
}

/// [`layout_run`] with lines narrowed (and, when a word does not fit beside
/// them, lowered) around `floats`.
pub(crate) fn layout_run_around(
    ctx: &Ctx<'_>,
    container: NodeId,
    nodes: &[NodeId],
    content_width: f64,
    first_in_block: bool,
    floats: Option<&FloatView<'_>>,
) -> InlineResult {
    let mut pieces = Vec::new();
    let mut abs = Vec::new();
    let mut state = State { last_space: true };
    for node in nodes {
        collect(ctx, *node, content_width, &mut pieces, &mut abs, &mut state);
    }
    let mut result = InlineResult {
        lines: Vec::new(),
        height: 0.0,
        pending_abs: abs,
        extent: 0.0,
    };
    if pieces.is_empty() {
        return result;
    }

    let cstyle = ctx.style(container);
    let base = if direction_is_auto(ctx, container)
        || cstyle.keyword("unicode-bidi") == Some("plaintext")
    {
        Base::Auto
    } else if cstyle.is_rtl() {
        Base::Rtl
    } else {
        Base::Ltr
    };
    // `unicode-bidi: bidi-override` (or `embed`) on the block itself reads all of its text one way.
    if matches!(
        cstyle.keyword("unicode-bidi"),
        Some("bidi-override" | "isolate-override" | "embed")
    ) {
        let (opening, closing) = bidi_marks(ctx, container, &cstyle);
        for mark in opening.into_iter().rev() {
            pieces.insert(0, Piece::Mark(mark));
        }
        pieces.extend(closing.into_iter().map(Piece::Mark));
    }
    let pieces = apply_trimming(ctx, pieces);
    let (pieces, levels, paragraph_level) = resolve_bidi(ctx, pieces, base);
    let rtl_paragraph = paragraph_level % 2 == 1;
    let text_align = cstyle
        .style
        .and_then(|s| s.text_align.as_deref())
        .unwrap_or("start")
        .to_string();
    // `text-align-last` places the last line of the run and every line a `<br>` ends.
    let align_last = cstyle
        .keyword("text-align-last")
        .filter(|v| *v != "auto")
        .map(str::to_string);
    let indent = if first_in_block {
        cstyle.px("text-indent", content_width).unwrap_or(0.0)
    } else {
        0.0
    };
    let (strut_above, strut_below) = text_extent(&cstyle);
    let room = |y: f64| match floats {
        Some(view) => {
            let top = view.top + y;
            let (left, right) = float::free_span(
                view.floats,
                top,
                top + strut_above + strut_below,
                content_width,
            );
            Room {
                left,
                avail: right - left,
                next: float::next_edge(view.floats, top).map(|e| e - view.top),
            }
        }
        None => Room {
            left: 0.0,
            avail: content_width,
            next: None,
        },
    };
    let lines = break_lines(&pieces, indent, &room, strut_above + strut_below, first_in_block);
    let line_count = lines.len();
    let mut y = 0.0f64;
    // Inline boxes still open across lines, with where each starts on the line.
    let mut open: Vec<(NodeId, f64, usize)> = Vec::new();
    let mut opened = 0usize;

    for (line_index, line) in lines.iter().enumerate() {
        let is_first = line_index == 0;
        let line_indent = if is_first { indent } else { 0.0 };
        let available = line.avail - line_indent;
        if let Some(start) = line.start_y {
            y = y.max(start);
        }

        // ---- vertical metrics ----
        let mut max_above = strut_above;
        let mut max_below = strut_below;
        let mut tall = 0.0f64;
        let mut metrics: Vec<Option<(f64, f64, Align)>> = Vec::new();
        for item in &line.items {
            let LineItem::Piece(i) = item else {
                metrics.push(None);
                continue;
            };
            let entry = match &pieces[*i] {
                Piece::Atomic(a) => {
                    let above = a.margin_top + a.baseline;
                    let below = (a.margin_top + a.fragment.height + a.margin_bottom) - above;
                    Some((above, below, a.align))
                }
                piece => piece_style(ctx, piece).map(|style| {
                    let (above, below) = text_extent(&style);
                    let align = match piece {
                        Piece::Word { style_node, .. } | Piece::Space { style_node, .. }
                            if *style_node != container =>
                        {
                            vertical_align(&style, above + below)
                        }
                        _ => Align::Baseline(0.0),
                    };
                    (above, below, align)
                }),
            };
            if let Some((above, below, align)) = entry {
                match align {
                    Align::Baseline(shift) => {
                        max_above = max_above.max(above + shift);
                        max_below = max_below.max(below - shift);
                    }
                    Align::Top | Align::Bottom => tall = tall.max(above + below),
                }
            }
            metrics.push(entry);
        }
        let line_height = (max_above + max_below).max(tall);
        let baseline = max_above;

        // ---- horizontal placement and fragments ----
        let last_line = line.forced_end || line_index + 1 == line_count;
        let line_align = match (&align_last, last_line) {
            (Some(last), true) => last.as_str(),
            _ => text_align.as_str(),
        };
        let side = align_side(line_align, rtl_paragraph);
        // The line's items in the order they sit on screen. Spaces at its end take the
        // paragraph's level, so they do not wander into the middle of the line.
        let mut item_levels: Vec<u8> = line
            .items
            .iter()
            .map(|item| match item {
                LineItem::Piece(i) | LineItem::Space(_, i) => levels[*i],
            })
            .collect();
        for (k, item) in line.items.iter().enumerate().rev() {
            match item {
                LineItem::Space(..) => item_levels[k] = paragraph_level,
                LineItem::Piece(i) if matches!(pieces[*i], Piece::Space { .. }) => {
                    item_levels[k] = paragraph_level
                }
                _ => break,
            }
        }
        let visual = bidi::visual_order(&item_levels);
        // What hangs outside the line: a stop or comma (or, on the last line, a closing bracket)
        // at its end, an opening bracket at the start of the first line. The line is aligned and
        // stretched as if those were not on it.
        let word_hang = |item: Option<&LineItem>| match item {
            Some(LineItem::Piece(i)) => match &pieces[*i] {
                Piece::Word { hang, .. } => *hang,
                _ => Hang::default(),
            },
            _ => Hang::default(),
        };
        let overflows = line.width > available + 0.01;
        let end_hang = word_hang(line.items.last());
        let hung_at_end = if end_hang.end > 0.0 && (end_hang.force || overflows) {
            end_hang.end
        } else if end_hang.last > 0.0 && line_index + 1 == line_count && overflows {
            end_hang.last
        } else {
            0.0
        };
        // The blank half of a closing mark at the end of a line (`text-spacing-trim`).
        let trimmed_at_end = if end_hang.end_trim > 0.0 && (end_hang.end_trim_force || overflows) {
            end_hang.end_trim
        } else {
            0.0
        };
        let trailing_hang = hung_at_end.max(trimmed_at_end);
        let start_hang = (if is_first && first_in_block {
            word_hang(line.items.first()).first
        } else {
            0.0
        })
        .max(word_hang(line.items.first()).start_trim);
        // A hung character is outside the edge of the line it is at on screen: the logical end
        // is the left edge in right-to-left text, the logical start the right edge there. One
        // in the middle of a line (mixed directions) does not hang.
        let (mut hang_left, mut hang_right) = (0.0f64, 0.0f64);
        let last_slot = line.items.len().saturating_sub(1);
        for (amount, slot, is_end) in [(trailing_hang, last_slot, true), (start_hang, 0, false)] {
            if amount <= 0.0 {
                continue;
            }
            // The start of a left-to-right word is at its left, of a right-to-left one at its
            // right; its end is the other side.
            let odd = item_levels.get(slot).is_some_and(|l| l % 2 == 1);
            let at_right = if is_end { !odd } else { odd };
            if at_right && visual.last() == Some(&slot) {
                hang_right += amount;
            } else if !at_right && visual.first() == Some(&slot) {
                hang_left += amount;
            }
        }
        let visible = line.width - hang_left - hang_right;
        let extra_per_space =
            if line_align == "justify" && (!last_line || align_last.is_some()) {
                let spaces = line
                    .items
                    .iter()
                    .filter(|i| matches!(i, LineItem::Space(..)))
                    .count();
                if spaces > 0 {
                    ((available - visible).max(0.0)) / spaces as f64
                } else {
                    0.0
                }
            } else {
                0.0
            };
        let used_width = line.width
            + extra_per_space
                * line
                    .items
                    .iter()
                    .filter(|i| matches!(i, LineItem::Space(..)))
                    .count() as f64;
        // The indent is taken from the start side: the left edge, or the right in a paragraph
        // that runs right to left.
        let offset = line.left
            + if rtl_paragraph { 0.0 } else { line_indent }
            + align_offset(side, used_width - hang_left - hang_right, available);
        let mut x = if hang_left > 0.0 { -hang_left } else { 0.0 };
        let mut children: Vec<Fragment> = Vec::new();
        let mut decorations: Vec<(usize, NodeId, f64, f64)> = Vec::new();
        for entry in open.iter_mut() {
            entry.1 = 0.0;
        }
        // text, x, width, node, y, height, drawn right to left, first piece in reading order, size share
        type Run = (String, f64, f64, NodeId, f64, f64, bool, u32, f64, bool);
        let mut run_text: Option<Run> = None;
        let flush = |run: &mut Option<Run>, children: &mut Vec<Fragment>| {
            if let Some((text, rx, rw, node, ry, rh, rtl, logical, size_scale, trim)) = run.take() {
                let hidden = !ctx.style(node).is_visible();
                children.push(Fragment {
                    node: Some(node),
                    kind: FragmentKind::Text(text),
                    x: rx,
                    y: ry,
                    width: rw,
                    height: rh,
                    hidden,
                    rtl,
                    logical,
                    size_scale,
                    trim,
                    ..Fragment::empty_block()
                });
            }
        };
        for slot in visual {
            let item = &line.items[slot];
            let odd = item_levels[slot] % 2 == 1;
            match item {
                LineItem::Space(width, _) => {
                    flush(&mut run_text, &mut children);
                    x += width + extra_per_space;
                }
                LineItem::Piece(i) => match &pieces[*i] {
                    Piece::Word {
                        text,
                        width,
                        style_node,
                        scale,
                        trim,
                        ..
                    } => {
                        let (above, below, align) = metrics[slot].unwrap_or((
                            strut_above,
                            strut_below,
                            Align::Baseline(0.0),
                        ));
                        let top = match align {
                            Align::Baseline(shift) => baseline - shift - above,
                            Align::Top => 0.0,
                            Align::Bottom => line_height - (above + below),
                        };
                        match run_text.as_mut() {
                            Some((run, _, rw, node, _, _, run_rtl, first, run_scale, run_trim))
                                if *node == *style_node
                                    && *run_rtl == odd
                                    && *run_scale == *scale
                                    && *run_trim == *trim =>
                            {
                                *first = (*first).min(*i as u32);
                                // On screen a right-to-left run reads backwards, so the word
                                // that comes next on the left is the one before in the text.
                                if odd {
                                    run.insert_str(0, text);
                                } else {
                                    run.push_str(text);
                                }
                                *rw += width;
                            }
                            _ => {
                                flush(&mut run_text, &mut children);
                                run_text = Some((
                                    text.clone(),
                                    x,
                                    *width,
                                    *style_node,
                                    top,
                                    above + below,
                                    odd,
                                    *i as u32,
                                    *scale,
                                    *trim,
                                ));
                            }
                        }
                        x += width;
                    }
                    Piece::Space { width, .. } => {
                        flush(&mut run_text, &mut children);
                        x += width;
                    }
                    Piece::Gap(width) => {
                        flush(&mut run_text, &mut children);
                        x += width;
                    }
                    Piece::Edge {
                        node,
                        open: opens,
                        width,
                        outside,
                    } => {
                        flush(&mut run_text, &mut children);
                        // A box's edges swap sides when they sit in right-to-left order.
                        let opens = &(*opens != odd);
                        if *opens {
                            open.push((*node, x + outside, opened));
                            opened += 1;
                        } else if let Some(at) = open.iter().rposition(|(n, _, _)| n == node) {
                            let (n, start, order) = open.remove(at);
                            decorations.push((order, n, start, x + width - outside));
                        }
                        x += width;
                    }
                    Piece::Atomic(a) => {
                        flush(&mut run_text, &mut children);
                        let mut fragment = a.fragment.clone();
                        fragment.x = x + a.margin_left;
                        fragment.y = match a.align {
                            Align::Baseline(shift) => baseline - shift - a.baseline,
                            Align::Top => a.margin_top,
                            Align::Bottom => line_height - a.margin_bottom - a.fragment.height,
                        };
                        for p in &a.pending {
                            result.pending_abs.push(PendingAbs {
                                node: p.node,
                                x: p.x + fragment.x + offset,
                                y: p.y + fragment.y + y,
                            });
                        }
                        x += atomic_width(a);
                        fragment.logical = *i as u32;
                        children.push(fragment);
                    }
                    Piece::Break | Piece::Mark(_) => {}
                },
            }
        }
        flush(&mut run_text, &mut children);
        for (n, start, order) in &open {
            decorations.push((*order, *n, *start, x));
        }
        decorations.sort_by_key(|d| d.0);
        let boxes: Vec<Fragment> = decorations
            .into_iter()
            .map(|(_, node, x0, x1)| {
                let style = ctx.style(node);
                let padding = style.padding(content_width);
                let border = style.border(content_width);
                let (ascent, descent) = blueice_font::ascent_descent_in(
                    style.font_size(),
                    style.style.is_some_and(|s| s.is_bold()),
                    style.style.is_some_and(|s| s.is_italic()),
                    style.web_face(),
                );
                Fragment {
                    node: Some(node),
                    kind: FragmentKind::Block,
                    x: x0,
                    y: baseline - ascent - padding.top - border.top,
                    width: (x1 - x0).max(0.0),
                    height: ascent + descent + padding.vertical() + border.vertical(),
                    hidden: !style.is_visible(),
                    ..Fragment::empty_block()
                }
            })
            .collect();
        children.splice(0..0, boxes);

        // How far the content reaches, not how far alignment pushed it: a preferred width is
        // measured with unlimited room, where a centered or right-aligned line would otherwise
        // report half of that room.
        result.extent = result
            .extent
            .max(line.left + line_indent + used_width.max(x));
        result.lines.push(Fragment {
            node: None,
            kind: FragmentKind::Line,
            x: offset,
            y,
            width: x,
            height: line_height,
            children,
            baseline: Some(baseline),
            ..Fragment::empty_block()
        });
        y += line_height;
    }
    result.height = y;
    result
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::{Fragment, FragmentKind};
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    /// The body fragment of `html` laid out at `width` with `css` on top of the UA sheet.
    fn body(html: &str, css: &str, width: f64) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!("html,body,div,p,ul,ol,li,h1,h2,h3,h4,h5,h6,pre,blockquote,figure,dl,dd,menu,fieldset,hr,section,address{{margin:0;padding:0}} {css}")).rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, width)
    }

    fn texts(line: &Fragment) -> Vec<(String, f64)> {
        line.children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some((t.clone(), c.x)),
                _ => None,
            })
            .collect()
    }

    fn word_width(text: &str) -> f64 {
        blueice_font::measure_text_width(text, 16.0, false, false)
    }

    fn space() -> f64 {
        word_width(" ")
    }

    const ROBOTO: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-400.woff2");

    #[test]
    fn text_with_a_web_font_is_measured_and_spaced_with_that_face() {
        let face = blueice_font::web::register(ROBOTO).expect("a face");
        let f = body(
            "<p>Hello world</p><p class=d>Hello world</p>",
            &format!("p:not(.d) {{ -blueice-web-font: {}; }}", face.as_u32()),
            800.0,
        );
        let own = &f.children[0].children[0];
        let plain = &f.children[1].children[0];
        let t = texts(own);
        let own_width = |text: &str| blueice_font::measure_text_width_in(text, 16.0, false, false, Some(face));
        assert_eq!(t.len(), 2);
        assert!(
            (t[1].1 - (own_width("Hello") + own_width(" "))).abs() < 0.01,
            "the second word starts where the face's own widths put it: {t:?}"
        );
        assert!((texts(plain)[1].1 - (word_width("Hello") + space())).abs() < 0.01);
        assert!((t[1].1 - texts(plain)[1].1).abs() > 0.5, "and not where the bundled font does");
        // Line boxes follow the face's own ascent and descent.
        let (a, d) = blueice_font::ascent_descent_in(16.0, false, false, Some(face));
        let (ba, bd) = blueice_font::ascent_descent(16.0, false, false);
        assert!((a - ba).abs() > 0.01 || (d - bd).abs() > 0.01);
        assert!((own.height - plain.height).abs() < 3.0);
    }

    #[test]
    fn whitespace_collapses_and_edges_are_trimmed() {
        let f = body("<p>  a \n\t b   c  </p>", "", 800.0);
        let line = &f.children[0].children[0];
        let t = texts(line);
        assert_eq!(
            t.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "exactly one space"
        );
        assert_eq!(t[0].1, 0.0, "no leading space");
    }

    #[test]
    fn adjacent_inline_elements_do_not_gain_a_space() {
        let f = body("<p><b>bo</b>ld <i>x</i></p>", "", 800.0);
        let line = &f.children[0].children[0];
        let t = texts(line);
        assert_eq!(t.len(), 3);
        let bold_bo = blueice_font::measure_text_width("bo", 16.0, true, false);
        assert!((t[1].1 - bold_bo).abs() < 0.01, "ld sits flush against bo");
        assert_eq!(
            f.children[0].children[0].children[0]
                .node
                .map(|n| n.as_u64() > 0),
            Some(true)
        );
    }

    #[test]
    fn a_space_collapses_across_an_element_boundary() {
        let f = body("<p>a <span> b</span></p>", "", 800.0);
        let t = texts(&f.children[0].children[0]);
        assert_eq!(t.len(), 2);
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "one space, not two"
        );
    }

    #[test]
    fn br_forces_a_new_line_and_a_double_br_leaves_a_blank_one() {
        let f = body("<p>a<br>b<br><br>c</p>", "", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 4, "a, b, blank, c");
        assert!(lines[2].children.is_empty());
        assert_eq!(lines[3].y, 3.0 * 19.2);
    }

    const HE1: &str = "\u{5d0}\u{5d1}\u{5d2}";
    const HE2: &str = "\u{5d3}\u{5d4}\u{5d5}";

    /// Each text run on a line, left to right: its text, where it starts, and whether it is
    /// drawn right to left.
    fn runs(line: &Fragment) -> Vec<(String, f64, bool)> {
        line.children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some((t.clone(), c.x, c.rtl)),
                _ => None,
            })
            .collect()
    }

    fn only_line(f: &Fragment) -> &Fragment {
        &f.children[0].children[0]
    }

    #[test]
    fn a_hebrew_phrase_in_a_left_to_right_block_reads_right_to_left() {
        let f = body(&format!("<p>{HE1} {HE2}</p>"), "", 300.0);
        let r = runs(only_line(&f));
        assert_eq!(r[0].0, HE2);
        assert_eq!(r[1].0, HE1);
        assert!(r[0].2 && r[1].2);
        assert_eq!(r[0].1, 0.0);
        assert!((r[1].1 - (word_width(HE2) + space())).abs() < 0.01);
    }

    #[test]
    fn latin_inside_hebrew_keeps_its_own_order_and_direction() {
        let f = body(&format!("<p>abc {HE1} {HE2} def</p>"), "", 300.0);
        let r = runs(only_line(&f));
        let order: Vec<&str> = r.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(order, ["abc", HE2, HE1, "def"]);
        assert_eq!(r.iter().map(|r| r.2).collect::<Vec<_>>(), [false, true, true, false]);
    }

    #[test]
    fn latin_in_a_right_to_left_block_is_one_left_to_right_run_placed_at_the_right() {
        let f = body("<p>abc def</p>", "p { direction: rtl }", 300.0);
        let line = only_line(&f);
        let r = runs(line);
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["abc", "def"]);
        assert!(r.iter().all(|r| !r.2), "Latin words are still drawn left to right");
        let width = word_width("abc") + space() + word_width("def");
        assert!((line.x - (300.0 - width)).abs() < 0.01, "start is the right edge");
    }

    #[test]
    fn a_right_to_left_block_reads_its_runs_from_the_right() {
        let f = body(&format!("<p>abc {HE1} def</p>"), "p { direction: rtl }", 300.0);
        let r = runs(only_line(&f));
        // abc is read first, so it is the rightmost run; def is read last, leftmost
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["def", HE1, "abc"]);
    }

    #[test]
    fn digits_in_right_to_left_text_stay_in_reading_order() {
        let f = body(&format!("<p>{HE1} 123 {HE2}</p>"), "p { direction: rtl }", 300.0);
        let r = runs(only_line(&f));
        let order: Vec<&str> = r.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(order, [HE2, "123", HE1]);
        assert_eq!(r.iter().map(|r| r.2).collect::<Vec<_>>(), [true, false, true]);
    }

    #[test]
    fn start_and_end_follow_the_direction_and_left_and_right_do_not() {
        let w = 200.0;
        let hi = word_width("hi");
        let x = |css: &str| {
            let f = body("<p>hi</p>", css, w);
            only_line(&f).x
        };
        assert_eq!(x("p { text-align: start }"), 0.0);
        assert!((x("p { text-align: end }") - (w - hi)).abs() < 0.01);
        assert!((x("p { direction: rtl; text-align: start }") - (w - hi)).abs() < 0.01);
        assert_eq!(x("p { direction: rtl; text-align: end }"), 0.0);
        assert_eq!(x("p { direction: rtl; text-align: left }"), 0.0);
        assert!((x("p { direction: rtl; text-align: right }") - (w - hi)).abs() < 0.01);
        assert!((x("p { direction: rtl }") - (w - hi)).abs() < 0.01, "start is the default");
    }

    #[test]
    fn a_dir_attribute_sets_the_direction_and_isolates_its_content() {
        let f = body(&format!("<p dir=rtl>{HE1} {HE2}</p>"), "", 300.0);
        let line = only_line(&f);
        let r = runs(line);
        assert_eq!(r[0].0, HE2, "the first word is at the right, so it is last on the line");
        let width = word_width(HE1) + space() + word_width(HE2);
        assert!((line.x - (300.0 - width)).abs() < 0.01);
        // an isolated span does not let its neighbours reorder its words
        let f = body(
            &format!("<p>abc <span dir=rtl>{HE1} {HE2}</span> def</p>"),
            "",
            300.0,
        );
        let order: Vec<String> = runs(only_line(&f)).into_iter().map(|r| r.0).collect();
        assert_eq!(order, ["abc", HE2, HE1, "def"]);
    }

    #[test]
    fn dir_auto_takes_the_direction_of_the_first_strong_letter() {
        let f = body(&format!("<p dir=auto>{HE1} abc</p>"), "", 300.0);
        let line = only_line(&f);
        assert!(line.x > 100.0, "a Hebrew paragraph is right-aligned");
        let f = body(&format!("<p dir=auto>abc {HE1}</p>"), "", 300.0);
        assert_eq!(only_line(&f).x, 0.0);
    }

    #[test]
    fn a_bdo_overrides_the_direction_of_what_it_holds() {
        let f = body("<p><bdo dir=rtl>abc def</bdo></p>", "", 300.0);
        let r = runs(only_line(&f));
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["def", "abc"]);
        assert!(r.iter().all(|r| r.2), "overridden Latin is drawn reversed too");
    }

    fn run_texts(f: &Fragment) -> Vec<Vec<String>> {
        f.children[0]
            .children
            .iter()
            .map(|line| line.children.iter().filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some(t.clone()),
                _ => None,
            }).collect())
            .collect()
    }

    #[test]
    fn an_opening_bracket_is_not_left_at_the_end_of_a_line() {
        // 3em wide: "あいう（" would put the bracket last; it goes to the next line with え
        let f = body("<p>\u{3042}\u{3044}\u{3046}\u{ff08}\u{3048}</p>", "p { width: 48px }", 400.0);
        let lines = run_texts(&f);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].concat(), "\u{3042}\u{3044}\u{3046}");
        assert_eq!(lines[1].concat(), "\u{ff08}\u{3048}");
    }

    #[test]
    fn a_dash_or_ellipsis_pair_is_never_split() {
        let f = body("<p>\u{3042}\u{3044}\u{2014}\u{2014}\u{3046}</p>", "p { width: 48px }", 400.0);
        let lines = run_texts(&f);
        assert!(
            lines.iter().all(|l| {
                let s = l.concat();
                s.matches('\u{2014}').count() != 1
            }),
            "the two dashes stay together: {lines:?}"
        );
    }

    #[test]
    fn a_stop_may_hang_past_the_end_of_a_line() {
        let text = "<p>\u{3042}\u{3044}\u{3046}\u{3048}\u{3002}</p>";
        // 4em of text and a stop in a 4em line: without hanging the stop pulls え down with it
        let plain = body(text, "p { width: 64px }", 400.0);
        assert_eq!(run_texts(&plain).len(), 2, "{:?}", run_texts(&plain));
        let hanging = body(text, "p { width: 64px; hanging-punctuation: allow-end }", 400.0);
        let lines = run_texts(&hanging);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].concat(), "\u{3042}\u{3044}\u{3046}\u{3048}\u{3002}");
        let line = &hanging.children[0].children[0];
        let last = line.children.last().unwrap();
        assert!(last.x + last.width > 64.0 + 1.0, "the stop is outside the 64px line");
    }

    fn first_line(f: &Fragment) -> &Fragment {
        &f.children[0].children[0]
    }

    #[test]
    fn force_end_hangs_a_stop_even_when_it_fits_and_allow_end_does_not() {
        let w = 100.0;
        let text = "<p>\u{3042}\u{3044}\u{3002}</p>";
        let plain = body(text, "p { text-align: right }", w);
        assert!((first_line(&plain).x - (w - 48.0)).abs() < 0.01, "three ems, all inside");
        let force = body(text, "p { text-align: right; hanging-punctuation: force-end }", w);
        assert!((first_line(&force).x - (w - 32.0)).abs() < 0.01, "the stop is outside the line");
        let allow = body(text, "p { text-align: right; hanging-punctuation: allow-end }", w);
        assert!((first_line(&allow).x - (w - 48.0)).abs() < 0.01, "it fits, so it stays inside");
    }

    #[test]
    fn first_hangs_an_opening_bracket_outside_the_start_of_the_first_line() {
        let text = "<p>\u{300c}\u{3042}\u{3044}</p><p>\u{300c}\u{3042}</p>";
        let plain = body(text, "p { width: 100px }", 100.0);
        assert_eq!(first_line(&plain).children[0].x, 0.0);
        let hung = body(text, "p { width: 100px; hanging-punctuation: first }", 100.0);
        let line = first_line(&hung);
        assert!((line.x + line.children[0].x + 16.0).abs() < 0.01, "the bracket sits left of the edge");
        // the text after the bracket starts at the edge: one em into that run
        assert!((line.x + line.children[0].x + 16.0 - 0.0).abs() < 0.01);
        // only the first line of the element
        let later = &hung.children[1].children[0];
        assert_eq!(later.x + later.children[0].x, -16.0, "each paragraph's own first line");
    }

    #[test]
    fn last_lets_a_closing_bracket_at_the_end_of_the_last_line_hang_when_it_would_not_fit() {
        let text = "<p>\u{3042}\u{3044}\u{300d}</p>";
        let plain = body(text, "p { width: 32px }", 100.0);
        assert!(run_texts(&plain).len() >= 2, "{:?}", run_texts(&plain));
        let hung = body(text, "p { width: 32px; hanging-punctuation: last }", 100.0);
        let lines = run_texts(&hung);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = first_line(&hung);
        let last = line.children.last().unwrap();
        assert!(last.x + last.width > 32.0 + 1.0, "the bracket is outside");
        // a closing bracket that is not at the end of the last line does not hang
        let mid = body("<p>\u{3042}\u{300d}\u{3044}\u{3046}</p>", "p { width: 32px; hanging-punctuation: last }", 100.0);
        assert!(run_texts(&mid).len() >= 2);
    }

    #[test]
    fn a_stop_hangs_past_the_left_end_of_a_right_to_left_line_and_an_opening_bracket_past_the_right() {
        let (gd, ab) = ("\u{5d2}\u{5d3}", "\u{5d0}\u{5d1}");
        let w = blueice_font::measure_text_width(ab, 16.0, false, false)
            + blueice_font::measure_text_width(" ", 16.0, false, false)
            + blueice_font::measure_text_width(gd, 16.0, false, false);
        let stop = blueice_font::measure_text_width(".", 16.0, false, false);
        let text = format!("<p>{gd} {ab}.</p>");
        let plain = body(&text, &format!("p {{ direction: rtl; width: {w}px }}"), 400.0);
        assert!(run_texts(&plain).len() >= 2, "the stop does not fit: {:?}", run_texts(&plain));
        let hung = body(
            &text,
            &format!("p {{ direction: rtl; width: {w}px; hanging-punctuation: allow-end }}"),
            400.0,
        );
        let lines = run_texts(&hung);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = first_line(&hung);
        let leftmost = line.children.iter().map(|c| c.x).fold(f64::INFINITY, f64::min);
        assert!(
            (line.x + leftmost + stop).abs() < 0.01,
            "the stop is just left of the box: {} {leftmost} {stop}",
            line.x
        );
        // an opening bracket at the start hangs past the right edge
        let bracket = blueice_font::measure_text_width("(", 16.0, false, false);
        let f = body(
            &format!("<p>({ab})</p>"),
            "p { direction: rtl; width: 100px; hanging-punctuation: first }",
            400.0,
        );
        let line = first_line(&f);
        let rightmost = line.children.iter().map(|c| c.x + c.width).fold(0.0, f64::max);
        assert!(line.x + rightmost > 100.0 + bracket - 0.5, "the bracket is outside on the right: {}", line.x + rightmost);
    }

    #[test]
    fn hanging_does_not_apply_to_letters_or_without_the_property() {
        let f = body("<p>aaaa bbbb.</p>", "p { width: 40px; hanging-punctuation: allow-end }", 400.0);
        assert!(run_texts(&f).len() >= 2);
        let f = body("<p>\u{3042}\u{3044}\u{3046}\u{3048}\u{3002}</p>", "p { width: 64px; hanging-punctuation: none }", 400.0);
        assert_eq!(run_texts(&f).len(), 2);
    }

    #[test]
    fn kerning_tucks_letter_pairs_together_unless_the_style_turns_it_off() {
        let width = |css: &str| {
            let f = body("<p>AVAVAV</p>", css, 400.0);
            first_line(&f).width
        };
        let kerned = width("");
        let plain = width("p { font-kerning: none }");
        assert!(kerned < plain - 0.5, "{kerned} vs {plain}");
        assert!((width("p { font-feature-settings: \"kern\" 0 }") - plain).abs() < 0.01);
        assert!((width("p { font-kerning: normal }") - kerned).abs() < 0.01);
        // letters spaced out are the sum of the kerned letters and the spacing
        assert!(width("p { letter-spacing: 1px }") > kerned + 5.0);
    }

    #[test]
    fn a_block_with_bidi_override_reads_all_of_its_text_in_its_own_direction() {
        // without it, Latin words in a right-to-left block stay left to right
        let plain = body("<p>abc def</p>", "p { direction: rtl }", 300.0);
        let r = runs(first_line(&plain));
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["abc", "def"]);
        assert!(r.iter().all(|r| !r.2));
        // with it, the words come in reverse and are drawn reversed
        let over = body("<p>abc def</p>", "p { direction: rtl; unicode-bidi: bidi-override }", 300.0);
        let r = runs(first_line(&over));
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["def", "abc"]);
        assert!(r.iter().all(|r| r.2), "{r:?}");
        // and the other way round: Hebrew in a left-to-right block overridden stays in order
        let he = "\u{5d0}\u{5d1} \u{5d2}\u{5d3}";
        let over = body(&format!("<p>{he}</p>"), "p { direction: ltr; unicode-bidi: bidi-override }", 300.0);
        let r = runs(first_line(&over));
        assert_eq!(r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["\u{5d0}\u{5d1}", "\u{5d2}\u{5d3}"]);
        assert!(r.iter().all(|r| !r.2));
    }

    /// The text runs on the first line with the size each is drawn at, as a share of the font size.
    fn scaled_runs(f: &Fragment) -> Vec<(String, f64)> {
        first_line(f)
            .children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some((t.clone(), c.size_scale)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn small_caps_are_made_of_smaller_capitals_where_the_face_has_none() {
        // the bundled face has no `smcp`
        let f = body("<p>Hello World</p>", "p { font-variant: small-caps }", 400.0);
        let runs = scaled_runs(&f);
        let texts: Vec<&str> = runs.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(texts, ["H", "ELLO", "W", "ORLD"], "{runs:?}");
        assert_eq!(runs.iter().map(|r| r.1).collect::<Vec<_>>(), [1.0, 0.7, 1.0, 0.7]);
        // the small ones are as wide as capitals at 70% of the size
        let line = first_line(&f);
        let elo = line.children.iter().find(|c| matches!(&c.kind, FragmentKind::Text(t) if t == "ELLO")).unwrap();
        assert!((elo.width - word_width_at("ELLO", 16.0 * 0.7)).abs() < 0.01, "{}", elo.width);
        // all-small-caps makes the capitals small too
        let f = body("<p>Hi</p>", "p { font-variant: all-small-caps }", 400.0);
        assert_eq!(scaled_runs(&f), [("HI".to_string(), 0.7)]);
        // the property turned off, or synthesis refused, leaves the text alone
        for css in ["p { font-variant: normal }", "p { font-variant: small-caps; font-synthesis: none }", "p { font-variant: small-caps; font-synthesis-small-caps: none }"] {
            let f = body("<p>Hello</p>", css, 400.0);
            assert_eq!(scaled_runs(&f), [("Hello".to_string(), 1.0)], "{css}");
        }
        // petite capitals without `pcap` are small capitals, and unicase draws the capitals small
        let f = body("<p>Hello</p>", "p { font-variant: petite-caps }", 400.0);
        assert_eq!(scaled_runs(&f), [("H".to_string(), 1.0), ("ELLO".to_string(), 0.7)]);
        let f = body("<p>Hi</p>", "p { font-variant: all-petite-caps }", 400.0);
        assert_eq!(scaled_runs(&f), [("HI".to_string(), 0.7)]);
        let f = body("<p>Hello</p>", "p { font-variant-caps: unicase }", 400.0);
        assert_eq!(scaled_runs(&f), [("H".to_string(), 0.7), ("ello".to_string(), 1.0)]);
        // titling capitals have no visible effect without the feature, and `smcp` 0 refuses small caps
        let f = body("<p>Hello</p>", "p { font-variant-caps: titling-caps }", 400.0);
        assert_eq!(scaled_runs(&f), [("Hello".to_string(), 1.0)]);
        let f = body("<p>Hello</p>", "p { font-variant: small-caps; font-feature-settings: \"smcp\" 0 }", 400.0);
        assert_eq!(scaled_runs(&f), [("Hello".to_string(), 1.0)]);
        // synthesis that lists the other kinds only does not synthesize small caps
        let f = body("<p>Hello</p>", "p { font-variant: small-caps; font-synthesis: weight style }", 400.0);
        assert_eq!(scaled_runs(&f), [("Hello".to_string(), 1.0)]);
    }

    fn word_width_at(text: &str, size: f64) -> f64 {
        blueice_font::measure_text_width(text, size, false, false)
    }

    /// The width of `text` set with and without the font's `halt` trimming, at 16px.
    fn full_and_half(text: &str) -> (f64, f64) {
        let halt = blueice_font::register_features(&[(*b"halt", 1)]);
        let at = |f| blueice_font::measure_text_width_features(text, 16.0, false, false, None, f);
        (at(blueice_font::FeaturesId::default()), at(halt))
    }

    fn line_width(html: &str, css: &str) -> f64 {
        first_line(&body(html, css, 400.0)).width
    }

    #[test]
    fn adjacent_fullwidth_punctuation_loses_its_blank_half_unless_space_all() {
        let (open_full, open_half) = full_and_half("\u{300c}");
        let (close_full, close_half) = full_and_half("\u{300d}");
        assert!(open_half < open_full - 1.0 && close_half < close_full - 1.0, "the face can trim: {open_half} {close_half}");
        // a closing mark before an opening one: the opening one is trimmed
        let pair = "<p>\u{300d}\u{300c}</p>";
        assert!((line_width(pair, "") - (close_full + open_half)).abs() < 0.01);
        assert!((line_width(pair, "p { text-spacing-trim: space-all }") - (close_full + open_full)).abs() < 0.01);
        // an opening mark after an opening one, a closing mark before a closing one
        assert!((line_width("<p>\u{ff08}\u{ff08}</p>", "") - (full_and_half("\u{ff08}").0 + full_and_half("\u{ff08}").1)).abs() < 0.01);
        assert!((line_width("<p>\u{ff09}\u{ff09}</p>", "") - (full_and_half("\u{ff09}").1 + full_and_half("\u{ff09}").0)).abs() < 0.01);
        // between letters both stay full width
        let framed = "<p>\u{3042}\u{300c}\u{3042}\u{300d}\u{3042}</p>";
        let a = full_and_half("\u{3042}").0;
        assert!((line_width(framed, "") - (3.0 * a + open_full + close_full)).abs() < 0.01);
        // trim-all trims them wherever they are
        assert!((line_width(framed, "p { text-spacing-trim: trim-all }") - (3.0 * a + open_half + close_half)).abs() < 0.01);
    }

    #[test]
    fn adjacent_marks_are_trimmed_across_an_inline_element_boundary() {
        let (close_full, _) = full_and_half("\u{300d}");
        let (_, open_half) = full_and_half("\u{300c}");
        let expected = close_full + open_half;
        for html in [
            "<p>\u{300d}<span>\u{300c}</span></p>",
            "<p><b>\u{300d}</b><i>\u{300c}</i></p>",
        ] {
            let w = line_width(html, "");
            assert!((w - expected).abs() < 0.5, "{html}: {w} vs {expected}");
        }
        // a space between them separates them
        let spaced = line_width("<p>\u{300d} <span>\u{300c}</span></p>", "");
        assert!(spaced > expected + 1.0, "{spaced}");
    }

    #[test]
    fn trim_start_and_trim_both_trim_a_mark_at_the_edge_of_a_line() {
        let (open_full, open_half) = full_and_half("\u{300c}");
        let a = full_and_half("\u{3042}").0;
        let html = "<p>\u{300c}\u{3042}</p>";
        // the opening mark at the start of the line: the blank half hangs before it
        let normal = body(html, "p { width: 100px }", 100.0);
        assert!(first_line(&normal).children[0].x.abs() < 0.01);
        for mode in ["trim-start", "trim-both"] {
            let f = body(html, &format!("p {{ width: 100px; text-spacing-trim: {mode} }}"), 100.0);
            let line = first_line(&f);
            assert!(
                (line.x + line.children[0].x + (open_full - open_half)).abs() < 0.01,
                "{mode}: the blank half of the bracket is outside the box: {}",
                line.x + line.children[0].x
            );
        }
        // a closing mark at the end of a line: trimmed always by trim-both, only if needed otherwise
        let (close_full, close_half) = full_and_half("\u{300d}");
        let end = "<p>\u{3042}\u{300d}</p>";
        let right = |mode: &str| {
            let f = body(end, &format!("p {{ text-align: right; width: 100px; text-spacing-trim: {mode} }}"), 100.0);
            first_line(&f).x
        };
        assert!((right("normal") - (100.0 - a - close_full)).abs() < 0.01, "it fits, so it stays");
        assert!((right("trim-both") - (100.0 - a - close_half)).abs() < 0.01, "always trimmed");
        // when it does not fit it hangs by its blank half in normal too
        let text = "<p>\u{3042}\u{3044}\u{300d}</p>";
        let tight = format!("p {{ width: {}px }}", 2.0 * a + close_half + 0.5);
        assert_eq!(run_texts(&body(text, &tight, 400.0)).len(), 1, "the closing mark hangs by its blank half");
        let space_all = format!("p {{ width: {}px; text-spacing-trim: space-all }}", 2.0 * a + close_half + 0.5);
        assert!(run_texts(&body(text, &space_all, 400.0)).len() > 1, "space-all trims nothing");
    }

    #[test]
    fn text_indent_is_taken_from_the_start_side() {
        let w = 200.0;
        let hi = word_width("hi");
        let ltr = body("<p>hi</p>", "p { text-indent: 30px }", w);
        assert_eq!(only_line(&ltr).x, 30.0);
        let rtl = body("<p>hi</p>", "p { direction: rtl; text-indent: 30px }", w);
        assert!((only_line(&rtl).x - (w - 30.0 - hi)).abs() < 0.01);
    }

    #[test]
    fn text_align_last_places_the_last_line_and_the_lines_a_break_ends() {
        let w = 200.0;
        let hi = word_width("hi");
        let x = |css: &str| {
            let f = body("<p>hi</p>", css, w);
            only_line(&f).x
        };
        assert!((x("p { text-align-last: right }") - (w - hi)).abs() < 0.01);
        assert!((x("p { text-align: right; text-align-last: left }")).abs() < 0.01);
        assert!((x("p { text-align-last: center }") - (w - hi) / 2.0).abs() < 0.01);
        assert!((x("p { direction: rtl; text-align: left; text-align-last: end }")).abs() < 0.01);
        assert!((x("p { text-align: center; text-align-last: auto }") - (w - hi) / 2.0).abs() < 0.01);
        // only the last line: an earlier line still follows text-align
        let f = body(
            "<p>aaa bbb ccc ddd eee fff ggg hhh</p>",
            "p { text-align: left; text-align-last: right }",
            80.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        assert_eq!(lines[0].x, 0.0);
        let last = lines.last().unwrap();
        assert!((last.x + last.width - 80.0).abs() < 0.01);
    }

    #[test]
    fn the_last_justified_line_of_a_right_to_left_paragraph_stays_at_the_right() {
        let f = body(
            &format!("<p>{HE1} {HE2} {HE1} {HE2} {HE1} {HE2} {HE1} {HE2} {HE1} {HE2}</p>"),
            "p { direction: rtl; text-align: justify }",
            120.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        let first = &lines[0];
        assert!(first.x.abs() < 0.5 && (first.x + first.width - 120.0).abs() < 0.5, "stretched");
        let last = lines.last().unwrap();
        assert!(
            (last.x + last.width - 120.0).abs() < 0.01 && last.x > 1.0,
            "the ragged last line sits at the start side, which is the right"
        );
    }

    #[test]
    fn an_inline_box_pads_its_start_on_the_right_in_right_to_left_text() {
        let f = body(
            &format!("<p dir=rtl><span style='padding-right:10px;background:red'>{HE1}</span></p>"),
            "",
            300.0,
        );
        let line = only_line(&f);
        let bg = line
            .children
            .iter()
            .find(|c| c.kind == FragmentKind::Block)
            .expect("a painted inline box");
        let word = line.children.iter().find(|c| matches!(c.kind, FragmentKind::Text(_))).unwrap();
        // the padding sits to the right of the text, inside the box
        assert!((bg.x + bg.width - (word.x + word.width + 10.0)).abs() < 0.01);
        assert!((bg.x - word.x).abs() < 0.01);
    }

    #[test]
    fn a_forced_break_ends_a_right_to_left_line_without_reordering_across_it() {
        let f = body(&format!("<p>{HE1} {HE2}<br>abc</p>"), "", 300.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2);
        assert_eq!(runs(&lines[0])[0].0, HE2);
        assert_eq!(runs(&lines[1])[0].0, "abc");
    }

    #[test]
    fn trailing_spaces_do_not_move_to_the_wrong_end_of_a_right_to_left_line() {
        let f = body(&format!("<p>{HE1} {HE2} </p>"), "", 300.0);
        let r = runs(only_line(&f));
        assert_eq!(r[0].1, 0.0, "the line starts with text, not a stray space");
    }

    #[test]
    fn text_align_moves_lines_and_justify_fills_all_but_the_last_line() {
        let w = 200.0;
        let f = body("<p>hi</p>", "p { text-align: right; }", w);
        let line = &f.children[0].children[0];
        assert!((line.x - (w - word_width("hi"))).abs() < 0.01);
        let f = body("<p>hi</p>", "p { text-align: center; }", w);
        assert!((f.children[0].children[0].x - (w - word_width("hi")) / 2.0).abs() < 0.01);
        let f = body(
            "<p>aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll</p>",
            "p { text-align: justify; }",
            120.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        let first_right = {
            let last = lines[0].children.last().unwrap();
            last.x + last.width
        };
        assert!(
            (first_right - 120.0).abs() < 0.5,
            "justified line reaches the right edge: {first_right}"
        );
        let last = lines.last().unwrap();
        let end = last.children.last().unwrap();
        assert!(
            end.x + end.width < 120.0 - 1.0,
            "the last line stays ragged"
        );
    }

    #[test]
    fn narrow_containers_wrap_and_long_words_overflow() {
        let f = body("<p>aaa bbb ccc</p>", "", word_width("aaa bbb") + 1.0);
        assert_eq!(
            f.children[0].children.len(),
            2,
            "two words fit, the third wraps"
        );
        let f = body("<p>aaaaaaaaaaaaaaaaaaaaaaaa bb</p>", "", 20.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].width > 20.0,
            "an unbreakable word overflows its line"
        );
    }

    #[test]
    fn white_space_nowrap_pre_pre_wrap_and_pre_line() {
        let long = "aaa bbb ccc ddd";
        let f = body(
            &format!("<p>{long}</p>"),
            "p { white-space: nowrap; }",
            40.0,
        );
        assert_eq!(f.children[0].children.len(), 1, "nowrap never wraps");
        let f = body("<p>a  b\nc</p>", "p { white-space: pre; }", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2, "the newline breaks the line");
        let t = texts(&lines[0]);
        assert!(
            (t[1].1 - (word_width("a") + 2.0 * space())).abs() < 0.01,
            "two spaces kept"
        );
        let f = body(
            &format!("<p>{long}</p>"),
            "p { white-space: pre-wrap; }",
            60.0,
        );
        assert!(f.children[0].children.len() > 1, "pre-wrap still wraps");
        let f = body("<p>a  b\nc</p>", "p { white-space: pre-line; }", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2);
        let t = texts(&lines[0]);
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "spaces collapse, newlines do not"
        );
    }

    #[test]
    fn text_nodes_are_tagged_with_their_own_element_and_font() {
        let f = body(
            "<p>a <b>big</b> <span style='font-size:32px'>B</span></p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        assert_eq!(line.children.len(), 3);
        assert!(
            line.height > 19.2,
            "the 32px span makes the line taller: {}",
            line.height
        );
        let nodes: Vec<_> = line.children.iter().map(|c| c.node).collect();
        assert!(nodes[0] != nodes[1] && nodes[1] != nodes[2]);
        let b = &line.children[2];
        assert!(b.width > 16.0 * 0.5, "measured at 32px");
    }

    #[test]
    fn display_none_inline_content_never_appears() {
        let f = body(
            "<p>a<span style='display:none'>HIDDEN</span>b<style>.q{}</style></p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let all: String = texts(line).into_iter().map(|(t, _)| t).collect();
        assert_eq!(all, "ab".to_string().replace("ab", "a") + "b", "{all}");
        assert!(!all.contains("HIDDEN") && !all.contains(".q"));
    }

    #[test]
    fn text_transform_changes_what_is_laid_out() {
        let f = body(
            "<p>hello world</p>",
            "p { text-transform: uppercase; }",
            800.0,
        );
        let t = texts(&f.children[0].children[0]);
        assert_eq!(t[0].0, "HELLO");
        let f = body(
            "<p>hello world</p>",
            "p { text-transform: capitalize; }",
            800.0,
        );
        assert_eq!(texts(&f.children[0].children[0])[1].0, "World");
    }

    #[test]
    fn letter_and_word_spacing_widen_text() {
        let plain = body("<p>ab cd</p>", "", 800.0);
        let spaced = body(
            "<p>ab cd</p>",
            "p { letter-spacing: 2px; word-spacing: 5px; }",
            800.0,
        );
        let pw = &plain.children[0].children[0].children;
        let sw = &spaced.children[0].children[0].children;
        assert!((sw[0].width - (pw[0].width + 4.0)).abs() < 0.01);
        assert!(
            (sw[1].x - (pw[1].x + 4.0 + 2.0 * 0.0 + 5.0 + 0.0 + 2.0 * 1.0)).abs() < 0.5,
            "{} vs {}",
            sw[1].x,
            pw[1].x
        );
    }

    #[test]
    fn cjk_text_breaks_between_characters_but_not_before_closing_punctuation() {
        let f = body("<p>一二三四五六七八九十</p>", "", 16.0 * 3.5);
        let lines = &f.children[0].children;
        assert!(lines.len() >= 3, "no spaces, yet it wraps: {}", lines.len());
        let f = body("<p>一二三，四五</p>", "", 16.0 * 3.2);
        let lines = &f.children[0].children;
        let second_first = texts(&lines[1])[0].0.clone();
        assert!(
            !second_first.starts_with('，'),
            "a comma never starts a line: {second_first}"
        );
        let f = body("<p>一二三</p>", "", 800.0);
        assert_eq!(
            f.children[0].children[0].children.len(),
            1,
            "one fragment for unbroken CJK text"
        );
    }

    #[test]
    fn text_indent_shifts_only_the_first_line() {
        let f = body(
            "<p>aaa bbb ccc ddd eee</p>",
            "p { text-indent: 30px; }",
            80.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        assert_eq!(texts(&lines[0])[0].1, 0.0);
        assert_eq!(lines[0].x, 30.0);
        assert_eq!(lines[1].x, 0.0);
    }

    #[test]
    fn inline_boxes_take_room_for_their_horizontal_padding_and_margin() {
        let plain = body("<p><span>a</span>b</p>", "", 800.0);
        let padded = body(
            "<p><span style='padding:0 10px;margin-right:5px'>a</span>b</p>",
            "",
            800.0,
        );
        let p = texts(&plain.children[0].children[0]);
        let q = texts(&padded.children[0].children[0]);
        assert!(
            (q[0].1 - 10.0).abs() < 0.01,
            "left padding precedes the text"
        );
        assert!(
            (q[1].1 - (p[1].1 + 25.0)).abs() < 0.01,
            "padding both sides plus the margin"
        );
    }

    #[test]
    fn inline_block_is_an_atomic_box_that_shrinks_to_fit_and_sits_on_the_line() {
        let f = body(
            "<p>x<span style='display:inline-block;padding:3px'>abc</span>y</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let atomic = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert!(
            (atomic.width - (word_width("abc") + 6.0)).abs() < 0.01,
            "shrink-to-fit: {}",
            atomic.width
        );
        let x = line
            .children
            .iter()
            .find(|c| matches!(&c.kind, FragmentKind::Text(t) if t == "x"))
            .unwrap();
        let y = line
            .children
            .iter()
            .find(|c| matches!(&c.kind, FragmentKind::Text(t) if t == "y"))
            .unwrap();
        assert!((atomic.x - x.width).abs() < 0.01, "flush after x");
        assert!(
            (y.x - (atomic.x + atomic.width)).abs() < 0.01,
            "y follows the box"
        );
        // The box's own text baseline equals the surrounding text baseline.
        let inner_line = &atomic.children[0];
        let baseline_in_box = atomic.y + inner_line.y + inner_line.baseline.unwrap();
        assert!((baseline_in_box - line.baseline.unwrap()).abs() < 0.01);
    }

    #[test]
    fn inline_blocks_wrap_when_they_do_not_fit() {
        let f = body(
            "<p><span style='display:inline-block;width:70px'>a</span><span style='display:inline-block;width:70px'>b</span></p>",
            "",
            100.0,
        );
        assert_eq!(f.children[0].children.len(), 2, "one box per line");
        let wide = body(
            "<p><span style='display:inline-block;width:70px'>a</span><span style='display:inline-block;width:70px'>b</span></p>",
            "",
            200.0,
        );
        assert_eq!(wide.children[0].children.len(), 1);
    }

    #[test]
    fn vertical_align_top_and_baseline_shift_atomic_boxes() {
        let css = "i { display: inline-block; width: 10px; height: 50px; }";
        let base = body("<p>a<i></i></p>", css, 800.0);
        let line = &base.children[0].children[0];
        assert!(
            line.height >= 50.0,
            "the box sets the line height: {}",
            line.height
        );
        let atomic = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert!(
            (atomic.y + atomic.height - line.baseline.unwrap()).abs() < 0.01,
            "bottom edge on the baseline"
        );
        let top = body("<p>a<i style='vertical-align:top'></i></p>", css, 800.0);
        let atomic = top.children[0].children[0]
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert_eq!(atomic.y, 0.0, "top aligned");
        let raised = body(
            "<p>a<span style='vertical-align:super'>b</span></p>",
            "",
            800.0,
        );
        let line = &raised.children[0].children[0];
        assert!(
            line.height > 19.2,
            "a raised span grows the line: {}",
            line.height
        );
    }

    #[test]
    fn replaced_elements_take_their_attribute_or_css_size() {
        let f = body("<p>a<img width=30 height=20>b</p>", "", 800.0);
        let line = &f.children[0].children[0];
        let img = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert_eq!((img.width, img.height), (30.0, 20.0));
        let f = body(
            "<p><img width=30 height=20 style='width:60px'></p>",
            "",
            800.0,
        );
        let img = &f.children[0].children[0].children[0];
        assert_eq!(
            (img.width, img.height),
            (60.0, 20.0),
            "CSS wins over the attribute"
        );
        let f = body("<p><svg width='24' height='24'></svg></p>", "", 800.0);
        assert_eq!(f.children[0].children[0].children[0].width, 24.0);
        let f = body(
            "<p><img width=40 height=20 style='width:80px;height:auto'></p>",
            "",
            800.0,
        );
        assert_eq!(
            f.children[0].children[0].children[0].height, 40.0,
            "auto keeps the aspect ratio"
        );
    }

    #[test]
    fn an_inline_element_holding_a_block_splits_into_block_and_text() {
        let f = body(
            "<section>a<span><div style='height:10px'>x</div>y</span></section>",
            "",
            800.0,
        );
        let kinds: Vec<_> = f.children[0]
            .children
            .iter()
            .map(|c| matches!(c.kind, FragmentKind::Line))
            .collect();
        assert_eq!(kinds, vec![true, false, true], "line, block, line");
    }

    #[test]
    fn the_container_strut_gives_empty_looking_lines_their_height() {
        let f = body("<div style='font-size:40px'>a</div>", "", 800.0);
        assert_eq!(f.children[0].height, 48.0);
        let f = body("<div style='line-height:2'>a</div>", "", 800.0);
        assert_eq!(f.children[0].height, 32.0);
        let f = body("<div style='line-height:30px'>a<br>b</div>", "", 800.0);
        assert_eq!(f.children[0].height, 60.0);
    }

    #[test]
    fn hidden_text_is_marked_but_keeps_its_place() {
        let f = body(
            "<p>a<span style='visibility:hidden'>b</span>c</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        assert_eq!(line.children.len(), 3);
        assert!(!line.children[0].hidden && line.children[1].hidden && !line.children[2].hidden);
    }

    #[test]
    fn a_painted_inline_box_becomes_a_fragment_behind_its_text() {
        let f = body(
            "<p>a <span style='background:red;padding:2px 5px'>bc</span> d</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let first = &line.children[0];
        assert!(
            matches!(first.kind, FragmentKind::Block) && first.node.is_some(),
            "the box comes before the text it sits behind"
        );
        let bc = blueice_font::measure_text_width("bc", 16.0, false, false);
        assert!(
            (first.width - (bc + 10.0)).abs() < 0.01,
            "content plus padding"
        );
        let a = blueice_font::measure_text_width("a ", 16.0, false, false);
        assert!((first.x - a).abs() < 0.01, "starts where the span does");
    }

    #[test]
    fn an_unpainted_inline_box_adds_no_fragment() {
        let f = body("<p>a <span style='padding:0 5px'>bc</span></p>", "", 800.0);
        let line = &f.children[0].children[0];
        assert!(line
            .children
            .iter()
            .all(|c| matches!(c.kind, FragmentKind::Text(_))));
    }

    #[test]
    fn a_painted_inline_box_split_across_lines_has_a_box_per_line() {
        let f = body(
            "<p><span style='background:red'>one two three four five six seven</span></p>",
            "p { width: 80px; }",
            800.0,
        );
        let boxes = f.children[0]
            .children
            .iter()
            .filter(|l| {
                matches!(
                    l.children.first().map(|c| &c.kind),
                    Some(FragmentKind::Block)
                )
            })
            .count();
        assert!(
            boxes >= 3,
            "each line carries its piece of the box, got {boxes}"
        );
    }

    fn generated_text_of(line: &Fragment) -> Vec<String> {
        line.children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn before_and_after_content_becomes_text_in_the_line() {
        let f = body(
            "<p>mid</p>",
            "p::before { content: \"[\"; } p::after { content: \"]\"; }",
            800.0,
        );
        let line = &f.children[0].children[0];
        let joined: String = generated_text_of(line).concat();
        assert_eq!(joined, "[mid]");
        let first = &line.children[0];
        assert_eq!(first.node, None, "no DOM node stands behind generated text");
        assert!(first.style_node.is_some());
    }

    #[test]
    fn attr_content_reads_the_owners_attribute() {
        let f = body(
            "<a href=\"http://x\">link</a>",
            "a::after { content: \" (\" attr(href) \")\"; }",
            800.0,
        );
        assert_eq!(
            generated_text_of(&f.children[0]),
            vec!["link".to_string(), "(http://x)".to_string()]
        );
    }

    #[test]
    fn an_empty_block_pseudo_element_is_a_box_with_its_own_size() {
        let f = body(
            "<div>text</div>",
            "div::after { content: \"\"; display: block; width: 30px; height: 7px; background: red; }",
            800.0,
        );
        let div = &f.children[0];
        let boxed = div.children.last().unwrap();
        assert_eq!((boxed.width, boxed.height), (30.0, 7.0));
        assert!(boxed.node.is_none() && boxed.style_node.is_some());
        assert_eq!(boxed.y, div.children[0].height, "after the text line");
    }

    #[test]
    fn content_none_and_missing_content_generate_nothing() {
        let f = body(
            "<p>x</p>",
            "p::before { content: none; } p::after { color: red; }",
            800.0,
        );
        assert_eq!(
            generated_text_of(&f.children[0].children[0]),
            vec!["x".to_string()]
        );
    }

    #[test]
    fn a_clearfix_pseudo_element_contains_floats_in_its_parent() {
        let f = body(
            "<div class=c><div style='float:left;width:10px;height:40px'></div></div>",
            ".c::after { content: \"\"; display: table; clear: both; }",
            800.0,
        );
        assert_eq!(f.children[0].height, 40.0);
    }

    fn img_size(html: &str, css: &str, width: f64) -> (f64, f64) {
        let f = body(
            html,
            &format!("img {{ -blueice-intrinsic-size: 200 100; }} {css}"),
            width,
        );
        let atom = f.children[0].children[0]
            .children
            .iter()
            .find(|c| c.node.is_some())
            .unwrap();
        (atom.width, atom.height)
    }

    #[test]
    fn an_image_takes_its_own_size_when_nothing_else_says() {
        assert_eq!(img_size("<p><img></p>", "", 800.0), (200.0, 100.0));
    }

    #[test]
    fn one_given_dimension_scales_the_other_by_the_images_ratio() {
        assert_eq!(img_size("<p><img width=100></p>", "", 800.0), (100.0, 50.0));
        assert_eq!(img_size("<p><img height=20></p>", "", 800.0), (40.0, 20.0));
        assert_eq!(
            img_size("<p><img></p>", "img { width: 50px; height: auto; }", 800.0),
            (50.0, 25.0)
        );
    }

    #[test]
    fn both_given_dimensions_win() {
        assert_eq!(
            img_size("<p><img width=30 height=30></p>", "", 800.0),
            (30.0, 30.0)
        );
    }

    #[test]
    fn max_width_shrinks_an_image_in_proportion() {
        assert_eq!(
            img_size(
                "<p><img></p>",
                "img { max-width: 100px; height: auto; }",
                800.0
            ),
            (100.0, 50.0)
        );
        assert_eq!(
            img_size("<p><img></p>", "img { max-width: 100%; }", 150.0),
            (150.0, 75.0)
        );
    }

    #[test]
    fn an_inline_block_shrinks_to_its_content_whatever_the_text_align() {
        // Preferred widths are measured by laying the box out with unlimited room; a centered or
        // right-aligned line must not count the room it was pushed across.
        for align in ["left", "center", "right"] {
            let f = body(
                &format!(
                    "<div style='text-align:{align};width:400px'><span style='display:inline-block'>abc</span></div>"
                ),
                "",
                800.0,
            );
            let line = &f.children[0].children[0];
            let atomic = line
                .children
                .iter()
                .find(|c| matches!(c.kind, FragmentKind::Block))
                .unwrap_or_else(|| panic!("{align}: no inline-block on the line"));
            assert!(
                (atomic.width - word_width("abc")).abs() < 0.01,
                "{align}: shrink-to-fit width was {}",
                atomic.width
            );
        }
    }

    #[test]
    fn two_centered_inline_blocks_sit_side_by_side_not_stacked() {
        let f = body(
            "<div style='text-align:center;width:400px'><span style='display:inline-block;width:50px'>a</span><span style='display:inline-block'>bb</span></div>",
            "",
            800.0,
        );
        assert_eq!(f.children[0].children.len(), 1, "one line holds both boxes");
    }

    #[test]
    fn a_button_input_is_as_wide_as_its_label_and_buttons_share_a_line() {
        let f = body(
            "<div style='width:600px'><input type=submit value='Search'><input type=button value='Go'></div>",
            "",
            800.0,
        );
        let div = &f.children[0];
        assert_eq!(div.children.len(), 1, "both buttons on one line");
        let buttons: Vec<&Fragment> = div.children[0]
            .children
            .iter()
            .filter(|c| matches!(c.kind, FragmentKind::NativeControl { .. }))
            .collect();
        assert_eq!(buttons.len(), 2);
        let search = word_width("Search");
        assert!(
            buttons[0].width > search && buttons[0].width < search + 40.0,
            "label plus padding and border, not the 20em of a text field: {}",
            buttons[0].width
        );
        assert!(buttons[1].width < buttons[0].width, "a shorter label is a narrower button");
        // The author's own width still wins.
        let wide = body(
            "<div style='width:600px'><input type=submit value='Search' style='width:200px'></div>",
            "",
            800.0,
        );
        let button = wide.children[0].children[0]
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::NativeControl { .. }))
            .unwrap();
        assert_eq!(button.width, 200.0);
    }
}
