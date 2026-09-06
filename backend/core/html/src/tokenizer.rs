// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML tokenizer: a `&str` -> [`Token`] stream, with no DOM awareness
//! (`../development/browser_core/research/html-parsing.md` §1/§2: in both
//! Gecko and Blink the tokenizer talks to the tree builder only through a
//! narrow callback/token interface, never touching the tree itself).
//!
//! Scope, per `phase-2-mvp-scope/PLAN.md`'s "MVP HTML scope": the tag and
//! attribute state machine is implemented close to the full spec (this is
//! the part `html-parsing.md` found "not optional" -- it's what correctly
//! delimits tokens in well-formed documents at all, not error recovery).
//! Comments and doctypes are scanned for their terminator (`-->` / `>`)
//! rather than modeled as their own sub-state-machines, since neither
//! engine's tree builder needs their *content*, only the fact that one
//! occurred (Initial-mode's doctype check) -- BlueIce doesn't materialize
//! either as a DOM node (see `tree_builder.rs`). RAWTEXT/RCDATA content
//! (`<script>`/`<style>`/`<textarea>`/`<title>`) is consumed via a direct
//! lookahead for the matching end tag rather than simulating the spec's
//! per-character `RAWTEXTLessThanSign`/`RAWTEXTEndTagOpen`/`RAWTEXTEndTagName`
//! sub-states -- behaviorally equivalent for well-formed real pages, which
//! is all the MVP bar requires. Foreign-content states (CDATA sections)
//! are not implemented at all, per the foreign-content cut.

/// Which state family the tokenizer's `Data`-equivalent state consumes
/// text under. The tree builder pushes this back into the tokenizer right
/// after inserting a `<script>`/`<style>`/`<textarea>`/`<title>` element
/// -- mirroring how Gecko/Blink let the tree builder switch tokenizer
/// state for exactly these elements (`html-parsing.md` §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentModel {
    Data,
    Rcdata,
    Rawtext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A `<!DOCTYPE ...>` occurred. Content is discarded -- BlueIce's MVP
    /// doesn't implement quirks-mode-specific behavior (see tree_builder).
    Doctype,
    /// A comment occurred. Content is discarded -- comments aren't
    /// materialized as DOM nodes for MVP (they don't affect rendering).
    Comment,
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    /// A run of consecutive character data (character references already
    /// resolved), coalesced rather than emitted one code point at a time.
    Character(String),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
}

const NAMED_REFERENCES: &[(&str, &str)] = &[
    ("amp;", "&"),
    ("lt;", "<"),
    ("gt;", ">"),
    ("quot;", "\""),
    ("apos;", "'"),
    ("nbsp;", "\u{A0}"),
];

fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r')
}

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    state: State,
    content_model: ContentModel,
    /// The tag name of the most recently emitted start tag -- an "end tag
    /// token is appropriate" (spec term) if its name matches this one.
    last_start_tag_name: Option<String>,

    tag_name: String,
    tag_is_end: bool,
    tag_self_closing: bool,
    tag_attrs: Vec<(String, String)>,
    attr_name: String,
    attr_value: String,

    text_buffer: String,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Tokenizer {
            input: input.chars().collect(),
            pos: 0,
            state: State::Data,
            content_model: ContentModel::Data,
            last_start_tag_name: None,
            tag_name: String::new(),
            tag_is_end: false,
            tag_self_closing: false,
            tag_attrs: Vec::new(),
            attr_name: String::new(),
            attr_value: String::new(),
            text_buffer: String::new(),
        }
    }

    /// Switches the tokenizer's content model. Always returns to plain
    /// `Data`-state scanning position (the caller only ever calls this
    /// right after receiving a `StartTag` token, at which point the
    /// tokenizer's internal state is already back at `State::Data`).
    pub fn set_content_model(&mut self, model: ContentModel) {
        self.content_model = model;
        self.state = State::Data;
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn matches_ahead(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.input.get(self.pos + i) == Some(&c))
    }

    fn matches_ahead_ignore_case(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(i, c)| {
            self.input
                .get(self.pos + i)
                .is_some_and(|&input_c| input_c.eq_ignore_ascii_case(&c))
        })
    }

    pub fn next_token(&mut self) -> Token {
        loop {
            match self.state {
                State::Data => match self.content_model {
                    ContentModel::Rcdata => return self.consume_rawtext_or_rcdata(true),
                    ContentModel::Rawtext => return self.consume_rawtext_or_rcdata(false),
                    ContentModel::Data => match self.peek() {
                        None => return self.flush_text_or_eof(),
                        Some('<') => {
                            if !self.text_buffer.is_empty() {
                                return self.flush_text_or_eof();
                            }
                            self.advance();
                            self.state = State::TagOpen;
                        }
                        Some('&') => {
                            self.advance();
                            let s = self.consume_character_reference();
                            self.text_buffer.push_str(&s);
                        }
                        Some(c) => {
                            self.advance();
                            self.text_buffer.push(c);
                        }
                    },
                },

                State::TagOpen => match self.peek() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.tag_is_end = false;
                        self.tag_name.clear();
                        self.tag_attrs.clear();
                        self.tag_self_closing = false;
                        self.state = State::TagName;
                    }
                    Some('/') => {
                        self.advance();
                        self.state = State::EndTagOpen;
                    }
                    Some('!') => {
                        self.advance();
                        if self.matches_ahead("--") {
                            self.pos += 2;
                            return self.consume_comment();
                        } else if self.matches_ahead_ignore_case("DOCTYPE") {
                            self.pos += 7;
                            return self.consume_doctype();
                        } else {
                            return self.consume_bogus_comment();
                        }
                    }
                    Some('?') => return self.consume_bogus_comment(),
                    None => {
                        self.state = State::Data;
                        return Token::Character("<".to_string());
                    }
                    _ => {
                        self.state = State::Data;
                        self.text_buffer.push('<');
                    }
                },

                State::EndTagOpen => match self.peek() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.tag_is_end = true;
                        self.tag_name.clear();
                        self.tag_attrs.clear();
                        self.tag_self_closing = false;
                        self.state = State::TagName;
                    }
                    Some('>') => {
                        self.advance();
                        self.state = State::Data;
                    }
                    None => {
                        self.state = State::Data;
                        return Token::Character("</".to_string());
                    }
                    _ => return self.consume_bogus_comment(),
                },

                State::TagName => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                        self.state = State::BeforeAttributeName;
                    }
                    Some('/') => {
                        self.advance();
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('>') => {
                        self.advance();
                        return self.emit_tag();
                    }
                    None => return self.emit_tag(),
                    Some(c) => {
                        self.advance();
                        self.tag_name.push(c.to_ascii_lowercase());
                    }
                },

                State::BeforeAttributeName => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                    }
                    Some('/') => {
                        self.advance();
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('>') => {
                        self.advance();
                        return self.emit_tag();
                    }
                    None => return self.emit_tag(),
                    _ => {
                        self.attr_name.clear();
                        self.attr_value.clear();
                        self.state = State::AttributeName;
                    }
                },

                State::AttributeName => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                        self.state = State::AfterAttributeName;
                    }
                    Some('/') => {
                        self.commit_pending_attr();
                        self.advance();
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('=') => {
                        self.advance();
                        self.state = State::BeforeAttributeValue;
                    }
                    Some('>') => {
                        self.commit_pending_attr();
                        self.advance();
                        return self.emit_tag();
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.advance();
                        self.attr_name.push(c.to_ascii_lowercase());
                    }
                },

                State::AfterAttributeName => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                    }
                    Some('/') => {
                        self.commit_pending_attr();
                        self.advance();
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('=') => {
                        self.advance();
                        self.state = State::BeforeAttributeValue;
                    }
                    Some('>') => {
                        self.commit_pending_attr();
                        self.advance();
                        return self.emit_tag();
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                    _ => {
                        self.commit_pending_attr();
                        self.attr_name.clear();
                        self.state = State::AttributeName;
                    }
                },

                State::BeforeAttributeValue => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                    }
                    Some('"') => {
                        self.advance();
                        self.state = State::AttributeValueDoubleQuoted;
                    }
                    Some('\'') => {
                        self.advance();
                        self.state = State::AttributeValueSingleQuoted;
                    }
                    Some('>') => {
                        self.commit_pending_attr();
                        self.advance();
                        return self.emit_tag();
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                    _ => {
                        self.state = State::AttributeValueUnquoted;
                    }
                },

                State::AttributeValueDoubleQuoted => match self.peek() {
                    Some('"') => {
                        self.commit_pending_attr();
                        self.advance();
                        self.state = State::AfterAttributeValueQuoted;
                    }
                    Some('&') => {
                        self.advance();
                        let s = self.consume_character_reference();
                        self.attr_value.push_str(&s);
                    }
                    Some(c) => {
                        self.advance();
                        self.attr_value.push(c);
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                },

                State::AttributeValueSingleQuoted => match self.peek() {
                    Some('\'') => {
                        self.commit_pending_attr();
                        self.advance();
                        self.state = State::AfterAttributeValueQuoted;
                    }
                    Some('&') => {
                        self.advance();
                        let s = self.consume_character_reference();
                        self.attr_value.push_str(&s);
                    }
                    Some(c) => {
                        self.advance();
                        self.attr_value.push(c);
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                },

                State::AttributeValueUnquoted => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.commit_pending_attr();
                        self.advance();
                        self.state = State::BeforeAttributeName;
                    }
                    Some('&') => {
                        self.advance();
                        let s = self.consume_character_reference();
                        self.attr_value.push_str(&s);
                    }
                    Some('>') => {
                        self.commit_pending_attr();
                        self.advance();
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.advance();
                        self.attr_value.push(c);
                    }
                    None => {
                        self.commit_pending_attr();
                        return self.emit_tag();
                    }
                },

                State::AfterAttributeValueQuoted => match self.peek() {
                    Some(c) if is_html_space(c) => {
                        self.advance();
                        self.state = State::BeforeAttributeName;
                    }
                    Some('/') => {
                        self.advance();
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('>') => {
                        self.advance();
                        return self.emit_tag();
                    }
                    None => return self.emit_tag(),
                    _ => {
                        self.state = State::BeforeAttributeName;
                    }
                },

                State::SelfClosingStartTag => match self.peek() {
                    Some('>') => {
                        self.tag_self_closing = true;
                        self.advance();
                        return self.emit_tag();
                    }
                    None => return self.emit_tag(),
                    _ => {
                        self.state = State::BeforeAttributeName;
                    }
                },
            }
        }
    }

    fn flush_text_or_eof(&mut self) -> Token {
        if self.text_buffer.is_empty() {
            Token::Eof
        } else {
            Token::Character(std::mem::take(&mut self.text_buffer))
        }
    }

    fn commit_pending_attr(&mut self) {
        if !self.attr_name.is_empty() {
            let name = std::mem::take(&mut self.attr_name);
            let value = std::mem::take(&mut self.attr_value);
            if !self.tag_attrs.iter().any(|(n, _)| *n == name) {
                self.tag_attrs.push((name, value));
            }
        }
        self.attr_name.clear();
        self.attr_value.clear();
    }

    fn emit_tag(&mut self) -> Token {
        self.commit_pending_attr();
        self.state = State::Data;
        let name = std::mem::take(&mut self.tag_name);
        if self.tag_is_end {
            Token::EndTag { name }
        } else {
            let attrs = std::mem::take(&mut self.tag_attrs);
            self.last_start_tag_name = Some(name.clone());
            Token::StartTag {
                name,
                attrs,
                self_closing: self.tag_self_closing,
            }
        }
    }

    /// Consumes a numeric (`&#…;`/`&#x…;`) or the small named set of
    /// character references this MVP hand-covers (see module docs),
    /// assuming the leading `&` has already been consumed. Falls back to
    /// a literal `&` on no match, per the spec's "ambiguous ampersand"
    /// behavior -- no separate lookahead/backtrack needed since an
    /// unmatched reference is just treated as plain text either way.
    fn consume_character_reference(&mut self) -> String {
        if self.peek() == Some('#') {
            self.advance();
            let hex = matches!(self.peek(), Some('x') | Some('X'));
            if hex {
                self.advance();
            }
            let mut digits = String::new();
            while let Some(c) = self.peek() {
                let ok = if hex { c.is_ascii_hexdigit() } else { c.is_ascii_digit() };
                if !ok {
                    break;
                }
                digits.push(c);
                self.advance();
            }
            if self.peek() == Some(';') {
                self.advance();
            }
            if digits.is_empty() {
                return "\u{FFFD}".to_string();
            }
            let code = u32::from_str_radix(&digits, if hex { 16 } else { 10 }).unwrap_or(0xFFFD);
            return char::from_u32(code).unwrap_or('\u{FFFD}').to_string();
        }

        for (name, value) in NAMED_REFERENCES {
            if self.matches_ahead(name) {
                self.pos += name.chars().count();
                return value.to_string();
            }
        }

        "&".to_string()
    }

    fn consume_comment(&mut self) -> Token {
        while !self.matches_ahead("-->") && self.peek().is_some() {
            self.advance();
        }
        if self.peek().is_some() {
            self.pos += 3;
        }
        self.state = State::Data;
        Token::Comment
    }

    fn consume_bogus_comment(&mut self) -> Token {
        while !matches!(self.peek(), None | Some('>')) {
            self.advance();
        }
        self.advance();
        self.state = State::Data;
        Token::Comment
    }

    fn consume_doctype(&mut self) -> Token {
        while !matches!(self.peek(), None | Some('>')) {
            self.advance();
        }
        self.advance();
        self.state = State::Data;
        Token::Doctype
    }

    /// `true` if the tokenizer is positioned at `</` followed by (a
    /// case-insensitive match of) the last start tag's name and then a
    /// valid terminator -- the spec's "appropriate end tag token" check,
    /// computed directly by lookahead rather than via the per-character
    /// `*LessThanSign`/`*EndTagOpen`/`*EndTagName` sub-states (see module
    /// docs for why that's a safe simplification here).
    fn matches_appropriate_end_tag_ahead(&self) -> bool {
        let Some(name) = &self.last_start_tag_name else {
            return false;
        };
        if self.input.get(self.pos) != Some(&'<') || self.input.get(self.pos + 1) != Some(&'/') {
            return false;
        }
        let start = self.pos + 2;
        let name_len = name.chars().count();
        if !name
            .chars()
            .enumerate()
            .all(|(i, c)| self.input.get(start + i).is_some_and(|&ic| ic.eq_ignore_ascii_case(&c)))
        {
            return false;
        }
        matches!(
            self.input.get(start + name_len),
            None | Some(' ') | Some('\t') | Some('\n') | Some('\x0C') | Some('\r') | Some('/') | Some('>')
        )
    }

    fn consume_end_tag_simple(&mut self) -> Token {
        let name = self.last_start_tag_name.clone().unwrap_or_default();
        self.pos += 2 + name.chars().count();
        while let Some(c) = self.advance() {
            if c == '>' {
                break;
            }
        }
        self.state = State::Data;
        Token::EndTag { name }
    }

    fn consume_rawtext_or_rcdata(&mut self, rcdata: bool) -> Token {
        loop {
            match self.peek() {
                None => return self.flush_text_or_eof(),
                Some('<') if self.matches_appropriate_end_tag_ahead() => {
                    if !self.text_buffer.is_empty() {
                        return self.flush_text_or_eof();
                    }
                    return self.consume_end_tag_simple();
                }
                Some('&') if rcdata => {
                    self.advance();
                    let s = self.consume_character_reference();
                    self.text_buffer.push_str(&s);
                }
                Some(c) => {
                    self.advance();
                    self.text_buffer.push(c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize_all(input: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = t.next_token();
            let is_eof = tok == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn start(name: &str, attrs: &[(&str, &str)]) -> Token {
        Token::StartTag {
            name: name.to_string(),
            attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            self_closing: false,
        }
    }

    fn end(name: &str) -> Token {
        Token::EndTag { name: name.to_string() }
    }

    fn text(s: &str) -> Token {
        Token::Character(s.to_string())
    }

    #[test]
    fn simple_element_with_attribute_and_text() {
        let tokens = tokenize_all(r#"<div class="a">hi</div>"#);
        assert_eq!(
            tokens,
            vec![start("div", &[("class", "a")]), text("hi"), end("div"), Token::Eof]
        );
    }

    #[test]
    fn self_closing_flag_is_recorded() {
        let tokens = tokenize_all("<br/>");
        assert_eq!(
            tokens,
            vec![
                Token::StartTag {
                    name: "br".to_string(),
                    attrs: vec![],
                    self_closing: true,
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn unquoted_attribute_value() {
        let tokens = tokenize_all("<input type=text>");
        assert_eq!(tokens, vec![start("input", &[("type", "text")]), Token::Eof]);
    }

    #[test]
    fn single_quoted_attribute_value() {
        let tokens = tokenize_all("<a href='x'>t</a>");
        assert_eq!(
            tokens,
            vec![start("a", &[("href", "x")]), text("t"), end("a"), Token::Eof]
        );
    }

    #[test]
    fn multiple_attributes_and_duplicate_is_dropped() {
        let tokens = tokenize_all(r#"<div id="a" class="b" id="c">"#);
        assert_eq!(
            tokens,
            vec![start("div", &[("id", "a"), ("class", "b")]), Token::Eof]
        );
    }

    #[test]
    fn tag_and_attribute_names_are_lowercased() {
        let tokens = tokenize_all(r#"<DIV CLASS="x"></DIV>"#);
        assert_eq!(
            tokens,
            vec![start("div", &[("class", "x")]), end("div"), Token::Eof]
        );
    }

    #[test]
    fn comment_produces_a_token_but_no_content() {
        let tokens = tokenize_all("a<!-- hi -- there -->b");
        assert_eq!(tokens, vec![text("a"), Token::Comment, text("b"), Token::Eof]);
    }

    #[test]
    fn doctype_produces_a_token() {
        let tokens = tokenize_all("<!DOCTYPE html><p>x</p>");
        assert_eq!(
            tokens,
            vec![Token::Doctype, start("p", &[]), text("x"), end("p"), Token::Eof]
        );
    }

    #[test]
    fn bogus_comment_from_bang_not_comment_or_doctype() {
        let tokens = tokenize_all("<![CDATA[x]]>y");
        assert_eq!(tokens, vec![Token::Comment, text("y"), Token::Eof]);
    }

    #[test]
    fn bogus_comment_from_question_mark() {
        let tokens = tokenize_all("<?xml version='1.0'?>y");
        assert_eq!(tokens, vec![Token::Comment, text("y"), Token::Eof]);
    }

    #[test]
    fn stray_end_tag_with_no_name_is_ignored() {
        let tokens = tokenize_all("a</>b");
        assert_eq!(tokens, vec![text("a"), text("b"), Token::Eof]);
    }

    #[test]
    fn named_character_references() {
        let tokens = tokenize_all("a &amp; &lt;&gt; &quot;&apos;&nbsp;b");
        assert_eq!(tokens, vec![text("a & <> \"'\u{A0}b"), Token::Eof]);
    }

    #[test]
    fn numeric_and_hex_character_references() {
        let tokens = tokenize_all("&#65;&#x41;");
        assert_eq!(tokens, vec![text("AA"), Token::Eof]);
    }

    #[test]
    fn ambiguous_ampersand_is_literal() {
        let tokens = tokenize_all("Q&A team");
        assert_eq!(
            tokens,
            vec![text("Q&A team"), Token::Eof],
            "no semicolon-terminated match should just be literal text"
        );
    }

    #[test]
    fn character_reference_in_attribute_value() {
        let tokens = tokenize_all(r#"<a href="?a=1&amp;b=2">x</a>"#);
        assert_eq!(
            tokens,
            vec![start("a", &[("href", "?a=1&b=2")]), text("x"), end("a"), Token::Eof]
        );
    }

    /// The tokenizer alone never switches content model on its own -- per
    /// the module docs, that's the tree builder's job (it calls
    /// `set_content_model` right after seeing the relevant start tag, the
    /// same way Gecko/Blink let their tree builders drive tokenizer
    /// state). These tests drive that handshake manually to exercise the
    /// tokenizer's RAWTEXT/RCDATA consumption in isolation.
    fn tokenize_with_content_switch(input: &str, tag: &str, model: ContentModel) -> Vec<Token> {
        let mut t = Tokenizer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = t.next_token();
            let is_eof = tok == Token::Eof;
            let is_target_start = matches!(&tok, Token::StartTag { name, .. } if name == tag);
            tokens.push(tok);
            if is_target_start {
                t.set_content_model(model);
            }
            if is_eof {
                break;
            }
        }
        tokens
    }

    #[test]
    fn rawtext_script_content_is_opaque() {
        let tokens =
            tokenize_with_content_switch("<script>if (a < b) { x(); }</script>done", "script", ContentModel::Rawtext);
        assert_eq!(
            tokens,
            vec![
                start("script", &[]),
                text("if (a < b) { x(); }"),
                end("script"),
                text("done"),
                Token::Eof
            ]
        );
    }

    #[test]
    fn rawtext_does_not_process_character_references() {
        let tokens = tokenize_with_content_switch("<style>a &amp; b</style>", "style", ContentModel::Rawtext);
        assert_eq!(
            tokens,
            vec![start("style", &[]), text("a &amp; b"), end("style"), Token::Eof]
        );
    }

    #[test]
    fn rcdata_title_processes_character_references() {
        let tokens = tokenize_with_content_switch("<title>A &amp; B</title>", "title", ContentModel::Rcdata);
        assert_eq!(
            tokens,
            vec![start("title", &[]), text("A & B"), end("title"), Token::Eof]
        );
    }

    #[test]
    fn rawtext_end_tag_requires_matching_name() {
        let tokens = tokenize_with_content_switch("<script>a</b>c</script>", "script", ContentModel::Rawtext);
        assert_eq!(
            tokens,
            vec![start("script", &[]), text("a</b>c"), end("script"), Token::Eof]
        );
    }

    #[test]
    fn eof_mid_tag_still_emits_it_leniently() {
        let tokens = tokenize_all("<div class=\"x\"");
        assert_eq!(tokens, vec![start("div", &[("class", "x")]), Token::Eof]);
    }

    #[test]
    fn eof_mid_rawtext_flushes_pending_text() {
        let tokens = tokenize_all("<script>a");
        assert_eq!(tokens, vec![start("script", &[]), text("a"), Token::Eof]);
    }

    #[test]
    fn lone_lt_and_lone_end_slash_at_eof_are_literal() {
        assert_eq!(tokenize_all("a<"), vec![text("a"), text("<"), Token::Eof]);
        assert_eq!(tokenize_all("a</"), vec![text("a"), text("</"), Token::Eof]);
    }

    #[test]
    fn invalid_tag_open_char_is_literal() {
        // the '<' is flushed as its own text run (buffer was already
        // non-empty from "a") before TagOpen decides '3' isn't a valid
        // tag start and falls back to treating "<3b" as literal text.
        let tokens = tokenize_all("a<3b");
        assert_eq!(tokens, vec![text("a"), text("<3b"), Token::Eof]);
    }

    #[test]
    fn end_tag_open_with_invalid_char_is_bogus_comment() {
        let tokens = tokenize_all("a</3>b");
        assert_eq!(tokens, vec![text("a"), Token::Comment, text("b"), Token::Eof]);
    }

    #[test]
    fn eof_mid_tag_name_emits_leniently() {
        let tokens = tokenize_all("<di");
        assert_eq!(
            tokens,
            vec![
                Token::StartTag {
                    name: "di".to_string(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn eof_before_attribute_name_emits_leniently() {
        let tokens = tokenize_all("<div ");
        assert_eq!(tokens, vec![start("div", &[]), Token::Eof]);
    }

    #[test]
    fn after_attribute_name_starts_a_new_attribute_without_whitespace_state_loop() {
        // whitespace after `a` enters AfterAttributeName; `b` then starts a
        // second attribute directly (the "_ =>" arm of AfterAttributeName).
        let tokens = tokenize_all("<div a b=\"1\">");
        assert_eq!(tokens, vec![start("div", &[("a", ""), ("b", "1")]), Token::Eof]);
    }

    #[test]
    fn eof_after_equals_with_no_value_emits_leniently() {
        let tokens = tokenize_all("<div a=");
        assert_eq!(tokens, vec![start("div", &[("a", "")]), Token::Eof]);
    }

    #[test]
    fn eof_mid_double_quoted_value_emits_leniently() {
        let tokens = tokenize_all(r#"<div a="unterminated"#);
        assert_eq!(tokens, vec![start("div", &[("a", "unterminated")]), Token::Eof]);
    }

    #[test]
    fn eof_mid_single_quoted_value_emits_leniently() {
        let tokens = tokenize_all("<div a='unterminated");
        assert_eq!(tokens, vec![start("div", &[("a", "unterminated")]), Token::Eof]);
    }

    #[test]
    fn character_reference_in_unquoted_attribute_value() {
        let tokens = tokenize_all("<div a=x&amp;y>");
        assert_eq!(tokens, vec![start("div", &[("a", "x&y")]), Token::Eof]);
    }

    #[test]
    fn self_closing_slash_right_after_quoted_value() {
        let tokens = tokenize_all(r#"<br a="x"/>"#);
        assert_eq!(
            tokens,
            vec![
                Token::StartTag {
                    name: "br".to_string(),
                    attrs: vec![("a".to_string(), "x".to_string())],
                    self_closing: true,
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn garbage_right_after_quoted_value_reconsumes_as_new_attribute() {
        let tokens = tokenize_all(r#"<div a="x"b="y">"#);
        assert_eq!(tokens, vec![start("div", &[("a", "x"), ("b", "y")]), Token::Eof]);
    }

    #[test]
    fn slash_not_followed_by_gt_is_not_self_closing() {
        let tokens = tokenize_all("<br/ >");
        assert_eq!(
            tokens,
            vec![
                Token::StartTag {
                    name: "br".to_string(),
                    attrs: vec![],
                    self_closing: false,
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn numeric_character_reference_with_no_digits_is_replacement_char() {
        let tokens = tokenize_all("a&#;b");
        assert_eq!(tokens, vec![text("a\u{FFFD}b"), Token::Eof]);
    }

    #[test]
    fn appropriate_end_tag_check_false_with_no_prior_start_tag() {
        // No start tag has ever been seen, so nothing can be an
        // "appropriate" end tag -- exercised directly since content-model
        // switching (and thus RAWTEXT/RCDATA scanning) is normally only
        // ever entered right after a StartTag token in real usage.
        let mut t = Tokenizer::new("</x>done");
        t.set_content_model(ContentModel::Rawtext);
        assert_eq!(t.next_token(), text("</x>done"));
        assert_eq!(t.next_token(), Token::Eof);
    }

    #[test]
    fn attribute_with_no_value_and_trailing_slash_before_gt() {
        let tokens = tokenize_all("<input disabled />");
        assert_eq!(
            tokens,
            vec![
                Token::StartTag {
                    name: "input".to_string(),
                    attrs: vec![("disabled".to_string(), String::new())],
                    self_closing: true,
                },
                Token::Eof
            ]
        );
    }
}
