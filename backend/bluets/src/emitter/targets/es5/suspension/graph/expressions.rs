// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression continuations preserve callee and receiver evaluation order.

use super::*;

impl Builder<'_> {
    fn completed_value(
        &mut self,
        value: &str,
        binding: Option<&str>,
        completion: &str,
        next: usize,
    ) -> Option<usize> {
        let code = if completion.is_empty() {
            let assignment = binding.map_or(String::new(), |name| format!("{name} = "));
            format!("{assignment}{value}; {}", self.jump(next))
        } else {
            format!(
                "return {{kind:{}, value:({value})}};",
                serde_json::to_string(completion).ok()?
            )
        };
        Some(self.block(code))
    }

    pub(super) fn compound_expression(
        &mut self,
        tokens: &[Token],
        binding: Option<&str>,
        completion: &str,
        next: usize,
    ) -> Option<usize> {
        if tokens.first()?.is("(") && exponentiation::matching_close(tokens, 0)? == tokens.len() - 1
        {
            return self.expression(&tokens[1..tokens.len() - 1], binding, completion, next);
        }
        let mut top = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            top.push(index);
            if matches!(tokens[index].text.as_str(), "(" | "[" | "{") {
                index = exponentiation::matching_close(tokens, index)?;
            }
            index += 1;
        }
        if let Some(&equal) = top.iter().find(|&&at| tokens[at].is("=")) {
            if equal != 1 || tokens[0].kind != TokenKind::Identifier {
                return None;
            }
            let value = self.temporary();
            let finish = self.completed_value(
                &format!("({} = {value})", tokens[0].text),
                binding,
                completion,
                next,
            )?;
            return self.expression(&tokens[equal + 1..], Some(&value), "", finish);
        }
        if matches!(
            tokens[0].text.as_str(),
            "!" | "~" | "+" | "-" | "void" | "typeof"
        ) {
            let value = self.temporary();
            let finish = self.completed_value(
                &format!("{} {value}", tokens[0].text),
                binding,
                completion,
                next,
            )?;
            return self.expression(&tokens[1..], Some(&value), "", finish);
        }
        if tokens.len() >= 3 && tokens[tokens.len() - 2].is(".") {
            let value = self.temporary();
            let finish = self.completed_value(
                &format!("{value}.{}", tokens.last()?.text),
                binding,
                completion,
                next,
            )?;
            return self.expression(&tokens[..tokens.len() - 2], Some(&value), "", finish);
        }
        let &open = top.iter().rev().find(|&&at| {
            tokens[at].is("(")
                && at > 0
                && exponentiation::matching_close(tokens, at) == Some(tokens.len() - 1)
        })?;
        let arguments = &tokens[open + 1..tokens.len() - 1];
        let mut groups = Vec::new();
        let mut begin = 0;
        let mut at = 0;
        while at < arguments.len() {
            if arguments[at].is(",") {
                groups.push(&arguments[begin..at]);
                begin = at + 1;
            }
            if matches!(arguments[at].text.as_str(), "(" | "[" | "{") {
                at = exponentiation::matching_close(arguments, at)?;
            }
            at += 1;
        }
        if begin < arguments.len() {
            groups.push(&arguments[begin..]);
        }
        let callee = self.temporary();
        let receiver = self.temporary();
        let values = (0..groups.len())
            .map(|_| self.temporary())
            .collect::<Vec<_>>();
        let finish = self.completed_value(
            &format!(
                "Function.prototype.apply.call({callee}, {receiver}, [{}])",
                values.join(", ")
            ),
            binding,
            completion,
            next,
        )?;
        let mut entry = finish;
        for (group, value) in groups.iter().zip(&values).rev() {
            entry = self.expression(group, Some(value), "", entry)?;
        }
        if open >= 3 && tokens[open - 2].is(".") {
            let get = self.block(format!(
                "{callee} = {receiver}.{}; {}",
                tokens[open - 1].text,
                self.jump(entry)
            ));
            self.expression(&tokens[..open - 2], Some(&receiver), "", get)
        } else {
            let get = self.block(format!("{receiver} = void 0; {}", self.jump(entry)));
            self.expression(&tokens[..open], Some(&callee), "", get)
        }
    }
}
