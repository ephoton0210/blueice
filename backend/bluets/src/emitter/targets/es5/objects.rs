// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assigned computed-property literals preserve key/value and descriptor order.

use super::*;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let Some((open, close, properties)) = literal(&tokens) else {
            return Ok(emitted);
        };
        let define = unused_name(&emitted.javascript, "computed_property", &mut sequence);
        let key = unused_name(&emitted.javascript, "property_key", &mut sequence);
        let copy = unused_name(&emitted.javascript, "object_properties", &mut sequence);
        let mut replacement = "{}".to_string();
        for (first, end) in properties {
            if tokens[first].is("[") {
                let close = exponentiation::matching_close(&tokens, first)
                    .filter(|close| *close + 2 < end && tokens[*close + 1].is(":"))
                    .ok_or_else(|| {
                        unsupported(
                            module,
                            &SourceSpan::new(&module.id, 0, 0),
                            "computed property value was not retained",
                        )
                    })?;
                let expression = &emitted.javascript[tokens[first].end..tokens[close].start];
                let value = &emitted.javascript[tokens[close + 2].start..tokens[end - 1].end];
                replacement = format!("{define}({replacement}, {key}(({expression})), ({value}))");
            } else {
                if tokens[first..end].iter().any(|token| token.is("super"))
                    || tokens[first..end]
                        .windows(2)
                        .any(|pair| pair[0].is("__proto__") && pair[1].is(":"))
                {
                    return Err(unsupported(
                        module,
                        &SourceSpan::new(&module.id, 0, 0),
                        "computed literal prototype/super semantics require retained home objects",
                    ));
                }
                let source = &emitted.javascript[tokens[first].start..tokens[end - 1].end];
                replacement = format!("{copy}({replacement}, {{{source}}}, false)");
            }
        }
        let helper = include_str!("../computed_property_helpers.v1.js")
            .replace("__blueice_target_computed_property", &define)
            .replace("__blueice_target_property_key", &key);
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!(
                        "\n{helper}\n{}\n",
                        object_spread::HELPER.replace("__blueice_target_object_properties", &copy)
                    ),
                },
                TextEdit {
                    start: tokens[open].start,
                    end: tokens[close].end,
                    replacement,
                },
            ],
        );
    }
}

type Literal = (usize, usize, Vec<(usize, usize)>);

fn literal(tokens: &[Token]) -> Option<Literal> {
    for open in (1..tokens.len()).rev() {
        if !tokens[open].is("{") || !tokens[open - 1].is("=") {
            continue;
        }
        let close = exponentiation::matching_close(tokens, open)?;
        let mut properties: Vec<(usize, usize)> = Vec::new();
        let mut start = open + 1;
        let mut index = start;
        while index < close {
            if matches!(tokens[index].text.as_str(), "(" | "[" | "{") {
                index = exponentiation::matching_close(tokens, index)? + 1;
                continue;
            }
            if tokens[index].is(",") {
                if start < index {
                    properties.push((start, index));
                }
                start = index + 1;
            }
            index += 1;
        }
        if start < close {
            properties.push((start, close));
        }
        if !properties.iter().any(|&(first, _)| tokens[first].is("[")) {
            continue;
        }
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for (first, end) in properties {
            if !tokens[first].is("[") {
                if let Some(previous) = groups.last_mut().filter(|group| !tokens[group.0].is("[")) {
                    previous.1 = end;
                    continue;
                }
            }
            groups.push((first, end));
        }
        return Some((open, close, groups));
    }
    None
}
