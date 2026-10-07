// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration diagnostics distinguish invalid projections from assignments.
use super::*;
use crate::checker::type_operators;

impl ModuleChecker<'_> {
    pub(super) fn check_operator(&mut self, value: &Type) {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        match value {
            Type::Mapped(value) => {
                let Some(constraint) = &value.parameter.constraint else {
                    return;
                };
                let key =
                    type_operators::expanded(constraint, &self.types, &mut visited, &mut budget);
                if matches!(
                    key,
                    Type::Boolean
                        | Type::Record(_)
                        | Type::Array(_)
                        | Type::Unknown
                        | Type::Null
                        | Type::Undefined
                ) {
                    let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source)
                    else {
                        return;
                    };
                    let tokens = tokens
                        .iter()
                        .filter(|token| {
                            value.parameter.span.start <= token.start
                                && token.end <= value.parameter.span.end
                        })
                        .collect::<Vec<_>>();
                    let Some(start) = tokens
                        .iter()
                        .position(|token| token.is("in"))
                        .and_then(|index| tokens.get(index + 1))
                    else {
                        return;
                    };
                    let span =
                        SourceSpan::new(&self.module.id, start.start, value.parameter.span.end);
                    self.operator_error(
                        span,
                        2322,
                        vec![
                            crate::diagnostic::type_text::render(&key),
                            "string | number | symbol".into(),
                        ],
                        "mapped type keys must be property keys",
                    );
                }
            }
            Type::IndexedAccess {
                object,
                index,
                index_span,
            } => {
                let object =
                    type_operators::expanded(object, &self.types, &mut visited, &mut budget);
                let index =
                    type_operators::expanded(index, &self.types, &mut HashSet::new(), &mut budget);
                let Type::Literal(key) = &index else {
                    return;
                };
                let key = key.trim_matches(['\'', '"']);
                match &object {
                    Type::Record(fields) | Type::CallableRecord { fields, .. }
                        if !fields.iter().any(|field| field.name == key) =>
                    {
                        self.operator_error(
                            index_span.clone(),
                            2339,
                            vec![key.into(), crate::diagnostic::type_text::render(&object)],
                            "indexed type refers to a missing property",
                        );
                    }
                    Type::Tuple(items)
                        if key.parse::<usize>().is_ok_and(|index| index >= items.len()) =>
                    {
                        self.operator_error(
                            index_span.clone(),
                            2493,
                            vec![
                                crate::diagnostic::type_text::render(&object),
                                items.len().to_string(),
                                key.into(),
                            ],
                            "indexed type exceeds the tuple length",
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn operator_error(
        &mut self,
        span: SourceSpan,
        code: u32,
        arguments: Vec<String>,
        message: &str,
    ) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticCode::TypeMismatch, span, message)
                .with_typescript(code, arguments)
                .with_source_position(&self.module.source),
        );
    }

    pub(in crate::checker::module) fn check_alias_cycles(&mut self) {
        fn cycle(
            value: &Type,
            root: &str,
            types: &BTreeMap<String, TypeDefinition>,
            active: &mut HashSet<String>,
            budget: &mut TypeExpansionBudget,
        ) -> bool {
            if !budget.consume() {
                return false;
            }
            match value {
                Type::Named { name, .. } => {
                    if name == root {
                        return true;
                    }
                    if !active.insert(name.clone()) {
                        return false;
                    }
                    let result = types.get(name).is_some_and(|definition| {
                        cycle(&definition.value, root, types, active, budget)
                    });
                    active.remove(name);
                    result
                }
                Type::Union(parts) | Type::Intersection(parts) => parts
                    .iter()
                    .any(|part| cycle(part, root, types, active, budget)),
                Type::KeyOf(value) => cycle(value, root, types, active, budget),
                Type::IndexedAccess { object, .. } => cycle(object, root, types, active, budget),
                _ => false,
            }
        }
        for declaration in &self.module.declarations {
            let Declaration::TypeAlias(alias) = declaration else {
                continue;
            };
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            if !cycle(
                &alias.value,
                &alias.name,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                continue;
            }
            let Ok(tokens) = crate::syntax::lex(&self.module.id, &self.module.source) else {
                continue;
            };
            if let Some(token) = tokens.iter().find(|token| {
                alias.span.start <= token.start
                    && token.end <= alias.span.end
                    && token.is(&alias.name)
            }) {
                self.operator_error(
                    token.span(&self.module.id),
                    2456,
                    vec![alias.name.clone()],
                    "type alias has an unguarded circular reference",
                );
            }
        }
    }
}
