// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! System assignment phases retain the original declaration and final binding.
use super::*;

pub(super) fn system_arrays(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target != crate::EcmaTarget::Es5
        || !module.declarations.iter().any(|declaration| matches!(declaration, Declaration::Variable(variable) if variable.pattern.as_ref().is_some_and(|pattern| matches!(pattern.pattern, BindingPattern::Array(_)))))
    {
        return Ok(emitted);
    }
    let source_tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let arrays = module
        .declarations
        .iter()
        .filter_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            let BindingPattern::Array(bindings) = &variable.pattern.as_ref()?.pattern else {
                return None;
            };
            let final_binding = bindings.iter().flatten().next_back()?;
            if final_binding.nested.is_some()
                || final_binding.default.is_some()
                || variable.pattern.as_ref()?.rest.is_some()
            {
                return None;
            }
            let opener = source_tokens.iter().find(|token| {
                variable.span.start <= token.start
                    && token.start < variable.span.end
                    && token.is("[")
            })?;
            Some((variable.span.start, opener.start, final_binding))
        })
        .collect::<Vec<_>>();
    let mut remaining = options.limits.parser.max_tokens;
    for (declaration, opener, final_binding) in arrays {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let original = coordinate(&module.source, opener);
        let mut depth = 0usize;
        let candidate = tokens.iter().enumerate().find_map(|(index, token)| {
            let top = depth == 0;
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
            if !top
                || !matches!(token.text.as_str(), "var" | "let" | "const")
                || !tokens.get(index + 2)?.is("=")
            {
                return None;
            }
            let point = owner(&emitted, tokens[index + 1].start)?;
            (point.mapped && (point.source_line, point.source_column) == original).then_some(index)
        });
        let Some(index) = candidate else {
            continue;
        };
        let helper = tokens[index + 3].start;
        let mut restore = owner(&emitted, helper).unwrap();
        let original = coordinate(&module.source, declaration);
        let from = coordinate(&emitted.javascript, tokens[index].start);
        let to = coordinate(&emitted.javascript, helper);
        for point in &mut emitted.provenance {
            if from <= (point.generated_line, point.generated_column)
                && (point.generated_line, point.generated_column) < to
            {
                point.source_line = original.0;
                point.source_column = original.1;
            }
        }
        restore.generated_line = to.0;
        restore.generated_column = to.1;
        let at = emitted
            .provenance
            .partition_point(|point| (point.generated_line, point.generated_column) < to);
        if emitted
            .provenance
            .get(at)
            .is_some_and(|point| (point.generated_line, point.generated_column) == to)
        {
            emitted.provenance[at] = restore;
        } else if emitted
            .segment_limit
            .is_some_and(|limit| emitted.provenance.len() >= limit)
        {
            emitted.overflowed = true;
        } else {
            emitted.provenance.insert(at, restore);
        }
        let mut end = assignment_end(&tokens, index + 3);
        while tokens.get(end).is_some_and(|token| token.is(",")) {
            end = assignment_end(&tokens, end + 1);
        }
        let Some(boundary) = tokens.get(end).filter(|token| token.is(";")) else {
            continue;
        };
        let final_name = tokens[index..end].iter().find(|token| {
            token.is(&final_binding.name)
                && owner(&emitted, token.start).is_some_and(|point| {
                    (point.source_line, point.source_column)
                        == coordinate(&module.source, final_binding.span.start)
                })
        });
        let Some(final_name) = final_name else {
            continue;
        };
        emitted = super::super::super::mapped_fragments::with_completion(
            emitted,
            vec![TextEdit {
                start: boundary.start,
                end: boundary.end,
                replacement: format!("{};{}", anchor(final_name.end), anchor(boundary.end)),
            }],
            boundary.end,
            boundary.end,
        );
    }
    Ok(emitted)
}

fn coordinate(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}
fn owner(emitted: &EmittedJavaScript, offset: usize) -> Option<crate::emitter::ProvenanceSegment> {
    let key = coordinate(&emitted.javascript, offset);
    let at = emitted
        .provenance
        .partition_point(|point| (point.generated_line, point.generated_column) <= key);
    at.checked_sub(1)
        .and_then(|index| emitted.provenance.get(index))
        .filter(|point| point.generated_line == key.0)
        .copied()
}
