// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical access for flow facts; textual names never identify a declaration.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BindingId(pub(super) ScopeId, pub(super) String);

impl ScopeModel<'_> {
    pub(super) fn flow_annotated(&self, id: &BindingId) -> bool {
        self.scopes[id.0]
            .values
            .get(&id.1)
            .is_some_and(|binding| binding.annotated)
    }
    pub(super) fn flow_shadowed(&self, id: &BindingId) -> bool {
        if id.1 == "this"
            || !self.scopes[id.0].values.get(&id.1).is_some_and(|binding| {
                matches!(binding.kind, BindingKind::Mutable | BindingKind::Const)
            })
        {
            return false;
        }
        let execution = self.scopes[id.0].execution;
        self.scopes
            .iter()
            .filter(|scope| scope.execution == execution && scope.values.contains_key(&id.1))
            .count()
            > 1
    }
    pub(super) fn flow_limit(&self, tokens: &[Token], reason: &str) {
        let mut failure = self.flow_failure.borrow_mut();
        if failure.is_none() {
            let span = match (tokens.first(), tokens.last()) {
                (Some(first), Some(last)) => {
                    SourceSpan::new(&self.module.id, first.start, last.end)
                }
                _ => self.scopes[0].span.clone(),
            };
            *failure = Some(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                span,
                reason,
            ));
        }
    }
    pub(in crate::checker) fn flow_initializer(
        &self,
        start: usize,
        annotation: Type,
    ) -> Option<VariableDeclaration> {
        let index = self.tokens.partition_point(|token| token.start < start);
        let keyword = self.tokens.get(index)?;
        let kind = match keyword.text.as_str() {
            "const" => VariableKind::Const,
            "let" => VariableKind::Let,
            "var" => VariableKind::Var,
            _ => return None,
        };
        let name = self.tokens.get(index + 1)?;
        if name.kind != TokenKind::Identifier {
            return None;
        }
        let mut end = index + 2;
        while end < self.tokens.len() && !matches!(self.tokens[end].text.as_str(), ";" | "}") {
            if matches!(self.tokens[end].text.as_str(), "(" | "[" | "{") {
                end = targets::close(&self.tokens, end).unwrap_or(end);
            }
            end += 1;
        }
        let tokens = &self.tokens[index + 2..end];
        let equal = targets::top_level(tokens, "=")?;
        Some(VariableDeclaration {
            name: name.text.clone(),
            kind,
            annotation: Some(annotation),
            initializer: tokens[equal + 1..].to_vec(),
            exported: false,
            declared: false,
            span: SourceSpan::new(
                &self.module.id,
                keyword.start,
                self.tokens.get(end).map_or(name.end, |token| token.end),
            ),
        })
    }
    pub(in crate::checker) fn flow_return_token(&self, start: usize) -> Option<Token> {
        let index = self.tokens.partition_point(|token| token.start < start);
        self.tokens
            .get(index.checked_sub(1)?)
            .filter(|token| token.is("return"))
            .cloned()
    }

    pub(in crate::checker) fn flow_execution(&self, offset: usize) -> usize {
        self.scopes[self.scope_at(offset)].execution
    }

    pub(super) fn flow_immediate(&self, creation: usize) -> bool {
        let Some(function) = self.module.nested_functions.get(&creation) else {
            return false;
        };
        let mut after = self
            .tokens
            .partition_point(|token| token.start < function.span.end);
        while self.tokens.get(after).is_some_and(|token| token.is(")")) {
            after += 1;
        }
        self.tokens.get(after).is_some_and(|token| token.is("("))
    }
    pub(super) fn flow_binding(&self, name: &str, offset: usize) -> Option<BindingId> {
        let (scope, binding) = self.resolve(self.scope_at(offset), name, Meaning::Value)?;
        (!binding?.type_only).then(|| BindingId(scope, name.into()))
    }

    pub(super) fn flow_declared(&self, id: &BindingId) -> Type {
        self.scopes[id.0]
            .values
            .get(&id.1)
            .or_else(|| {
                self.merged_members(id.0)
                    .and_then(|members| members.values.get(&id.1))
            })
            .map_or(Type::Unknown, |binding| binding.declared_type.clone())
    }

    pub(super) fn flow_values(&self, offset: usize) -> BTreeMap<String, Type> {
        let mut result = BTreeMap::new();
        let mut scope = Some(self.scope_at(offset));
        while let Some(id) = scope {
            for (name, binding) in &self.scopes[id].values {
                if !binding.type_only {
                    result
                        .entry(name.clone())
                        .or_insert_with(|| binding.declared_type.clone());
                }
            }
            scope = self.scopes[id].parent;
        }
        result
    }

    pub(super) fn flow_capture_allowed(
        &self,
        id: &BindingId,
        creation: usize,
        hoisted: bool,
    ) -> bool {
        let Some(binding) = self.scopes[id.0].values.get(&id.1) else {
            return false;
        };
        if binding.kind == BindingKind::Const {
            return true;
        }
        !hoisted
            && binding.kind == BindingKind::Mutable
            && !self.mutations.iter().any(|mutation| {
                self.flow_binding(&id.1, mutation.operator.start).as_ref() == Some(id)
                    && mutation.target.iter().any(|token| token.text == id.1)
                    && (mutation.operator.start >= creation
                        || self.scopes[mutation.scope].execution != self.scopes[id.0].execution)
            })
    }

    pub(super) fn flow_alias(&self, id: &BindingId) -> Option<Vec<Token>> {
        let binding = self.scopes[id.0].values.get(&id.1)?;
        if binding.kind != BindingKind::Const {
            return None;
        }
        let (start, end) = binding.initializer?;
        Some(self.tokens_in(&SourceSpan::new(&self.module.id, start, end)))
    }
}
