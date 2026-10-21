// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rejected nested protocols using the same bounded, directed relation.
//! This module never changes an acceptance verdict.
use super::*;

pub(super) struct StructuralReason {
    pub message: String,
    pub related: Option<crate::TypeScriptRelatedInformation>,
}

struct Missing {
    source: Type,
    target: Type,
    field: TypeField,
}

impl ModuleChecker<'_> {
    pub(super) fn nested_structural_reason(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<StructuralReason> {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        if matches!((actual,expected),(Type::Named{name:a,..},Type::Named{name:e,..}) if a == e) {
            let missing = self.nested_missing(actual, expected, &mut budget, 0)?;
            return Some(self.missing_reason(&missing, 1));
        }
        let a = self.compatibility_shape(actual);
        let e = self.compatibility_shape(expected);
        let (Type::Record(af), Type::Record(ef)) = (&a, &e) else {
            return None;
        };
        for target in ef {
            let source = af.iter().find(|f| f.name == target.name)?;
            if is_assignable(
                &source.value,
                &target.value,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                continue;
            }
            let (
                Type::GenericFunction {
                    type_parameters: ap,
                    parameters: av,
                    result: ar,
                    ..
                },
                Type::GenericFunction {
                    type_parameters: ep,
                    parameters: ev,
                    result: er,
                    ..
                },
            ) = (&source.value, &target.value)
            else {
                continue;
            };
            let text = |t: &Type| crate::diagnostic::type_text::render_in(t, self.project);
            // On a rejected generic signature, parameter constraints explain
            // the directed comparison before its unrelated return mismatch.
            for ((a, e), (at, et)) in av.iter().zip(ev).zip(ap.iter().zip(ep)) {
                if let (Some(ac), Some(ec)) = (&at.constraint, &et.constraint) {
                    if let Some(missing) = self.nested_missing(ec, ac, &mut budget, 0) {
                        let mut reason = self.missing_reason(&missing, 5);
                        reason.message = format!("\n  Types of property '{}' are incompatible.\n    Type '{}' is not assignable to type '{}'.\n      Types of parameters '{}' and '{}' are incompatible.\n        Type '{}' is not assignable to type '{}'.{}",
                            target.name,text(&source.value),text(&target.value),a.name,e.name,
                            e.annotation.as_ref().map(text)?,text(ac),reason.message);
                        return Some(reason);
                    }
                }
            }
            if let Type::Named { name, arguments } = er.as_ref() {
                if arguments.is_empty() {
                    if let Some(parameter) = ep.iter().find(|p| p.name == *name) {
                        if let Some(constraint) = &parameter.constraint {
                            if ar != er
                                && is_assignable(
                                    ar,
                                    constraint,
                                    &self.types,
                                    &mut HashSet::new(),
                                    &mut budget,
                                )
                            {
                                return Some(StructuralReason {message:format!(
                                    "\n  The types returned by '{}(...)' are incompatible between these types.\n    Type '{}' is not assignable to type '{}'.\n      '{}' is assignable to the constraint of type '{}', but '{}' could be instantiated with a different subtype of constraint '{}'.",
                                    target.name,text(ar),name,text(ar),name,name,text(constraint)),related:None});
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn nested_missing(
        &self,
        actual: &Type,
        expected: &Type,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<Missing> {
        if depth >= 32 || !budget.consume() {
            return None;
        }
        let a = crate::checker::type_operators::expanded(
            actual,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        let e = crate::checker::type_operators::expanded(
            expected,
            &self.types,
            &mut HashSet::new(),
            budget,
        );
        match (&a, &e) {
            (Type::Record(af), Type::Record(ef)) => {
                if let Some(field) = ef
                    .iter()
                    .find(|e| !e.optional && !af.iter().any(|a| a.name == e.name))
                {
                    return Some(Missing {
                        source: actual.clone(),
                        target: expected.clone(),
                        field: field.clone(),
                    });
                }
                for target in ef {
                    if let Some(source) = af.iter().find(|f| f.name == target.name) {
                        if !is_assignable(
                            &source.value,
                            &target.value,
                            &self.types,
                            &mut HashSet::new(),
                            budget,
                        ) {
                            if let Some(found) =
                                self.nested_missing(&source.value, &target.value, budget, depth + 1)
                            {
                                return Some(found);
                            }
                        }
                    }
                }
            }
            (
                Type::Function {
                    parameters: ap,
                    result: ar,
                }
                | Type::GenericFunction {
                    parameters: ap,
                    result: ar,
                    ..
                },
                Type::Function {
                    parameters: ep,
                    result: er,
                }
                | Type::GenericFunction {
                    parameters: ep,
                    result: er,
                    ..
                },
            ) => {
                for (a, e) in ap.iter().zip(ep) {
                    if let (Some(a), Some(e)) = (&a.annotation, &e.annotation) {
                        if !is_assignable(e, a, &self.types, &mut HashSet::new(), budget) {
                            if let Some(found) = self.nested_missing(e, a, budget, depth + 1) {
                                return Some(found);
                            }
                        }
                    }
                }
                if **er != Type::Void
                    && !is_assignable(ar, er, &self.types, &mut HashSet::new(), budget)
                {
                    return self.nested_missing(ar, er, budget, depth + 1);
                }
            }
            _ => {}
        }
        None
    }

    fn missing_reason(&self, missing: &Missing, depth: usize) -> StructuralReason {
        let field = &missing.field;
        let token = self
            .project
            .source(&field.span.module)
            .and_then(|source| crate::syntax::lex(&field.span.module, source).ok())
            .and_then(|tokens| {
                tokens.into_iter().find(|t| {
                    field.span.start <= t.start && t.end <= field.span.end && t.is(&field.name)
                })
            });
        StructuralReason {
            message: format!(
                "\n{}Property '{}' is missing in type '{}' but required in type '{}'.",
                "  ".repeat(depth),
                field.name,
                crate::diagnostic::type_text::render_in(&missing.source, self.project),
                crate::diagnostic::type_text::render_in(&missing.target, self.project)
            ),
            related: token.map(|token| crate::TypeScriptRelatedInformation {
                code: 2728,
                message: format!("'{}' is declared here.", field.name),
                span: token.span(&field.span.module),
                position: None,
            }),
        }
    }
}
