// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original runtime nodes inside exactly replayed namespace and enum wrappers.

use super::*;
use crate::lowering::namespaces::{
    lower_enum, lower_enum_function, NamespaceEntryKind, NamespaceOrigins,
};
use blueice_bluets::{EnumDeclaration, EvaluatedEnum, NamespaceDeclaration};

fn iife_function(statement: &bluejs::Stmt) -> Result<&bluejs::Function, BridgeError> {
    let expression = match statement {
        bluejs::Stmt::Expr(expression) => expression,
        bluejs::Stmt::VarDecl(_, declarations) if declarations.len() == 1 => {
            declarations[0].init.as_ref().ok_or_else(invalid)?
        }
        _ => return Err(invalid()),
    };
    let bluejs::Expr::Call { callee, .. } = expression else {
        return Err(invalid());
    };
    let bluejs::Expr::Parenthesized(callee) = callee.as_ref() else {
        return Err(invalid());
    };
    let bluejs::Expr::Function(function) = callee.as_ref() else {
        return Err(invalid());
    };
    Ok(function)
}

impl Evidence<'_> {
    pub(super) fn namespace(
        &mut self,
        original: &NamespaceDeclaration,
        statements: &[bluejs::Stmt],
        origins: &NamespaceOrigins,
    ) {
        if self.error.is_some() {
            return;
        }
        let previous =
            std::mem::replace(&mut self.namespace_references, origins.references.clone());
        let result = self.namespace_body(original, statements, origins);
        self.namespace_references = previous;
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn namespace_body(
        &mut self,
        mut original: &NamespaceDeclaration,
        mut statements: &[bluejs::Stmt],
        origins: &NamespaceOrigins,
    ) -> Result<(), BridgeError> {
        for (depth, index) in origins.calls.iter().enumerate() {
            statements = &iife_function(statements.get(*index).ok_or_else(invalid)?)?.body;
            if depth + 1 < origins.calls.len() {
                let [Declaration::Namespace(inner)] = original.body.as_slice() else {
                    return Err(invalid());
                };
                if !inner.implicit {
                    return Err(invalid());
                }
                original = inner;
            }
        }
        if origins.calls.is_empty() {
            return Err(invalid());
        }
        let mut evaluations = blueice_bluets::evaluate_enums_in(&original.body).into_iter();
        let values = original
            .body
            .iter()
            .enumerate()
            .filter(|(_, declaration)| matches!(declaration, Declaration::Enum(_)))
            .map(|(index, _)| (index, evaluations.next().expect("every enum was evaluated")))
            .collect::<BTreeMap<_, _>>();
        for entry in &origins.entries {
            let declaration = original.body.get(entry.declaration).ok_or_else(invalid)?;
            let rewritten = blueice_bluets::rewrite_declaration(declaration, &origins.references)
                .map_err(lowering::from_diagnostic)?;
            let statement = statements.get(entry.statement).ok_or_else(invalid)?;
            match (&entry.kind, &rewritten) {
                (NamespaceEntryKind::Declaration, declaration) => {
                    self.declaration(declaration, statement)
                }
                (NamespaceEntryKind::ExportedInitializer, Declaration::Variable(variable)) => {
                    let bluejs::Stmt::Expr(bluejs::Expr::Assign { value, .. }) = statement else {
                        return Err(invalid());
                    };
                    self.expression(value, &variable.initializer);
                }
                (NamespaceEntryKind::Enum, Declaration::Enum(declaration)) => {
                    let function = iife_function(statement)?;
                    self.enum_function(declaration, &values[&entry.declaration], function)?;
                }
                (NamespaceEntryKind::Namespace(paths), Declaration::Namespace(inner)) => {
                    self.namespace(inner, &statements[entry.statement..], paths);
                }
                _ => return Err(invalid()),
            }
            if self.error.is_some() {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn enum_statement(
        &mut self,
        original: &EnumDeclaration,
        evaluation: &EvaluatedEnum,
        statement: &bluejs::Stmt,
    ) {
        if self.error.is_some() {
            return;
        }
        let result = (|| {
            let replayed = lower_enum(
                self.module,
                original,
                &mut vec![evaluation.clone()].into_iter(),
            )?;
            if !same_syntax(replayed.as_ref().ok_or_else(invalid)?, statement)? {
                return Err(invalid());
            }
            self.enum_function(original, evaluation, iife_function(statement)?)
        })();
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn enum_function(
        &mut self,
        original: &EnumDeclaration,
        evaluation: &EvaluatedEnum,
        function: &bluejs::Function,
    ) -> Result<(), BridgeError> {
        let replayed = lower_enum_function(
            self.module,
            original,
            &mut vec![evaluation.clone()].into_iter(),
        )?;
        if !same_syntax(replayed.as_ref().ok_or_else(invalid)?, function)?
            || function.body.len() != original.members.len() + 1
            || evaluation.members.len() != original.members.len()
        {
            return Err(invalid());
        }
        for ((member, evaluated), statement) in original
            .members
            .iter()
            .zip(&evaluation.members)
            .zip(&function.body)
        {
            // Constant-folded values have no original runtime evaluation.
            if let Some(value) = &evaluated.value {
                // Structural Debug comparison admits NaN; the generated
                // numeric value must also preserve its exact payload/sign.
                if let blueice_bluets::EnumValue::Number(expected) = value {
                    let bluejs::Expr::Number(actual) = enum_value(statement)? else {
                        return Err(invalid());
                    };
                    if actual.to_bits() != expected.to_bits() {
                        return Err(invalid());
                    }
                }
                continue;
            }
            let Some(tokens) = &member.initializer else {
                continue;
            };
            self.expression(enum_value(statement)?, tokens);
        }
        Ok(())
    }
}

fn enum_value(statement: &bluejs::Stmt) -> Result<&bluejs::Expr, BridgeError> {
    let bluejs::Stmt::Expr(bluejs::Expr::Assign { target, .. }) = statement else {
        return Err(invalid());
    };
    let bluejs::Expr::Member { property, .. } = target.as_ref() else {
        return Err(invalid());
    };
    let bluejs::Expr::Assign { value, .. } = property.as_ref() else {
        return Err(invalid());
    };
    Ok(value)
}
