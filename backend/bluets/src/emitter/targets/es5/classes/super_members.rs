// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Super property reads retain the method receiver after an ES5 class becomes functions.

use super::*;

pub(super) fn body(
    source: &str,
    tokens: &[Token],
    body: (usize, usize, usize),
    base: &str,
    is_static: bool,
    module: &Module,
) -> Result<String, Diagnostic> {
    let (open, close, parameter_origin) = body;
    let mut cursor = tokens[open + 1].start;
    let mut output = format!(
        "{}{{{}",
        relocations::source_anchor(parameter_origin),
        &source[tokens[open].end..cursor]
    );
    let mut index = open + 1;
    while index < close {
        if tokens[index].is("class") {
            if let Some(body) = tokens[index..close].iter().position(|token| token.is("{")) {
                if let Some(end) = exponentiation::matching_close(tokens, index + body) {
                    index = end + 1;
                    continue;
                }
            }
        }
        if !tokens[index].is("super") {
            index += 1;
            continue;
        }
        let first = index;
        let delimiter_origin = relocations::source_anchor(tokens[index + 1].start);
        let computed = tokens[index + 1].is("[");
        let key_origin = relocations::source_anchor(tokens[index + 2].start);
        let key = if tokens.get(index + 1).is_some_and(|token| token.is(".")) {
            index += 3;
            serde_json::to_string(&tokens[index - 1].text).unwrap()
        } else if tokens.get(index + 1).is_some_and(|token| token.is("[")) {
            let end = exponentiation::matching_close(tokens, index + 1).ok_or_else(|| {
                unsupported(
                    module,
                    &tokens[index].span(&module.id),
                    "super key was not retained",
                )
            })?;
            let key = relocations::relocated_verbatim(tokens[index + 1].end, tokens[end].start);
            index = end + 1;
            key
        } else {
            return Err(unsupported(
                module,
                &tokens[index].span(&module.id),
                "this super reference needs a retained property",
            ));
        };
        if tokens
            .get(index)
            .is_some_and(|token| matches!(token.text.as_str(), "=" | "+=" | "-=" | "++" | "--"))
        {
            return Err(unsupported(
                module,
                &tokens[first].span(&module.id),
                "super property writes need receiver-aware assignment lowering",
            ));
        }
        let owner = if is_static {
            base.to_string()
        } else {
            format!("{base}.prototype")
        };
        output.push_str(&relocations::relocated_verbatim(
            cursor,
            tokens[first].start,
        ));
        output.push_str(&format!(
            "{}Reflect.get({}{owner}{delimiter_origin},{key_origin} {key}{}, this{})",
            relocations::unmapped(),
            relocations::source_anchor(tokens[first].start),
            relocations::unmapped(),
            if computed {
                relocations::source_anchor(tokens[index - 1].start)
            } else {
                relocations::unmapped().to_string()
            }
        ));
        cursor = tokens[index - 1].end;
        if tokens.get(index).is_some_and(|token| token.is("(")) {
            output.push_str(&format!(
                "{}.call(this",
                relocations::source_anchor(tokens[index].start)
            ));
            if !tokens.get(index + 1).is_some_and(|token| token.is(")")) {
                output.push_str(", ");
            }
            cursor = tokens[index].end;
            index += 1;
        }
    }
    output.push_str(&relocations::relocated_verbatim(cursor, tokens[close].end));
    Ok(output)
}
