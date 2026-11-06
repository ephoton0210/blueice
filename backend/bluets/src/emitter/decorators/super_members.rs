// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Receiver-aware super references retain once-only keys and strict writes.

use super::*;
mod origins;

fn fresh(used: &mut BTreeSet<String>, wanted: &str) -> String {
    let mut name = wanted.to_string();
    let mut index = 0;
    while !used.insert(name.clone()) {
        index += 1;
        name = format!("{wanted}_{index}");
    }
    name
}

const ASSIGNMENTS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=", ">>>=", "&&=", "||=",
    "??=",
];

pub(in crate::emitter) fn close(tokens: &[Token], open: usize) -> Option<usize> {
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.text.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop()? != token.text {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn rhs_end(tokens: &[Token], start: usize) -> usize {
    let mut depth = 0;
    let mut conditional = 0;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth == 0 => return index,
            ")" | "]" | "}" => depth -= 1,
            "?" if depth == 0 => conditional += 1,
            ":" if depth == 0 && conditional == 0 => return index,
            ":" if depth == 0 => conditional -= 1,
            "," | ";" if depth == 0 => return index,
            _ => {}
        }
    }
    tokens.len()
}

/// Only tokens owned by one callable or static initialization are supplied.
/// Nested classes, ordinary functions and object methods keep their home object.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceMode {
    StandardStatic,
    LegacyStatic,
    Private,
}

pub(in crate::emitter) fn lower_references(
    module: &Module,
    tokens: &[Token],
    base: &str,
    receiver: &str,
    native_static_semantics: bool,
    used: &mut BTreeSet<String>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let mode = if native_static_semantics {
        ReferenceMode::StandardStatic
    } else {
        ReferenceMode::Private
    };
    lower_owned_references(module, tokens, base, receiver, mode, used, edits)
}

pub(in crate::emitter) fn lower_legacy_references(
    module: &Module,
    tokens: &[Token],
    base: &str,
    receiver: &str,
    used: &mut BTreeSet<String>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    lower_owned_references(
        module,
        tokens,
        base,
        receiver,
        ReferenceMode::LegacyStatic,
        used,
        edits,
    )
}

fn lower_owned_references(
    module: &Module,
    tokens: &[Token],
    base: &str,
    receiver: &str,
    mode: ReferenceMode,
    used: &mut BTreeSet<String>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let native_static_semantics = mode == ReferenceMode::StandardStatic;
    let retained = mode != ReferenceMode::Private;
    let first_generated_edit = edits.len();
    if retained {
        origins::static_block_header(module, tokens, edits);
    }
    let range_start = tokens.first().map_or(0, |token| token.start);
    let range_end = tokens.last().map_or(0, |token| token.end);
    let nested_homes = module
        .nested_functions
        .values()
        .filter(|function| {
            function.kind != crate::parser::NestedFunctionKind::Arrow
                && function.span.start >= range_start
                && function.span.end <= range_end
        })
        .map(|function| &function.span)
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(end) = nested_homes
            .iter()
            .filter(|span| span.start <= tokens[index].start && tokens[index].start < span.end)
            .map(|span| span.end)
            .max()
        {
            index = tokens.partition_point(|token| token.start < end);
            continue;
        }
        if matches!(tokens[index].text.as_str(), "class" | "function") {
            if let Some(body) = tokens[index..].iter().position(|token| token.is("{")) {
                if let Some(end) = close(tokens, index + body) {
                    index = end + 1;
                    continue;
                }
            }
        }
        if !tokens[index].is("super") {
            index += 1;
            continue;
        }
        if index > 0 && matches!(tokens[index - 1].text.as_str(), "." | "?.") {
            index += 1;
            continue;
        }
        let first = index;
        let (key, key_end) = if tokens.get(first + 1).is_some_and(|token| token.is(".")) {
            let token = tokens.get(first + 2).ok_or_else(|| {
                unsupported(
                    &tokens[first].span(&module.id),
                    "a super property has no key",
                )
            })?;
            (
                if mode == ReferenceMode::LegacyStatic {
                    format!(
                        "{}{}{}",
                        super::super::relocations::source_anchor(token.start),
                        quoted(&token.text),
                        super::super::relocations::source_anchor(token.end)
                    )
                } else {
                    quoted(&token.text)
                },
                first + 2,
            )
        } else if tokens.get(first + 1).is_some_and(|token| token.is("[")) {
            let end = close(tokens, first + 1).ok_or_else(|| {
                unsupported(
                    &tokens[first].span(&module.id),
                    "a super computed key was not retained",
                )
            })?;
            let expression = relocated(tokens[first + 1].end, tokens[end].start);
            if native_static_semantics {
                // Pinned standard-decorator lowering carries the raw key. Each
                // Reflect operation converts it at its own evaluation point:
                // a compound write therefore converts again after its RHS.
                (expression, end)
            } else {
                // Native home-object references convert the key once before
                // evaluating the value, preserving ordinary private semantics.
                let input = fresh(used, "_superKeyInput");
                let object = fresh(used, "_superKeyObject");
                (format!("(({input}) => {{ const {object} = {{ [{input}]: 0 }}; return Reflect.ownKeys({object})[0]; }})({expression})"), end)
            }
        } else {
            return Err(unsupported(
                &tokens[first].span(&module.id),
                "this super reference needs a retained property",
            ));
        };
        let key_name = fresh(used, "_superKey");
        let base_name = fresh(used, "_superBase");
        let receiver_name = fresh(used, "_superReceiver");
        let reference = fresh(used, "_superReference");
        let value = fresh(used, "_superValue");
        let right = fresh(used, "_superRight");
        let after = key_end + 1;
        let operator = tokens.get(after);
        let prefix = first.checked_sub(1).and_then(|before| {
            matches!(tokens[before].text.as_str(), "++" | "--").then_some(before)
        });
        let update = prefix.or_else(|| {
            operator
                .filter(|token| token.is("++") || token.is("--"))
                .map(|_| after)
        });
        let assignment = operator.filter(|token| ASSIGNMENTS.contains(&token.text.as_str()));
        let read = update.is_some() || assignment.is_some_and(|token| !token.is("="));
        let reference_end = if retained {
            let completion = if mode == ReferenceMode::LegacyStatic || read {
                tokens[key_end].end
            } else if tokens[first + 1].is("[") {
                tokens[key_end - 1].end
            } else {
                tokens[first].start
            };
            format!("{} ", super::super::relocations::source_anchor(completion))
        } else {
            String::new()
        };
        let get_end = if retained {
            format!(
                "{} ",
                super::super::relocations::source_anchor(if native_static_semantics {
                    tokens[first].end
                } else {
                    tokens[key_end].end
                })
            )
        } else {
            String::new()
        };
        let make_reference = format!(
            "{}(({key_name}, {base_name}, {receiver_name}) => [{base_name}, {key_name}, {receiver_name}{}])({key}, {base}, {receiver}){reference_end}",
            if retained { super::super::relocations::source_anchor(if native_static_semantics { prefix.map_or(tokens[first].start, |index| tokens[index].start) } else { tokens[first].start }) } else { String::new() },
            if read { format!(", Reflect.get({base_name}, {key_name}, {receiver_name})") } else { String::new() }
        );
        let store = |expression: &str, completion: usize| {
            let set = format!(
                "Reflect.set({reference}[0], {reference}[1], {expression}, {reference}[2])"
            );
            // Pinned standard-decorator static lowering ignores a false result;
            // native class/private home-object writes retain strict failure.
            if native_static_semantics {
                format!(
                    "{}{set}{} ;",
                    super::super::relocations::source_anchor(
                        prefix.map_or(tokens[first].start, |index| tokens[index].start)
                    ),
                    super::super::relocations::source_anchor(completion)
                )
            } else if mode == ReferenceMode::LegacyStatic {
                format!(
                    "{}if (!{set}{} ) throw new TypeError(\"super assignment failed\");",
                    super::super::relocations::source_anchor(
                        prefix.map_or(tokens[first].start, |index| tokens[index].start)
                    ),
                    super::super::relocations::source_anchor(completion)
                )
            } else {
                format!("if (!{set}) throw new TypeError(\"super assignment failed\");")
            }
        };
        let (start, end, replacement) = if let Some(update) = update {
            let previous = fresh(used, "_superPrevious");
            let operator = &tokens[update].text;
            (
                prefix.map_or(tokens[first].start, |at| tokens[at].start),
                if prefix.is_some() { tokens[key_end].end } else { tokens[update].end },
                format!("(({reference}) => {{ let {value} = {reference}[3]; let {previous} = {value}{operator}; {} return {}; }})({make_reference})",
                    store(&value, if prefix.is_some() { tokens[key_end].end } else { tokens[update].end }), if prefix.is_some() { &value } else { &previous }),
            )
        } else if let Some(operator) = assignment {
            let end = rhs_end(tokens, after + 1);
            let last = tokens
                .get(end.saturating_sub(1))
                .filter(|_| end > after + 1)
                .ok_or_else(|| {
                    unsupported(
                        &operator.span(&module.id),
                        "a super assignment has no right operand",
                    )
                })?;
            let rhs = relocated(tokens[after + 1].start, last.end);
            let logical = matches!(operator.text.as_str(), "||=" | "&&=" | "??=");
            let result = if logical {
                if tokens[after + 1..end]
                    .iter()
                    .any(|token| token.is("await") || token.is("yield"))
                {
                    return Err(unsupported(
                        &operator.span(&module.id),
                        "a suspending logical super assignment needs its original evaluation scope",
                    ));
                }
                let set = format!(
                    "(({value}) => {{ {} return {value}; }})({rhs})",
                    store(&value, last.end)
                );
                let choice = match operator.text.as_str() {
                    "||=" => format!("{reference}[3] || {set}"),
                    "&&=" => format!("{reference}[3] && {set}"),
                    _ => format!("{reference}[3] !== null && {reference}[3] !== void 0 ? {reference}[3] : {set}"),
                };
                if native_static_semantics {
                    let expression = match operator.text.as_str() {
                        "||=" => format!("{reference}[3] || ({rhs})"),
                        "&&=" => format!("{reference}[3] && ({rhs})"),
                        _ => format!("{reference}[3] !== null && {reference}[3] !== void 0 ? {reference}[3] : ({rhs})"),
                    };
                    format!("(({reference}) => {{ let {value} = {expression}; {} return {value}; }})({make_reference})", store(&value, last.end))
                } else {
                    format!("(({reference}) => ({choice}))({make_reference})")
                }
            } else {
                let expression = if operator.is("=") {
                    right.clone()
                } else {
                    let binary = operator.text.trim_end_matches('=');
                    format!("{reference}[3] {binary} {right}")
                };
                format!("(({reference}, {right}) => {{ let {value} = {expression}; {} return {value}; }})({make_reference}, {rhs})", store(&value, last.end))
            };
            (tokens[first].start, last.end, result)
        } else {
            let get = format!("(({key_name}, {base_name}, {receiver_name}) => Reflect.get({base_name}, {key_name}, {receiver_name}))({key}, {base}, {receiver}){get_end}");
            let (call, optional) = if operator.is_some_and(|token| token.is("(")) {
                (Some(after), false)
            } else if operator.is_some_and(|token| token.is("?."))
                && tokens.get(after + 1).is_some_and(|token| token.is("("))
            {
                (Some(after + 1), true)
            } else {
                (None, false)
            };
            if let Some(call) = call {
                let comma = if tokens.get(call + 1).is_some_and(|token| token.is(")")) {
                    ""
                } else {
                    ", "
                };
                (
                    tokens[first].start,
                    tokens[call].end,
                    format!(
                        "{get}{}.call({receiver}{comma}",
                        if optional { "?" } else { "" }
                    ),
                )
            } else {
                (tokens[first].start, tokens[key_end].end, get)
            }
        };
        edits.push(TextEdit {
            start,
            end,
            replacement,
        });
        // Relocated keys/right operands own their nested edits, including another
        // super reference or an original await expression outside generated arrows.
        index = first + 1;
    }
    // A private descriptor itself relocates the whole callable. Resolve the
    // inner operand relocations first so the existing outer relocation can
    // retain the actual rewritten assignment instead of the original super.
    if retained {
        // These initializers retain their original evaluation scope. Their
        // operand relocations and logical-reference endpoints remain typed
        // until the owning field/block is emitted; no private descriptor
        // relocates this whole callable through a second source range.
        origins::resolve_operands(edits, first_generated_edit);
        return Ok(());
    }
    let mut generated = (first_generated_edit..edits.len()).collect::<Vec<_>>();
    generated.sort_by_key(|index| (std::cmp::Reverse(edits[*index].start), edits[*index].end));
    for index in generated {
        if edits[index].replacement.contains('\u{0}') {
            let parts =
                super::super::relocations::expand(&edits[index].replacement, &module.source, edits);
            edits[index].replacement = parts.iter().map(|part| part.text()).collect();
        }
    }
    Ok(())
}

pub(in crate::emitter) fn retain_legacy_static_field_values(
    class: &ClassDeclaration,
    define: bool,
    edits: &mut Vec<TextEdit>,
) {
    if define {
        return;
    }
    for field in class
        .members
        .iter()
        .filter_map(|member| member.field.as_ref())
        .filter(|field| field.is_static && !field.accessor && !field.name.starts_with('#'))
    {
        let Some(tokens) = &field.initializer else {
            continue;
        };
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            continue;
        };
        let anchor = super::super::relocations::source_anchor(field.name_span.end);
        if first.is("super") {
            if tokens.len() == 3 && tokens[1].is(".") {
                let key_end = super::super::relocations::source_anchor(last.end);
                for edit in edits
                    .iter_mut()
                    .filter(|edit| edit.start == first.start && edit.end == last.end)
                {
                    edit.replacement = edit.replacement.replace(&key_end, &anchor);
                }
            }
            let logical = format!("{} ", super::super::relocations::source_anchor(last.end));
            for edit in edits
                .iter_mut()
                .filter(|edit| edit.start == first.start && edit.end == last.end)
            {
                if let Some(prefix) = edit.replacement.strip_suffix(&logical) {
                    edit.replacement = format!("{prefix}{anchor} ");
                }
            }
        }
        edits.push(TextEdit {
            start: last.end,
            end: last.end,
            replacement: format!("{anchor} "),
        });
    }
}

pub(super) fn retain_static_field_values(
    module: &Module,
    class: &ClassDeclaration,
    define: bool,
    target: EcmaTarget,
    edits: &mut Vec<TextEdit>,
) {
    let form = if !define {
        origins::FieldForm::Assignment
    } else if target >= EcmaTarget::Es2022 {
        origins::FieldForm::Retained
    } else {
        origins::FieldForm::Definition
    };
    for field in class
        .members
        .iter()
        .filter_map(|member| member.field.as_ref())
        .filter(|field| field.is_static && !field.accessor && !field.name.starts_with('#'))
    {
        if let Some(tokens) = &field.initializer {
            origins::field_value(module, field, tokens, form, edits);
        }
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn rewrite_static_super(
        &mut self,
        class: &ClassDeclaration,
        base: &str,
        receiver: &str,
        native_static_semantics: bool,
    ) -> Result<(), Diagnostic> {
        for shell in &class.members {
            let tokens = if shell.kind == ClassMemberKind::StaticBlock {
                &class.body[shell.token_start..shell.token_end]
            } else if let Some(field) = shell.field.as_ref().filter(|field| field.is_static) {
                let Some(tokens) = &field.initializer else {
                    continue;
                };
                tokens.as_slice()
            } else {
                continue;
            };
            lower_references(
                self.module,
                tokens,
                base,
                receiver,
                native_static_semantics,
                &mut self.names.used,
                self.edits,
            )?;
        }
        Ok(())
    }
}
