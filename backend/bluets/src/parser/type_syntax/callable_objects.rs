// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Object types preserve call/construct signatures alongside data members.

use super::*;

impl Parser {
    pub(in crate::parser) fn parse_record_type(&mut self, field_expectation: &str) -> Type {
        let mut fields = Vec::new();
        let mut signatures = Vec::new();
        let mut indices = Vec::new();
        let mut closed = false;
        while !self.at_eof() {
            if self.consume("}") {
                closed = true;
                break;
            }
            let start = self.current().start;
            let construct = self.peek("new")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| token.is("(") || token.is("<"));
            if construct || self.peek("(") || self.peek("<") {
                if construct {
                    self.bump();
                }
                let type_parameters = self.parse_type_parameters();
                let Type::Function { parameters, result } =
                    self.parse_method_signature(&[";", ",", "}"])
                else {
                    unreachable!()
                };
                signatures.push(TypeSignature {
                    construct,
                    type_parameters,
                    parameters,
                    result: *result,
                    span: SourceSpan::new(&self.id, start, self.previous().end),
                });
            } else {
                let readonly = self.consume("readonly");
                if self.consume("[") {
                    let name = self.require_identifier("expected an index parameter name");
                    self.expect(":");
                    let key_start = self.current().start;
                    let key = self.parse_type_until(&["]"]);
                    let key_span = SourceSpan::new(&self.id, key_start, self.previous().end);
                    self.expect("]");
                    self.expect(":");
                    let value = self.parse_type_until(&[";", ",", "}"]);
                    indices.push(IndexSignature {
                        name,
                        key,
                        value,
                        readonly,
                        key_span,
                        span: SourceSpan::new(&self.id, start, self.previous().end),
                    });
                    self.consume(";");
                    self.consume(",");
                    continue;
                }
                let name = self.require_property_name(field_expectation);
                let optional = self.consume("?");
                let value = if self.peek("(") || self.peek("<") {
                    let type_parameters = self.parse_type_parameters();
                    let function = self.parse_method_signature(&[";", ",", "}"]);
                    if type_parameters.is_empty() {
                        function
                    } else {
                        let Type::Function { parameters, result } = function else {
                            unreachable!()
                        };
                        Type::GenericFunction {
                            type_parameters,
                            parameters,
                            result,
                            span: SourceSpan::new(&self.id, start, self.previous().end),
                        }
                    }
                } else {
                    self.expect(":");
                    self.parse_type_until(&[";", ",", "}"])
                };
                fields.push(TypeField {
                    name,
                    readonly,
                    optional,
                    value,
                    span: SourceSpan::new(&self.id, start, self.previous().end),
                });
            }
            self.consume(";");
            self.consume(",");
        }
        if !closed {
            self.expect("}");
        }
        let object = if signatures.is_empty() {
            Type::Record(fields)
        } else {
            Type::CallableRecord { fields, signatures }
        };
        if indices.is_empty() {
            object
        } else {
            Type::IndexedRecord {
                object: Box::new(object),
                indices,
            }
        }
    }
}

impl TypeSignature {
    pub(crate) fn function_type(&self) -> Type {
        Type::GenericFunction {
            type_parameters: self.type_parameters.clone(),
            parameters: self.parameters.clone(),
            result: Box::new(self.result.clone()),
            span: self.span.clone(),
        }
    }

    pub(crate) fn map_types(&self, transform: impl FnOnce(&Type) -> Type) -> Self {
        let Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } = transform(&self.function_type())
        else {
            unreachable!("signature transformation preserves callable binders")
        };
        Self {
            type_parameters,
            parameters,
            result: *result,
            ..self.clone()
        }
    }
}
