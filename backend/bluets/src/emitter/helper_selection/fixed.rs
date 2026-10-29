// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Existing private/decorator/interop call signatures implement the tslib ABI.
use super::*;

const HELPERS: &[(&str, &str)] = &[
    ("__bluetsClassPrivateGet", "__classPrivateFieldGet"),
    ("__bluetsClassPrivateSet", "__classPrivateFieldSet"),
    ("__bluetsClassPrivateIn", "__classPrivateFieldIn"),
    ("__bluetsRunInitializers", "__runInitializers"),
    ("__bluetsEsDecorate", "__esDecorate"),
    ("__bluetsDecorate", "__decorate"),
    ("__bluetsParam", "__param"),
    ("__bluetsMetadata", "__metadata"),
    ("__importDefault", "__importDefault"),
    ("__importStar", "__importStar"),
];

pub(super) fn lower(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let original = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let source_bindings = original
        .windows(2)
        .filter(|pair| matches!(pair[0].text.as_str(), "var" | "let" | "const" | "function"))
        .map(|pair| pair[1].text.as_str())
        .collect::<BTreeSet<_>>();
    let mut definitions = Vec::new();
    let mut names = BTreeMap::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.text.as_str(), "var" | "let" | "const") {
            continue;
        }
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        if source_bindings.contains(name.text.as_str()) {
            continue;
        }
        let Some((_, canonical)) = HELPERS.iter().find(|(old, _)| name.is(old)) else {
            continue;
        };
        if !tokens.get(index + 2).is_some_and(|token| token.is("=")) {
            continue;
        }
        let mut depth = 0usize;
        let mut end = None;
        for item in &tokens[index + 3..] {
            match item.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => {
                    end = Some(item.end);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            continue;
        };
        definitions.push((token.start, end));
        names.insert(name.text.clone(), reference(&emitted.javascript, canonical));
    }
    let mut edits = definitions
        .iter()
        .map(|(start, end)| TextEdit {
            start: *start,
            end: *end,
            replacement: String::new(),
        })
        .collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        let Some(replacement) = names.get(&token.text) else {
            continue;
        };
        if definitions
            .iter()
            .any(|(start, end)| token.start >= *start && token.end <= *end)
            || index
                .checked_sub(1)
                .is_some_and(|before| matches!(tokens[before].text.as_str(), "." | "?."))
        {
            continue;
        }
        edits.push(TextEdit {
            start: token.start,
            end: token.end,
            replacement: replacement.clone(),
        });
    }
    Ok(crate::emitter::targets::mapped_edits(emitted, edits))
}
