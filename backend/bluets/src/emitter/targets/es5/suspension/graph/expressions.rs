// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression continuations retain reference, receiver and operand ordering.
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
            let assignment = binding.map_or(String::new(), |name| self.binding_prefix(name));
            format!(
                "{assignment}{value}{}{}",
                self.assignment_terminator(binding),
                self.jump(next)
            )
        } else {
            self.completion_code(completion, value)?
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
            let inner = &tokens[1..tokens.len() - 1];
            let temporary = self.temporary();
            let value = format!(
                "{}{}{}{}",
                relocations::relocated_verbatim(
                    self.source_offset + tokens[0].start,
                    self.source_offset + tokens[0].end
                ),
                self.grouped_value_reference(&temporary, tokens),
                relocations::relocated_verbatim(
                    self.source_offset + tokens.last()?.start,
                    self.source_offset + tokens.last()?.end
                ),
                relocations::unmapped()
            );
            let finish = self.completed_value(&value, binding, completion, next)?;
            return self.expression(inner, Some(&temporary), "", finish);
        }
        let top = top_level(tokens)?;
        if let Some(&comma) = top.iter().find(|&&at| tokens[at].is(",")) {
            let right = self.expression(&tokens[comma + 1..], binding, completion, next)?;
            return self.expression(&tokens[..comma], None, "", right);
        }
        if let Some(&equal) = top.iter().find(|&&at| {
            matches!(
                tokens[at].text.as_str(),
                "=" | "+=" | "-=" | "*=" | "/=" | "%="
            )
        }) {
            return self.assignment(tokens, equal, binding, completion, next);
        }
        if let Some(&question) = top.iter().find(|&&at| tokens[at].is("?")) {
            let mut depth = 0usize;
            let colon = *top.iter().skip_while(|&&at| at <= question).find(|&&at| {
                if tokens[at].is("?") {
                    depth += 1;
                }
                if tokens[at].is(":") {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            })?;
            let mut joined = None;
            let mut continuation = next;
            let branch_completion = if completion.is_empty() {
                completion
            } else {
                let result = self.temporary();
                let previous = self.completion_site;
                self.completion_site = previous.map(|(start, _, end)| (start, start, end));
                let carried = format!(
                    "{}{result}{}",
                    self.completion_origin(),
                    relocations::unmapped()
                );
                let finish = self.completed_value(&carried, binding, completion, next);
                self.completion_site = previous;
                continuation = finish?;
                joined = Some(result);
                ""
            };
            let destination = joined.as_deref().or(binding);
            let consequent = self.expression(
                &tokens[question + 1..colon],
                destination,
                branch_completion,
                continuation,
            )?;
            let alternate = self.expression(
                &tokens[colon + 1..],
                destination,
                branch_completion,
                continuation,
            )?;
            let condition = self.temporary();
            if self.asynchronous {
                self.binding_initializers.insert(
                    condition.clone(),
                    origins::InitializerOrigin {
                        start: tokens.first()?.start,
                        end: tokens[question - 1].end,
                        statement: false,
                    },
                );
            }
            let branch = self.block(format!(
                "{}.label = {condition} ? {consequent} : {alternate}; continue;",
                self.context
            ));
            return self.expression(&tokens[..question], Some(&condition), "", branch);
        }
        for operators in [
            &["??"][..],
            &["||"][..],
            &["&&"][..],
            &["|"][..],
            &["^"][..],
            &["&"][..],
            &["==", "!=", "===", "!=="][..],
            &["<", ">", "<=", ">=", "in", "instanceof"][..],
            &["<<", ">>", ">>>"][..],
            &["+", "-"][..],
            &["*", "/", "%"][..],
            &["**"][..],
        ] {
            let mut candidates = top.iter().filter(|&&at| {
                at > 0
                    && at + 1 < tokens.len()
                    && operators.contains(&tokens[at].text.as_str())
                    && !is_prefix(tokens, at)
            });
            let at = if operators == ["**"] {
                candidates.next()
            } else {
                candidates.next_back()
            };
            if let Some(&at) = at {
                let left = self.temporary();
                let right = self.temporary();
                let operator = tokens[at].text.as_str();
                if matches!(operator, "&&" | "||" | "??") {
                    let skipped = self.completed_value(&left, binding, completion, next)?;
                    let evaluated =
                        self.expression(&tokens[at + 1..], binding, completion, next)?;
                    let condition = match operator {
                        "&&" => left.clone(),
                        "||" => format!("!{left}"),
                        _ => format!("{left} === null || {left} === void 0"),
                    };
                    let branch = self.block(format!(
                        "{}.label = ({condition}) ? {evaluated} : {skipped}; continue;",
                        self.context
                    ));
                    return self.expression(&tokens[..at], Some(&left), "", branch);
                }
                let value = if operator == "**" {
                    format!("Math.pow({left}, {right})")
                } else {
                    let left_tokens = &tokens[..at];
                    let right_tokens = &tokens[at + 1..];
                    let operator_origin = if self.suspends(right_tokens) {
                        left_tokens.first()?.start
                    } else {
                        left_tokens.last()?.end
                    };
                    format!(
                        "{}({} {}{operator}{} {}){}",
                        relocations::unmapped(),
                        self.value_reference(&left, left_tokens),
                        relocations::source_anchor(self.source_offset + operator_origin),
                        relocations::unmapped(),
                        self.value_reference(&right, right_tokens),
                        relocations::unmapped()
                    )
                };
                let finish = self.completed_value(&value, binding, completion, next)?;
                let second = self.expression(&tokens[at + 1..], Some(&right), "", finish)?;
                return self.expression(&tokens[..at], Some(&left), "", second);
            }
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
                &format!(
                    "{}{}{}",
                    self.value_reference(&value, &tokens[..tokens.len() - 2]),
                    relocations::relocated_verbatim(
                        self.source_offset + tokens[tokens.len() - 2].start,
                        self.source_offset + tokens.last()?.end
                    ),
                    relocations::unmapped()
                ),
                binding,
                completion,
                next,
            )?;
            return self.expression(&tokens[..tokens.len() - 2], Some(&value), "", finish);
        }
        if tokens.last()?.is("]") {
            let open = exponentiation::matching_open(tokens, tokens.len() - 1)?;
            if open > 0 {
                let object = self.temporary();
                let key = self.temporary();
                let finish =
                    self.completed_value(&format!("{object}[{key}]"), binding, completion, next)?;
                let read_key =
                    self.expression(&tokens[open + 1..tokens.len() - 1], Some(&key), "", finish)?;
                return self.expression(&tokens[..open], Some(&object), "", read_key);
            }
        }
        let &open = top.iter().rev().find(|&&at| {
            tokens[at].is("(")
                && at > 0
                && exponentiation::matching_close(tokens, at) == Some(tokens.len() - 1)
        })?;
        let arguments = &tokens[open + 1..tokens.len() - 1];
        let groups = split_arguments(arguments)?;
        let callee = self.temporary();
        let receiver = self.temporary();
        let values = (0..groups.len())
            .map(|_| self.temporary())
            .collect::<Vec<_>>();
        let finish = self.completed_value(
            &format!(
                "{}Function.prototype.apply.call({callee}, {receiver}, [{}]{}){}",
                relocations::unmapped(),
                values.join(", "),
                relocations::source_anchor(self.source_offset + tokens.last()?.start),
                relocations::source_anchor(self.source_offset + tokens.last()?.end)
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
                "{callee} = {}{}{}; {}",
                self.value_reference(&receiver, &tokens[..open - 2]),
                relocations::relocated_verbatim(
                    self.source_offset + tokens[open - 2].start,
                    self.source_offset + tokens[open - 1].end
                ),
                relocations::unmapped(),
                self.jump(entry)
            ));
            self.expression(&tokens[..open - 2], Some(&receiver), "", get)
        } else if tokens[open - 1].is("]") {
            let key_open = exponentiation::matching_open(tokens, open - 1)?;
            let key = self.temporary();
            let get = self.block(format!(
                "{callee} = {receiver}[{key}]; {}",
                self.jump(entry)
            ));
            let read_key = self.expression(&tokens[key_open + 1..open - 1], Some(&key), "", get)?;
            self.expression(&tokens[..key_open], Some(&receiver), "", read_key)
        } else {
            let get = self.block(format!("{receiver} = void 0; {}", self.jump(entry)));
            self.expression(&tokens[..open], Some(&callee), "", get)
        }
    }

    fn assignment(
        &mut self,
        tokens: &[Token],
        equal: usize,
        binding: Option<&str>,
        completion: &str,
        next: usize,
    ) -> Option<usize> {
        let left = &tokens[..equal];
        let value = self.temporary();
        let operator = &tokens[equal].text;
        let previous = (operator != "=").then(|| self.temporary());
        let (reference, object, key, object_tokens, key_tokens) =
            if left.len() == 1 && left[0].kind == TokenKind::Identifier {
                (left[0].text.clone(), None, None, None, None)
            } else if left.len() >= 3 && left[left.len() - 2].is(".") {
                let object = self.temporary();
                (
                    format!("{object}.{}", left.last()?.text),
                    Some(object),
                    None,
                    Some(&left[..left.len() - 2]),
                    None,
                )
            } else if left.last()?.is("]") {
                let open = exponentiation::matching_open(left, left.len() - 1)?;
                let object = self.temporary();
                let key = self.temporary();
                (
                    format!("{object}[{key}]"),
                    Some(object),
                    Some(key),
                    Some(&left[..open]),
                    Some(&left[open + 1..left.len() - 1]),
                )
            } else {
                return None;
            };
        let mapped_reference = if left.len() == 1 && left[0].kind == TokenKind::Identifier {
            relocations::relocated_verbatim(
                self.source_offset + left[0].start,
                self.source_offset + left[0].end,
            )
        } else if key.is_some() {
            format!(
                "{}{reference}{}",
                relocations::source_anchor(self.source_offset + left[0].start),
                relocations::source_anchor(self.source_offset + left.last()?.end)
            )
        } else {
            reference.clone()
        };
        let assigned = if let Some(previous) = &previous {
            format!(
                "{}({mapped_reference} = {}{} {}{}{} {}){}",
                relocations::unmapped(),
                relocations::unmapped(),
                self.value_reference(previous, left),
                relocations::source_anchor(self.source_offset + left[0].start),
                &operator[..operator.len() - 1],
                relocations::unmapped(),
                self.value_reference(&value, &tokens[equal + 1..]),
                relocations::unmapped()
            )
        } else {
            format!(
                "{}({mapped_reference} = {}{}){}",
                relocations::unmapped(),
                relocations::unmapped(),
                self.value_reference(&value, &tokens[equal + 1..]),
                relocations::unmapped()
            )
        };
        let finish = if binding.is_none() && completion.is_empty() {
            self.block(format!(
                "{assigned}{}{}",
                self.resume_terminator(tokens, true),
                self.jump(next)
            ))
        } else {
            self.completed_value(&assigned, binding, completion, next)?
        };
        let mut entry = self.expression(&tokens[equal + 1..], Some(&value), "", finish)?;
        if let Some(previous) = previous {
            entry = self.block(format!(
                "{previous} = {mapped_reference}{}; {}",
                relocations::unmapped(),
                self.jump(entry)
            ));
        }
        if let (Some(key), Some(tokens)) = (key, key_tokens) {
            entry = self.expression(tokens, Some(&key), "", entry)?;
        }
        if let (Some(object), Some(tokens)) = (object, object_tokens) {
            entry = self.expression(tokens, Some(&object), "", entry)?;
        }
        Some(entry)
    }
}

fn top_level(tokens: &[Token]) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        result.push(at);
        if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
            at = exponentiation::matching_close(tokens, at)?;
        }
        at += 1;
    }
    Some(result)
}
fn is_prefix(tokens: &[Token], at: usize) -> bool {
    matches!(tokens[at].text.as_str(), "+" | "-" | "*")
        && (at == 0
            || matches!(
                tokens[at - 1].text.as_str(),
                "(" | "["
                    | "{"
                    | "="
                    | ","
                    | ":"
                    | "?"
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "yield"
                    | "await"
            ))
}
fn split_arguments(tokens: &[Token]) -> Option<Vec<&[Token]>> {
    let mut result = Vec::new();
    let mut begin = 0;
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is(",") {
            result.push(&tokens[begin..at]);
            begin = at + 1;
        }
        if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
            at = exponentiation::matching_close(tokens, at)?;
        }
        at += 1;
    }
    if begin < tokens.len() {
        result.push(&tokens[begin..]);
    }
    Some(result)
}
