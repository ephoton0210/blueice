// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preserve outer generic arguments when a constructor escapes its lexical body.

use super::*;

type Frame = (SourceSpan, Vec<TypeParameter>);

impl Module {
    pub(in crate::parser) fn bind_class_expression_captures(&mut self) {
        let mut frames = Vec::new();
        declaration_frames(&self.declarations, &mut frames);
        for expression in self.class_expressions.values() {
            class_frames(&expression.class, &mut frames);
        }
        for function in self.nested_functions.values() {
            frames.push((function.span.clone(), function.type_parameters.clone()));
            if let NestedFunctionBody::Block { items, .. } = &function.body {
                body_frames(items, &mut frames);
            }
        }
        // Apply inner binders last, including methods that shadow a class binder.
        frames.sort_by_key(|(span, _)| (span.start, std::cmp::Reverse(span.end)));
        for expression in self.class_expressions.values_mut() {
            let class = &mut expression.class;
            let mut captured = BTreeMap::new();
            for (span, parameters) in &frames {
                if span.start < class.span.start && span.end >= class.span.end {
                    for parameter in parameters {
                        captured.insert(parameter.name.clone(), parameter.clone());
                    }
                }
            }
            for parameter in &class.type_parameters {
                captured.remove(&parameter.name);
            }
            class.captured_type_parameters = captured.into_values().collect();
        }
    }
}

fn declaration_frames(declarations: &[Declaration], frames: &mut Vec<Frame>) {
    for declaration in declarations {
        match declaration {
            Declaration::Function(function) => function_frames(function, frames),
            Declaration::Class(class) => class_frames(class, frames),
            Declaration::Namespace(namespace) => declaration_frames(&namespace.body, frames),
            _ => {}
        }
    }
}

fn function_frames(function: &FunctionDeclaration, frames: &mut Vec<Frame>) {
    frames.push((function.span.clone(), function.type_parameters.clone()));
    body_frames(&function.body, frames);
}

fn class_frames(class: &ClassDeclaration, frames: &mut Vec<Frame>) {
    frames.push((class.span.clone(), class.type_parameters.clone()));
    for member in &class.members {
        if let Some(method) = &member.method {
            frames.push((method.span.clone(), method.type_parameters.clone()));
            if let Some(body) = &method.body {
                body_frames(body, frames);
            }
        } else if let Some(constructor) = &member.constructor {
            if let Some(body) = &constructor.body {
                body_frames(body, frames);
            }
        } else if let Some(accessor) = &member.accessor {
            body_frames(&accessor.body, frames);
        } else if let Some(block) = &member.static_block {
            body_frames(&block.body, frames);
        }
    }
}

fn if_frames(statement: &FunctionIfStatement, frames: &mut Vec<Frame>) {
    body_frames(&statement.consequent, frames);
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(body)) => body_frames(body, frames),
        Some(FunctionElseBranch::ElseIf(statement)) => if_frames(statement, frames),
        None => {}
    }
}

fn body_frames(items: &[FunctionBodyItem], frames: &mut Vec<Frame>) {
    for item in items {
        match item {
            FunctionBodyItem::Function(function) => function_frames(function, frames),
            FunctionBodyItem::If(statement) => if_frames(statement, frames),
            FunctionBodyItem::While(statement) => body_frames(&statement.body, frames),
            FunctionBodyItem::Try(statement) => {
                body_frames(&statement.block, frames);
                if let Some(handler) = &statement.handler {
                    body_frames(&handler.body, frames);
                }
                if let Some(body) = &statement.finalizer {
                    body_frames(body, frames);
                }
            }
            _ => {}
        }
    }
}
