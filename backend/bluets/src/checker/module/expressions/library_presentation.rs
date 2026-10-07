// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned library signature text for rejected calls; checking keeps its own types.
use super::*;
use crate::diagnostic::{templates, type_text};

fn substitute(text: &str, substitutions: &BTreeMap<String, Type>) -> String {
    let text = text.replace("Awaited<T>", "T");
    let mut result = String::new();
    let mut word = String::new();
    for character in text.chars().chain(std::iter::once(' ')) {
        if character.is_alphanumeric() || character == '_' {
            word.push(character);
            continue;
        }
        if let Some(value) = substitutions.get(&word) {
            result.push_str(&type_text::render(value));
        } else {
            result.push_str(&word);
        }
        word.clear();
        result.push(character);
    }
    result.trim_end().into()
}
fn parse(text: &str) -> Type {
    let text = if let Some((parameters, _)) = text.split_once("=> value is ") {
        format!("{parameters}=> unknown")
    } else {
        text.into()
    };
    let text = text.replace("this: any, ", "");
    crate::parse_module(
        "<diagnostic-library-type>",
        format!("type Presentation = {text};"),
    )
    .ok()
    .and_then(|module| {
        module.declarations.into_iter().find_map(|item| match item {
            Declaration::TypeAlias(alias) => Some(alias.value),
            _ => None,
        })
    })
    .unwrap_or(Type::Any)
}
fn optional_target(value: &Type, actual: &Type) -> Type {
    if *actual == Type::Null {
        return value.clone();
    }
    if let Type::Union(values) = value {
        let mut values = values
            .iter()
            .filter(|value| !matches!(value, Type::Undefined | Type::Null))
            .cloned()
            .collect::<Vec<_>>();
        if values.len() == 1 {
            values.pop().unwrap()
        } else {
            Type::Union(values)
        }
    } else {
        value.clone()
    }
}
impl ModuleChecker<'_> {
    pub(super) fn present_library_argument(
        &mut self,
        key: &str,
        actual: &Type,
        expected: &Type,
        argument: &[Token],
        base: &Type,
        index: usize,
    ) {
        if !self.enforce_types {
            return;
        }
        let mut substitutions = BTreeMap::new();
        if let Type::Array(element) = base {
            substitutions.insert("T".into(), *element.clone());
        }
        if let Type::Named { name, arguments } = base {
            if let Some(value) = arguments.first() {
                substitutions.insert("T".into(), value.clone());
            }
            if let Some(value) = arguments.get(1) {
                for name in ["R", "TReturn"] {
                    substitutions.insert(name.into(), value.clone());
                }
            }
            if let Some(value) = arguments.get(2) {
                for name in ["N", "TNext"] {
                    substitutions.insert(name.into(), value.clone());
                }
            }
            if key.ends_with(".next")
                && matches!(name.as_str(), "Iterator" | "Generator" | "IterableIterator")
                && arguments.len() > 2
            {
                let literal = if let [token] = argument {
                    if matches!(token.kind, TokenKind::String | TokenKind::Number) {
                        Type::Literal(token.text.clone())
                    } else {
                        actual.clone()
                    }
                } else {
                    actual.clone()
                };
                let given = format!("[{}]", type_text::render_in(&literal, self.project));
                let expected = type_text::render_in(&arguments[2], self.project);
                let span = self
                    .diagnostics
                    .last()
                    .unwrap()
                    .typescript
                    .as_ref()
                    .unwrap()
                    .span
                    .clone();
                let mut counterpart = Diagnostic::error(DiagnosticCode::TypeMismatch, span, "")
                    .with_typescript(2345, vec![given.clone(), format!("[] | [{expected}]")])
                    .typescript
                    .unwrap();
                counterpart.message.push_str(&format!("\n  Type '{given}' is not assignable to type '[{expected}]'.\n    Type '{}' is not assignable to type '{expected}'.",type_text::argument(actual,&arguments[2])));
                self.diagnostics.last_mut().unwrap().typescript = Some(counterpart);
                return;
            }
        }
        if let Type::Function { result, .. } = actual {
            for name in ["U", "TResult1"] {
                substitutions.insert(name.into(), *result.clone());
            }
        }
        let pinned = templates::library_signatures(&format!("{}/{key}", self.target.as_str()))
            .and_then(|signatures| signatures.first())
            .and_then(|signature| signature.parameters.get(index));
        let expected = pinned
            .map(|parameter| {
                optional_target(
                    &parse(&substitute(&parameter.value, &substitutions)),
                    actual,
                )
            })
            .filter(|expected| *expected != Type::Any)
            .unwrap_or_else(|| expected.clone());
        let expected = if pinned.is_some_and(|parameter| parameter.rest) {
            match expected {
                Type::Array(value) => *value,
                _ => expected,
            }
        } else {
            expected
        };
        let actual_text = type_text::argument(actual, &expected);
        let span = self
            .diagnostics
            .last()
            .unwrap()
            .typescript
            .as_ref()
            .unwrap()
            .span
            .clone();
        let mut counterpart = Diagnostic::error(DiagnosticCode::TypeMismatch, span, "")
            .with_typescript(
                2345,
                vec![actual_text, type_text::render_in(&expected, self.project)],
            )
            .typescript
            .unwrap();
        counterpart
            .message
            .push_str(&type_text::detail(actual, &expected, self.project));
        self.diagnostics.last_mut().unwrap().typescript = Some(counterpart);
    }
    pub(in crate::checker::module) fn present_library_overloads(
        &mut self,
        key: &str,
        actuals: &[Type],
        substitutions: &BTreeMap<String, Type>,
    ) {
        if !self.enforce_types {
            return;
        }
        let Some(signatures) =
            templates::library_signatures(&format!("{}/{key}", self.target.as_str()))
        else {
            return;
        };
        let mut detail = String::new();
        let mut ordinal = 0;
        for signature in signatures {
            if actuals.len() < signature.minimum || actuals.len() > signature.parameters.len() {
                continue;
            }
            let mismatch=actuals.iter().zip(&signature.parameters).find_map(|(actual,parameter)|{
                let text=substitute(&parameter.value,substitutions);
                let value=parse(&text);
                let mut budget=TypeExpansionBudget::new(self.max_type_expansions);budget.checking=self.checking;
                if is_assignable(actual,&value,&self.types,&mut HashSet::new(),&mut budget){return None;}
                let primitive=matches!(actual,Type::String|Type::Number|Type::Boolean)||matches!(actual,Type::Literal(value) if value=="true"||value=="false");
                let target=if parameter.optional && primitive && matches!(value,Type::Union(ref values) if values.iter().all(|value|matches!(value,Type::String|Type::Number|Type::Boolean|Type::Undefined)) && values.iter().filter(|value|**value!=Type::Undefined).count()>1) {value.clone()}else if parameter.optional {optional_target(&value,actual)}else{value};
                let target_text=if text.contains("=> value is ") {text}else if text.contains("this: any") {if parameter.optional && *actual!=Type::Null {text.strip_prefix('(').unwrap_or(&text).strip_suffix(") | undefined").unwrap_or(&text).into()}else{text}}else{type_text::render_in(&target,self.project)};
                Some(format!("Argument of type '{}' is not assignable to parameter of type '{target_text}'.{}",type_text::argument(actual,&target),type_text::detail(actual,&target,self.project)))
            });
            let Some(mismatch) = mismatch else { continue };
            ordinal += 1;
            let parameters = signature
                .parameters
                .iter()
                .map(|parameter| {
                    let text = substitute(&parameter.value, substitutions);
                    let value = if text.contains("=> value is ") || text.contains("this: any") {
                        text
                    } else {
                        type_text::render_in(&parse(&text), self.project)
                    };
                    format!(
                        "{}{}{}: {value}",
                        if parameter.rest { "..." } else { "" },
                        parameter.name,
                        if parameter.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let result = substitute(&signature.result, substitutions);
            detail.push_str(&format!("\n  Overload {ordinal} of {}, '({parameters}): {result}', gave the following error.",signatures.len()));
            for line in mismatch.lines() {
                detail.push_str(&format!("\n    {line}"));
            }
        }
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
            .filter(|counterpart| counterpart.code == 2769)
        {
            counterpart.message = format!("No overload matches this call.{detail}");
        }
    }
}
