// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration helper use is independent of printing and output options.
use super::*;

impl Requirements<'_> {
    pub(super) fn declarations(&mut self) {
        for (declaration, _) in crate::emitter::runtime_declarations(&self.module.declarations) {
            match declaration {
                Declaration::Class(class) => self.class(class),
                Declaration::Function(function) if !function.declared && !function.overload => {
                    let span = self
                        .tokens
                        .iter()
                        .find(|token| {
                            token.start >= function.span.start
                                && token.end <= function.body_open.unwrap_or(function.span.end)
                                && token.text == function.name
                        })
                        .map_or_else(
                            || function.span.clone(),
                            |token| token.span(&self.module.id),
                        );
                    self.function(function.async_function, function.generator, span);
                    for variable in &function.locals {
                        self.variable(variable);
                    }
                }
                Declaration::Variable(variable) => self.variable(variable),
                Declaration::Import(import)
                    if !import.is_type_only()
                        && !import.equals_require
                        && self.options.es_module_interop
                        && matches!(
                            self.options.module_kind,
                            ModuleKind::CommonJs | ModuleKind::Amd | ModuleKind::Umd
                        )
                        && import
                            .bindings
                            .iter()
                            .any(|binding| !binding.type_only && binding.imported == "*") =>
                {
                    self.add("__importStar", import.span.clone());
                }
                Declaration::ValueExport(export)
                    if export.star
                        && export.namespace.is_none()
                        && !export.is_type_only()
                        && export.specifier.is_some()
                        && matches!(
                            self.options.module_kind,
                            ModuleKind::CommonJs | ModuleKind::Amd | ModuleKind::Umd
                        ) =>
                {
                    self.add("__exportStar", export.span.clone());
                }
                _ => {}
            }
        }
        for expression in self.module.class_expressions.values() {
            self.class(&expression.class);
        }
        for function in self.module.nested_functions.values() {
            self.function(
                function.async_function,
                function.generator,
                function.span.clone(),
            );
        }
    }

    fn class(&mut self, class: &ClassDeclaration) {
        if self.options.target < crate::EcmaTarget::Es2015 {
            if let Some(base) = &class.extends_span {
                let start = self
                    .tokens
                    .iter()
                    .rev()
                    .find(|token| {
                        token.is("extends")
                            && token.start >= class.name_span.end
                            && token.end <= base.start
                    })
                    .map_or(base.start, |token| token.start);
                self.add(
                    "__extends",
                    SourceSpan::new(&self.module.id, start, base.end),
                );
            }
        }
        for decorator in &class.decorators {
            self.decorator(&decorator.span);
        }
        for member in &class.members {
            for decorator in &member.decorators {
                self.decorator(&decorator.span);
            }
            if let Some(method) = &member.method {
                if method.body.is_some() {
                    self.function(
                        method.async_function,
                        method.generator,
                        method.name_span.clone(),
                    );
                }
                for parameter in &method.parameters {
                    for decorator in &parameter.decorators {
                        self.add("__param", decorator.span.clone());
                        self.decorator(&decorator.span);
                    }
                }
            }
            if let Some(constructor) = &member.constructor {
                for parameter in &constructor.parameters {
                    for decorator in &parameter.decorators {
                        self.add("__param", decorator.span.clone());
                        self.decorator(&decorator.span);
                    }
                }
            }
        }
    }

    fn decorator(&mut self, span: &SourceSpan) {
        if self.options.experimental_decorators {
            self.add("__decorate", span.clone());
            if self.options.emit_decorator_metadata {
                self.add("__metadata", span.clone());
            }
        } else if self.options.target < crate::EcmaTarget::EsNext {
            self.add("__esDecorate", span.clone());
            self.add("__runInitializers", span.clone());
        }
    }
}
