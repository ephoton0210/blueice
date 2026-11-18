// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project existing syntax and explicitly owned trivia through printing edits.

use super::*;
#[path = "projection/carriers.rs"]
mod carriers;

pub(super) fn apply(
    mut previous: EmittedJavaScript,
    edits: &[TextEdit],
    prefix: &str,
    tokens: &[Token],
    native: bool,
    prefix_owners: &BTreeSet<(usize, usize)>,
    roles: Option<&super::roles::Roles>,
) -> EmittedJavaScript {
    let mut output = String::from(prefix);
    let mut cursor = 0;
    let mut ranges = Vec::new();
    for edit in edits {
        output.push_str(&previous.javascript[cursor..edit.start]);
        let start = output.len();
        output.push_str(&edit.replacement);
        ranges.push((edit.start, edit.end, start, output.len()));
        cursor = edit.end;
    }
    output.push_str(&previous.javascript[cursor..]);
    let keywords = if native {
        tokens
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                matches!(t.text.as_str(), "var" | "let" | "const")
                    && tokens.get(i + 1).is_some_and(Token::is_binding_identifier)
            })
            .map(|(_, t)| t.end)
            .chain(
                tokens
                    .windows(2)
                    .filter(|pair| {
                        matches!(pair[0].text.as_str(), "export" | "default") && pair[1].is("class")
                    })
                    .map(|pair| pair[1].start),
            )
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let source_offsets = offsets(&previous.javascript, &previous.provenance);
    let generated_entries = carriers::generated_entries(
        &previous.javascript,
        &previous.provenance,
        &source_offsets,
        tokens,
    );
    // Inserted trivia can split one source boundary into a preceding node's
    // end and the next syntax entry. Retain both events for an inline callable
    // close and a mapped template entry, without inventing an original owner.
    let mapped_boundaries = previous
        .provenance
        .iter()
        .zip(&source_offsets)
        .filter(|(point, _)| point.mapped)
        .map(|(_, offset)| *offset)
        .collect::<BTreeSet<_>>();
    let split_boundaries = tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| {
            native && token.kind == TokenKind::Template && mapped_boundaries.contains(&token.start)
                || roles.is_some_and(|roles| {
                    matches!(
                        roles.closings[*index],
                        Some(super::roles::Kind::Function { inline: true, .. })
                    )
                })
        })
        .map(|(_, token)| token.start)
        .collect();
    let carries = carriers::copied_states(
        &previous.javascript,
        &previous.provenance,
        &source_offsets,
        edits,
        &ranges,
        carriers::CopyPolicy {
            prefix_owners,
            suppressed_boundaries: &keywords,
            split_boundaries: &split_boundaries,
        },
    );
    let mut projected = Vec::new();
    let mut retained_line_ends = BTreeSet::new();
    let mut range_index = 0;
    let mut passed = None;
    let callable_trivia = if native {
        trivia(tokens)
    } else {
        BTreeSet::new()
    };
    let function_starts = tokens
        .iter()
        .filter(|t| t.is("function"))
        .map(|t| t.start)
        .collect::<BTreeSet<_>>();
    // An empty export is a module identity marker, with no runtime syntax
    // node. Neither its written nor synthesized spelling owns a JS segment.
    let markers = if native {
        tokens
            .windows(3)
            .enumerate()
            .filter(|(_, row)| row[0].is("export") && row[1].is("{") && row[2].is("}"))
            .map(|(i, row)| {
                let end_index = if tokens.get(i + 3).is_some_and(|t| t.is(";")) {
                    i + 3
                } else {
                    i + 2
                };
                (
                    row[0].start,
                    tokens[end_index].end,
                    tokens.get(end_index + 1).map(|t| t.start),
                )
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut marker_index = 0;
    let mut previous_point: Option<&ProvenanceSegment> = None;
    for (point, offset) in previous.provenance.iter().zip(source_offsets) {
        while markers
            .get(marker_index)
            .is_some_and(|(_, end, _)| *end < offset)
        {
            marker_index += 1;
        }
        if markers.get(marker_index).is_some_and(|(start, end, next)| {
            *start <= offset && offset <= *end && *next != Some(offset)
        }) {
            continue;
        }
        // A binding keyword end is not a separate declaration end node.
        // An exported class keyword inherits its modifier end boundary.
        if keywords.contains(&offset) {
            continue;
        }
        // Callable keywords and body separators carry the preceding node's
        // range; repeating that same owner adds a false syntax boundary.
        if callable_trivia.contains(&offset)
            && previous_point.is_some_and(|p| {
                p.mapped == point.mapped
                    && p.source_line == point.source_line
                    && p.source_column == point.source_column
                    && p.name_index == point.name_index
                    && (!function_starts.contains(&offset)
                        || p.generated_line == point.generated_line)
            })
        {
            continue;
        }
        let leading_point = previous_point;
        previous_point = Some(point);
        while let Some(&(start, end, _, to_end)) = ranges.get(range_index) {
            if end > offset || (start == end && end == offset) {
                break;
            }
            passed = Some((end, to_end));
            range_index += 1;
        }
        let mut mapped = passed.map_or(offset + prefix.len(), |(end, to_end)| {
            to_end + (offset - end)
        });
        let mut inside = false;
        if let Some(&(start, end, to_start, to_end)) = ranges.get(range_index) {
            if offset == start {
                // An evaluation prefix owns inserted trivia up to the next
                // syntax role. A point on that copied role belongs after a
                // zero-width insertion, rather than before its new whitespace.
                let continues_prefix = leading_point.is_some_and(|previous| {
                    previous.mapped
                        && previous.generated_line == point.generated_line
                        && prefix_owners.contains(&(previous.source_line, previous.source_column))
                });
                let starts_role = !point.mapped
                    || !prefix_owners.contains(&(point.source_line, point.source_column));
                // Expression separators retain their owner through the gap
                // before the next syntax role. Statement boundaries do not.
                let copied_start = tokens
                    .binary_search_by_key(&offset, |token| token.start)
                    .ok();
                let after_separator = copied_start.is_some_and(|index| {
                    index > 0
                        && !super::spacing::member_name(tokens, index - 1)
                        && (super::spacing::binary(&tokens[index - 1])
                            || matches!(
                                tokens[index - 1].text.as_str(),
                                ":" | "," | "return" | "throw" | "yield" | "await"
                            ))
                });
                let copied_unmapped_start =
                    !point.mapped && !generated_entries.contains(&offset) && after_separator;
                // An operand or object member starts after its trivia. A
                // block's first statement retains its body-entry endpoint.
                let copied_mapped_start = point.mapped
                    && copied_start.is_some_and(|index| {
                        let token = &tokens[index];
                        let operand = token.is_binding_identifier()
                            || matches!(
                                token.kind,
                                TokenKind::Number
                                    | TokenKind::String
                                    | TokenKind::Template
                                    | TokenKind::RegExp
                            )
                            || matches!(
                                token.text.as_str(),
                                "await"
                                    | "yield"
                                    | "this"
                                    | "super"
                                    | "new"
                                    | "typeof"
                                    | "void"
                                    | "delete"
                                    | "true"
                                    | "false"
                                    | "null"
                                    | "#"
                                    | "..."
                            );
                        let object = roles.is_some_and(|roles| roles.before[index].kind.object());
                        // A class/namespace member begins after generated
                        // indentation, rather than owning its wrapper's end.
                        let member_entry = index > 0
                            && tokens[index - 1].is("{")
                            && roles.is_some_and(|roles| {
                                matches!(
                                    roles.before[index].kind,
                                    super::roles::Kind::Class
                                        | super::roles::Kind::Constructor
                                        | super::roles::Kind::Namespace
                                )
                            });
                        let expression_open = after_separator
                            && (matches!(token.text.as_str(), "(" | "[")
                                || token.is("{")
                                    && roles.is_some_and(|roles| {
                                        roles.openings[index]
                                            .is_some_and(super::roles::Kind::object)
                                    }));
                        operand && (after_separator || object)
                            || token.is("=>")
                            || expression_open
                            || member_entry
                            || token.is("{")
                                && index > 0
                                && matches!(
                                    tokens[index - 1].text.as_str(),
                                    "try" | "finally" | "else" | "export"
                                )
                    });
                mapped = if start == end
                    && (continues_prefix && starts_role
                        || copied_unmapped_start
                        || copied_mapped_start)
                {
                    to_end
                } else {
                    to_start
                };
            } else if start < offset && offset < end {
                inside = true;
            }
        }
        if !inside {
            if !point.mapped && generated_entries.contains(&offset) {
                retained_line_ends.insert(mapped);
            }
            projected.push((mapped, *point));
        }
    }
    // Explicit point projection may move an owner before an inserted newline
    // or remove the old line reset. Restore the old state at copied syntax,
    // including an unmapped state; never infer a new source grant from trivia.
    projected.extend(carries);
    projected.sort_by_key(|(offset, _)| *offset);
    let mut provenance = Vec::with_capacity(projected.len());
    let mut pending = projected.into_iter().peekable();
    let mut chars = output.char_indices().peekable();
    let code_ends = line_code_ends(&output);
    let (mut line, mut column) = (0, 0);
    while let Some((offset, c)) = chars.next() {
        while pending.peek().is_some_and(|p| p.0 == offset) {
            let mut point = pending.next().unwrap().1;
            point.generated_line = line;
            point.generated_column = column;
            // Each generated line begins unmapped. Its generated preamble
            // needs no explicit transition until a source node has appeared.
            if native
                && !point.mapped
                && ((offset >= code_ends[line] && !retained_line_ends.contains(&offset))
                    || provenance
                        .last()
                        .is_none_or(|p: &ProvenanceSegment| p.generated_line != line || !p.mapped))
            {
                continue;
            }
            // Collapsing a carrier gap may put the end of one source node
            // and start of another at one generated position. Preserve both
            // in source traversal order; lookup remains last-wins.
            if provenance.last() == Some(&point) {
                continue;
            }
            provenance.push(point);
        }
        if is_line_break(c, chars.peek().map(|(_, c)| *c)) {
            line += 1;
            column = 0;
        } else {
            column += c.len_utf16();
        }
    }
    for (_, mut point) in pending {
        if native && !point.mapped {
            continue;
        }
        point.generated_line = line;
        point.generated_column = column;
        provenance.push(point);
    }
    previous.javascript = output;
    previous.provenance = provenance;
    previous
}

pub(super) fn mapping_boundaries(
    emitted: &EmittedJavaScript,
) -> BTreeMap<usize, ProvenanceSegment> {
    emitted
        .provenance
        .iter()
        .zip(offsets(&emitted.javascript, &emitted.provenance))
        .map(|(point, offset)| (offset, *point))
        .collect()
}

pub(super) fn requires_separator(
    boundaries: &BTreeMap<usize, ProvenanceSegment>,
    gap: &str,
    start: usize,
    end: usize,
) -> bool {
    let Some(first) = boundaries.get(&start).filter(|point| point.mapped) else {
        return false;
    };
    let (&offset, last) = boundaries.range(start..=end).next_back().unwrap();
    if gap
        .rfind(['\r', '\n'])
        .is_some_and(|newline| offset <= start + newline)
    {
        return true;
    }
    !last.mapped
        || first.source_line != last.source_line
        || first.source_column != last.source_column
        || first.name_index != last.name_index
}

fn offsets(text: &str, points: &[ProvenanceSegment]) -> Vec<usize> {
    let mut out = Vec::with_capacity(points.len());
    let mut pending = points.iter().peekable();
    let (mut line, mut column) = (0, 0);
    let mut chars = text.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        while pending
            .peek()
            .is_some_and(|p| (p.generated_line, p.generated_column) == (line, column))
        {
            out.push(offset);
            pending.next();
        }
        if is_line_break(c, chars.peek().map(|(_, c)| *c)) {
            line += 1;
            column = 0;
        } else {
            column += c.len_utf16();
        }
    }
    while pending
        .peek()
        .is_some_and(|p| (p.generated_line, p.generated_column) == (line, column))
    {
        out.push(text.len());
        pending.next();
    }
    out
}

fn trivia(tokens: &[Token]) -> BTreeSet<usize> {
    let mut result = BTreeSet::new();
    let mut import = false;
    let mut star_export = false;
    let mut named_export = false;
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        let next = tokens.get(i + 1);
        let previous = i.checked_sub(1).map(|j| &tokens[j]);
        if depth == 0 && token.is("import") && !next.is_some_and(|t| t.is("(") || t.is(".")) {
            import = true;
        }
        if depth == 0 && token.is("export") && next.is_some_and(|token| token.is("*")) {
            star_export = true;
        }
        // Named export punctuation continues the current binding phase.
        // Only repeated owners are suppressed; coincident binding entries
        // still survive when compact syntax has no intervening whitespace.
        if depth == 0 && token.is("export") && next.is_some_and(|token| token.is("{")) {
            named_export = true;
            result.insert(token.end);
        }
        if named_export {
            if depth == 0 && token.is("{") {
                result.insert(token.end);
            } else if depth == 1 && token.is("as") {
                result.extend([token.start, token.end]);
            } else if depth == 1 && token.is("}") {
                result.insert(token.start);
                if let Some(from) = next.filter(|token| token.is("from")) {
                    result.extend([from.start, from.end]);
                }
                named_export = false;
            }
        }
        // Namespace/alias punctuation continues the existing binding phase;
        // it does not add another event with that same source owner.
        if import && depth == 0 && token.is("*") {
            result.insert(token.end);
        }
        if import && token.is("as") || star_export && token.kind != TokenKind::String {
            result.extend([token.start, token.end]);
        }
        if star_export && token.kind == TokenKind::String {
            star_export = false;
        }
        // The lossless lexer splits a private name after `#`. The whole
        // identifier has one start; `in` continues its completed operand.
        if previous.is_some_and(|token| token.is("#"))
            && matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
        {
            result.insert(token.start);
        }
        if token.is("in") && i >= 2 && tokens[i - 2].is("#") {
            result.extend([token.start, token.end]);
        }
        // A default assignment continues its parameter's existing range.
        // Repeating the same owner at `=` is not another source syntax node.
        if token.is("=") {
            result.insert(token.start);
        }
        if i >= 2
            && tokens[i - 1].is(":")
            && tokens[i - 2].text == token.text
            && token.is_binding_identifier()
        {
            // A lowered shorthand's value continues its original key end.
            // Written key/value nodes with different owners remain distinct.
            result.insert(token.start);
        }
        // Export expressions share one leading node range. Drop a repeated
        // default-modifier owner while preserving distinct declaration roles.
        if token.is("default") && previous.is_some_and(|token| token.is("export")) {
            result.insert(token.start);
        }
        if token.is("function") || token.is("extends") {
            result.extend([token.start, token.end]);
        } else if token.is("(")
            && (previous.is_some_and(|t| t.is("function"))
                || i >= 2 && tokens[i - 2].is("function"))
        {
            // A callable parameter opener continues the name/keyword phase;
            // erased generic parameters cannot add a repeated owner here.
            result.insert(token.start);
        } else if token.is("class") || token.is("constructor") && next.is_some_and(|t| t.is("(")) {
            result.insert(token.end);
        } else if token.is("{") && previous.is_some_and(|t| t.is(")"))
            || token.is(")") && previous.is_some_and(|t| t.is("("))
        {
            result.extend([token.start, token.end]);
        } else if token.is("(") && previous.is_some_and(|t| t.is(")")) {
            result.insert(token.start);
        }
        if import
            && depth == 0
            && token.is("from")
            && next.is_some_and(|t| t.kind == TokenKind::String)
        {
            result.extend([token.start, token.end]);
            import = false;
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => {
                import = false;
                star_export = false;
                named_export = false;
            }
            _ => {}
        }
    }
    result
}

fn line_code_ends(text: &str) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut last = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        if is_line_break(c, chars.peek().map(|(_, c)| *c)) {
            ends.push(last);
            last = offset + c.len_utf8();
        } else if !c.is_whitespace() {
            last = offset + c.len_utf8();
        }
    }
    ends.push(last);
    ends
}
