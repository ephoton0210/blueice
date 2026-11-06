// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class decoration replaces the receiver of public fields lowered below ES2022.
//! Identity comes from the original class and the owned name/descriptor phases;
//! source-map availability never changes the JavaScript receiver.
use super::{ClassDeclaration, ClassField, Module, Token};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Replacement {
    receiver: String,
    fields: BTreeSet<usize>,
    omitted_fields: BTreeSet<usize>,
    blocks: BTreeSet<usize>,
}

impl Replacement {
    pub(super) fn field_receiver(&self, field: &ClassField) -> Option<&str> {
        self.fields
            .contains(&field.name_span.start)
            .then_some(self.receiver.as_str())
    }

    pub(super) fn omits_field(&self, field: &ClassField) -> bool {
        self.omitted_fields.contains(&field.name_span.start)
    }

    pub(super) fn block_receiver(&self, start: usize) -> Option<&str> {
        self.blocks
            .contains(&start)
            .then_some(self.receiver.as_str())
    }
}

fn texts(tokens: &[Token]) -> Vec<&str> {
    tokens.iter().map(|token| token.text.as_str()).collect()
}

fn exact(module: &Module, tokens: &[Token], code: &str) -> bool {
    crate::lex(&module.id, code).is_ok_and(|mut expected| {
        expected.retain(|token| token.kind != crate::TokenKind::Eof);
        texts(&expected) == texts(tokens)
    })
}

fn body(class: &ClassDeclaration, index: usize) -> &[Token] {
    let member = &class.members[index];
    &class.body[member.token_start..member.token_end]
}

fn arguments(tokens: &[Token], open: usize) -> Option<Vec<&[Token]>> {
    let mut stack = vec![")"];
    let mut start = open + 1;
    let mut result = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop()? != token.text {
                    return None;
                }
                if stack.is_empty() {
                    result.push(&tokens[start..index]);
                    return Some(result);
                }
            }
            "," if stack.len() == 1 => {
                result.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    None
}

fn metadata(
    module: &Module,
    tokens: &[Token],
    receiver: &str,
    original_names: &BTreeSet<&str>,
) -> Option<String> {
    let fresh = |token: &Token| {
        token.kind == crate::TokenKind::Identifier && !original_names.contains(token.text.as_str())
    };
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !fresh(token) || !tokens.get(index + 1).is_some_and(|token| token.is("(")) {
            continue;
        }
        let Some(args) = arguments(tokens, index + 1) else {
            continue;
        };
        if args.len() != 6 || texts(args[0]) != ["null"] || texts(args[4]) != ["null"] {
            continue;
        }
        let (Some(descriptor), Some(decorators), Some(extra)) =
            (args[1].first(), args[2].first(), args[5].first())
        else {
            continue;
        };
        if !fresh(descriptor)
            || args[2].len() != 1
            || !fresh(decorators)
            || args[5].len() != 1
            || !fresh(extra)
            || !exact(
                module,
                args[1],
                &format!("{} = {{ value: {receiver} }}", descriptor.text),
            )
        {
            continue;
        }
        let Some(metadata) = args[3].get(args[3].len().saturating_sub(2)) else {
            continue;
        };
        if !fresh(metadata)
            || !exact(
                module,
                args[3],
                &format!(
                    "{{kind: \"class\", name: {receiver}.name, metadata: {}}}",
                    metadata.text
                ),
            )
        {
            continue;
        }
        let assignment = format!("{receiver} = {}.value;", descriptor.text);
        let mut expected = crate::lex(&module.id, &assignment).ok()?;
        expected.retain(|token| token.kind != crate::TokenKind::Eof);
        if tokens
            .windows(expected.len())
            .filter(|window| texts(window) == texts(&expected))
            .count()
            == 1
        {
            found.push(extra.text.clone());
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

pub(super) fn collect(original: &Module, owned: &Module) -> BTreeMap<usize, Replacement> {
    let Ok(original_tokens) = crate::lex(&original.id, &original.source) else {
        return BTreeMap::new();
    };
    let names = original_tokens
        .iter()
        .filter(|token| token.kind == crate::TokenKind::Identifier)
        .map(|token| token.text.as_str())
        .collect::<BTreeSet<_>>();
    let originals = original.classes().collect::<Vec<_>>();
    let mut result = BTreeMap::new();
    for expression in owned.class_expressions.values() {
        let class = &expression.class;
        if class.members.len() < 3 || class.members[0].static_block.is_none() {
            continue;
        }
        let header = body(class, 0);
        let assignments = header
            .windows(4)
            .filter(|tokens| texts(&tokens[1..]) == ["=", "this", ";"])
            .collect::<Vec<_>>();
        if assignments.len() != 1 {
            continue;
        }
        let receiver = &assignments[0][0];
        if receiver.kind != crate::TokenKind::Identifier || names.contains(receiver.text.as_str()) {
            continue;
        }
        let sources = originals.iter().filter(|source| {
            !source.anonymous
                && !source.decorators.is_empty()
                && exact(owned, header, &format!(
                    "static {{ Object.defineProperty(this, \"name\", {{value: {:?}, configurable: true}}); {} = this; }}",
                    source.name, receiver.text
                ))
        }).collect::<Vec<_>>();
        if sources.len() != 1 {
            continue;
        }
        let source = sources[0];
        if originals
            .iter()
            .filter(|item| item.name == source.name)
            .count()
            != 1
        {
            continue;
        }
        let blocks = class
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| member.static_block.as_ref().map(|_| index))
            .collect::<Vec<_>>();
        if blocks.len() < 2 || blocks[0] != 0 || blocks[1] != 1 {
            continue;
        }
        let meta = metadata(owned, body(class, blocks[1]), &receiver.text, &names);
        let Some(extra) = meta else {
            continue;
        };
        let ending = body(class, *blocks.last().unwrap());
        // Member extra initializers may precede the class-extra call. The
        // descriptor's exact class-extra binding identifies its final call;
        // no helper spelling or source-map coordinate grants this capability.
        let ending_calls = ending
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                (token.kind == crate::TokenKind::Identifier
                    && !names.contains(token.text.as_str())
                    && ending.get(index + 1).is_some_and(|token| token.is("(")))
                .then_some((index, token))
            })
            .filter(|(index, _)| {
                arguments(ending, index + 1).is_some_and(|args| {
                    args.len() == 2
                        && texts(args[0]) == [receiver.text.as_str()]
                        && texts(args[1]) == [extra.as_str()]
                })
            })
            .collect::<Vec<_>>();
        if ending_calls.len() != 1 {
            continue;
        }
        let wanted = source
            .members
            .iter()
            .filter_map(|member| member.field.as_ref())
            .filter(|field| field.is_static && !field.accessor && !field.name.starts_with('#'))
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        let owned_fields = class
            .members
            .iter()
            .filter_map(|member| member.field.as_ref())
            .filter(|field| field.is_static && wanted.contains(field.name.as_str()))
            .collect::<Vec<_>>();
        if owned_fields.len() != wanted.len() {
            continue;
        }
        // A pending member-extra queue can give an originally empty field an
        // initializer. Classify the actual owned initializer after decoration:
        // initialized stores use the replacement, while an unchanged empty
        // declaration has no downlevel standard-decorator store at all.
        let fields = owned_fields
            .iter()
            .filter(|field| field.initializer.is_some())
            .map(|field| field.name_span.start)
            .collect();
        let omitted_fields = owned_fields
            .iter()
            .filter(|field| field.initializer.is_none())
            .map(|field| field.name_span.start)
            .collect();
        result.insert(
            class.span.start,
            Replacement {
                receiver: receiver.text.clone(),
                fields,
                omitted_fields,
                blocks: blocks[2..]
                    .iter()
                    .map(|index| class.members[*index].span.start)
                    .collect(),
            },
        );
    }
    result
}
