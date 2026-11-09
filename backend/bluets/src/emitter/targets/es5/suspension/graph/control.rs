// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loop and block ownership over retained parser spans.

use super::*;

impl Builder<'_> {
    fn tokens_at(&self, items: &[FunctionBodyItem], index: usize) -> Option<(Vec<Token>, usize)> {
        let offset = item_start(&items[index]);
        let tokens = self.tokens.clone();
        let start = tokens.iter().position(|token| token.start == offset)?;
        Some((tokens, start))
    }

    pub(super) fn control_end(
        &self,
        items: &[FunctionBodyItem],
        index: usize,
    ) -> Option<Option<usize>> {
        if !matches!(
            &items[index],
            FunctionBodyItem::Opaque(_) | FunctionBodyItem::Variable(_)
        ) {
            return Some(None);
        }
        let (tokens, start) = self.tokens_at(items, index)?;
        let end = match tokens[start].text.as_str() {
            "var" => statement_end(&tokens, start + 1)?,
            "{" => exponentiation::matching_close(&tokens, start)?,
            "for" if tokens.get(start + 1)?.is("(") => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                if !tokens.get(close + 1)?.is("{") {
                    return None;
                }
                exponentiation::matching_close(&tokens, close + 1)?
            }
            "switch" if tokens.get(start + 1)?.is("(") => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                if !tokens.get(close + 1)?.is("{") {
                    return None;
                }
                exponentiation::matching_close(&tokens, close + 1)?
            }
            "break" | "continue" if tokens.get(start + 1)?.is(";") => start + 1,
            "if" if tokens.get(start + 1)?.is("(") => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                if tokens.get(close + 1)?.is("{") {
                    return None;
                }
                (close + 1..tokens.len()).find(|&at| tokens[at].is(";"))?
            }
            _ => return None,
        };
        let end = tokens[end].end;
        let consumed = (index..items.len())
            .take_while(|&at| item_start(&items[at]) < end)
            .count();
        Some(Some(index + consumed))
    }

    pub(super) fn control(
        &mut self,
        items: &[FunctionBodyItem],
        index: usize,
        end: usize,
        next: usize,
    ) -> Option<usize> {
        let (tokens, start) = self.tokens_at(items, index)?;
        match tokens[start].text.as_str() {
            "var" => {
                let end = statement_end(&tokens, start + 1)?;
                self.declarators(&tokens[start + 1..end], next, true)
            }
            "{" => self.sequence(&items[index + 1..end - 1], next),
            "break" | "continue" => {
                let targets = self.loops.last()?;
                let (target, depth) = if tokens[start].is("break") {
                    targets.broken
                } else {
                    targets.continued?
                };
                let origin = relocations::source_anchor(self.source_offset + tokens[start].start);
                let end = tokens
                    .get(start + 1)
                    .filter(|token| token.is(";"))
                    .unwrap_or(&tokens[start])
                    .end;
                let end = relocations::source_anchor(self.source_offset + end);
                Some(self.block(format!(
                    "{origin}return {{kind:\"jump\", label:{target}, depth:{depth}}};{end} {}",
                    relocations::unmapped()
                )))
            }
            "switch" => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                let end = exponentiation::matching_close(&tokens, close + 1)?;
                let body = &tokens[start..=end];
                // An ordinary switch needs no continuation rewrite. Its own
                // unlabelled breaks remain local; returns and outer continues
                // still require the continuation compiler.
                let outer_jump = body.iter().enumerate().any(|(index, token)| {
                    matches!(
                        token.text.as_str(),
                        "return" | "var" | "let" | "const" | "class" | "function"
                    ) || token.is("continue")
                        || token.is("break")
                            && body.get(index + 1).is_some_and(|next| !next.is(";"))
                });
                if !outer_jump && !self.suspends(body) {
                    let end =
                        end + usize::from(tokens.get(end + 1).is_some_and(|token| token.is(";")));
                    let code = self.text(&tokens[start..=end])?;
                    Some(self.block(format!("{code} {}", self.jump(next))))
                } else {
                    self.switch(&tokens, start, next)
                }
            }
            "if" => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                let temporary = self.temporary();
                self.predicate_origin(&temporary, tokens[start].start, &tokens[start + 2..close]);
                let stop = statement_end(&tokens, close + 1)? + 1;
                let consequent = crate::parser::parse_emitted_statement(
                    self.module,
                    tokens.clone(),
                    close + 1,
                    stop,
                    self.depth,
                )?;
                let then = self.sequence(&consequent, next)?;
                let branch = self.block(format!(
                    "{}.label = {temporary} ? {then} : {next}; continue;",
                    self.context
                ));
                self.expression(&tokens[start + 2..close], Some(&temporary), "", branch)
            }
            "for" => {
                let close = exponentiation::matching_close(&tokens, start + 1)?;
                let mut separators = Vec::new();
                let mut at = start + 2;
                while at < close {
                    if tokens[at].is(";") {
                        separators.push(at);
                    }
                    if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
                        at = exponentiation::matching_close(&tokens, at)?;
                    }
                    at += 1;
                }
                let [first, second] = separators.as_slice() else {
                    return None;
                };
                let body = crate::parser::parse_emitted_block(
                    self.module,
                    tokens.clone(),
                    close + 1,
                    self.depth,
                )?;
                let entry = self.while_loop(
                    &tokens[first + 1..*second],
                    &body,
                    next,
                    Some(&tokens[second + 1..close]),
                )?;
                let initializer = &tokens[start + 2..*first];
                if initializer.is_empty() {
                    return Some(entry);
                }
                if initializer[0].is("var") {
                    self.declarators(&initializer[1..], entry, false)
                } else {
                    self.expression(initializer, None, "", entry)
                }
            }
            _ => None,
        }
    }

    fn declarators(&mut self, tokens: &[Token], mut next: usize, statement: bool) -> Option<usize> {
        let mut groups = Vec::new();
        let mut begin = 0;
        let mut at = 0;
        while at < tokens.len() {
            if tokens[at].is(",") {
                groups.push(&tokens[begin..at]);
                begin = at + 1;
            }
            if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
                at = exponentiation::matching_close(tokens, at)?;
            }
            at += 1;
        }
        groups.push(&tokens[begin..]);
        for group in groups.into_iter().rev() {
            let name = group.first()?;
            if name.kind != TokenKind::Identifier {
                return None;
            }
            self.locals.insert(name.text.clone());
            self.bindings
                .insert(name.text.clone(), (name.start, name.end));
            if group.len() > 1 {
                if !group[1].is("=") {
                    return None;
                }
                self.binding_initializers.insert(
                    name.text.clone(),
                    origins::InitializerOrigin {
                        start: group.get(2)?.start,
                        end: group.last()?.end,
                        statement,
                    },
                );
                next = self.expression(&group[2..], Some(&name.text), "", next)?;
            }
        }
        Some(next)
    }

    pub(super) fn while_loop(
        &mut self,
        test: &[Token],
        body: &[FunctionBodyItem],
        next: usize,
        update: Option<&[Token]>,
    ) -> Option<usize> {
        let head = self.block(String::new());
        let continued = if let Some(update) = update.filter(|tokens| !tokens.is_empty()) {
            self.expression(update, None, "", head)?
        } else {
            head
        };
        self.loops.push(JumpTargets {
            broken: (next, self.regions),
            continued: Some((continued, self.regions)),
        });
        let body = self.sequence(body, continued)?;
        self.loops.pop();
        let temporary = self.temporary();
        let exit = test.last().map_or_else(
            || relocations::unmapped().to_string(),
            |token| relocations::source_anchor(self.source_offset + token.end),
        );
        let branch = self.block(format!(
            "{}.label = {temporary} ? {body} : {exit}{next}{}; continue;",
            self.context,
            relocations::unmapped(),
        ));
        let entry = if test.is_empty() {
            body
        } else {
            self.expression(test, Some(&temporary), "", branch)?
        };
        self.cases[head] = self.jump(entry);
        Some(head)
    }
}

fn statement_end(tokens: &[Token], mut at: usize) -> Option<usize> {
    while at < tokens.len() {
        if tokens[at].is(";") {
            return Some(at);
        }
        if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
            at = exponentiation::matching_close(tokens, at)?;
        }
        at += 1;
    }
    None
}

fn item_start(item: &FunctionBodyItem) -> usize {
    match item {
        FunctionBodyItem::Variable(value) => value.span.start,
        FunctionBodyItem::Expression { span, .. }
        | FunctionBodyItem::Throw { span, .. }
        | FunctionBodyItem::Return { span, .. }
        | FunctionBodyItem::Opaque(span) => span.start,
        FunctionBodyItem::If(value) => value.span.start,
        FunctionBodyItem::While(value) => value.span.start,
        FunctionBodyItem::Try(value) => value.span.start,
        FunctionBodyItem::Function(value) => value.span.start,
        FunctionBodyItem::TypeAlias(value) => value.span.start,
    }
}
