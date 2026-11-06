// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inferred wrappers have no origin; retained parameter/type nodes still do.
use super::*;
use crate::parser::{NestedFunction, NestedFunctionBody, NestedFunctionKind, Type};

pub(super) fn member_origins(module: &Module) -> BTreeMap<(usize, usize), &ClassMemberShell> {
    let mut origins = BTreeMap::new();
    for member in module.classes().flat_map(|class| &class.members) {
        let names = member
            .field
            .as_ref()
            .map(|field| &field.name_span)
            .or_else(|| member.method.as_ref().map(|method| &method.name_span))
            .or_else(|| member.accessor.as_ref().map(|accessor| &accessor.name_span));
        for span in std::iter::once(&member.span).chain(names) {
            origins.insert((span.start, span.end), member);
        }
    }
    origins
}

impl Mapper<'_> {
    pub(super) fn inferred_type(&mut self, value: &Type, declaration_start: usize) {
        let origins = self.inferred;
        if let Some(original) = origins.get(&declaration_start) {
            self.structural_type(value, original);
        }
    }

    fn structural_type(&mut self, value: &Type, original: &Type) {
        if self.exhausted {
            return;
        }
        let (fields, signatures) = match value {
            Type::Record(fields) => (fields.as_slice(), &[][..]),
            Type::CallableRecord { fields, signatures } => {
                (fields.as_slice(), signatures.as_slice())
            }
            _ => return,
        };
        let (source_fields, source_signatures) = match original {
            Type::Record(fields) => (fields.as_slice(), &[][..]),
            Type::CallableRecord { fields, signatures } => {
                (fields.as_slice(), signatures.as_slice())
            }
            _ => return,
        };
        for field in fields {
            if self.exhausted {
                break;
            }
            let Some(origin) = source_fields
                .iter()
                .find(|source| source.name == field.name)
            else {
                continue;
            };
            if origin.span.module != self.source.id {
                continue;
            }
            let tokens = self.generated(&field.span);
            let members = &self.member_origins;
            if let Some(&member) = members
                .get(&(origin.span.start, origin.span.end))
                .filter(|member| member.name.as_deref() == Some(&field.name))
            {
                if let Some(method) = &member.method {
                    if let Type::Function { parameters, .. }
                    | Type::GenericFunction { parameters, .. } = &field.value
                    {
                        self.inferred_parameters(parameters, &method.parameters);
                        if let Some(span) = &method.return_type_span {
                            self.ty(annotation(&tokens), &self.source(span));
                        }
                    }
                }
                if let Some(original) = &member.field {
                    if let Some(span) = &original.annotation_span {
                        self.ty(annotation(&tokens), &self.source(span));
                    }
                }
                if let Some(accessor) = &member.accessor {
                    if let Some(span) = &accessor.return_type_span {
                        self.ty(annotation(&tokens), &self.source(span));
                    }
                }
            }
        }
        for (signature, source) in signatures.iter().zip(source_signatures) {
            if signature.construct == source.construct {
                self.structural_type(&signature.result, &source.result);
            }
        }
    }

    fn inferred_parameters(&mut self, generated: &[Parameter], source: &[Parameter]) {
        for (g, s) in generated.iter().zip(source) {
            if self.exhausted {
                break;
            }
            self.point(g.span.start, s.span.start);
            if s.annotation.is_some() {
                self.ty(
                    annotation(&self.generated(&g.span)),
                    annotation(&self.source(&s.span)),
                );
            }
        }
    }

    pub(super) fn inferred_function(
        &mut self,
        value: &Type,
        initializer: &[Token],
        generated: &[Token],
    ) {
        let (Type::Function { parameters, .. } | Type::GenericFunction { parameters, .. }) = value
        else {
            return;
        };
        let Some(first) = initializer.first() else {
            return;
        };
        let module = self.source;
        let Some(function) = module.nested_functions.get(&first.start) else {
            return;
        };
        self.inferred_parameters(parameters, &function.parameters);
        if function.return_type.is_some() {
            let source = self.function_annotation(function);
            if let Some(arrow) = top(generated, "=>") {
                let result = &generated[arrow + 1..];
                let end = top(result, ";").unwrap_or(result.len());
                self.ty(&result[..end], &source);
            }
        }
    }

    fn function_annotation(&self, function: &NestedFunction) -> Vec<Token> {
        let source = self.source(&function.span);
        let body = match &function.body {
            NestedFunctionBody::Expression(tokens) => tokens.first().map(|token| token.start),
            NestedFunctionBody::Block { .. } => {
                let mut depth = 0;
                source.iter().rev().find_map(|token| {
                    if token.is("}") {
                        depth += 1;
                    }
                    if token.is("{") {
                        depth -= 1;
                        if depth == 0 {
                            return Some(token.start);
                        }
                    }
                    None
                })
            }
        }
        .unwrap_or(function.span.end);
        let end = source.partition_point(|token| token.start < body);
        let header = &source[..end];
        let end = if function.kind == NestedFunctionKind::Arrow {
            header
                .iter()
                .rposition(|token| token.is("=>"))
                .unwrap_or(header.len())
        } else {
            header.len()
        };
        annotation(&header[..end]).to_vec()
    }
}
