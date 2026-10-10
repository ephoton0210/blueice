// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Super property reads retain the method receiver after an ES5 class becomes functions.

use super::*;

pub(super) fn body(
    source: &str,
    tokens: &[Token],
    open: usize,
    close: usize,
    base: &str,
    is_static: bool,
    module: &Module,
) -> Result<String, Diagnostic> {
    let mut output = String::new();
    let mut cursor = tokens[open].start;
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
            let key = source[tokens[index + 1].end..tokens[end].start].to_string();
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
        output.push_str(&source[cursor..tokens[first].start]);
        output.push_str(&format!("Reflect.get({owner}, {key}, this)"));
        cursor = tokens[index - 1].end;
        if tokens.get(index).is_some_and(|token| token.is("(")) {
            output.push_str(".call(this");
            if !tokens.get(index + 1).is_some_and(|token| token.is(")")) {
                output.push_str(", ");
            }
            cursor = tokens[index].end;
            index += 1;
        }
    }
    output.push_str(&source[cursor..tokens[close].end]);
    Ok(output)
}
