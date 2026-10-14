// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Effects use the lexically resolved declaration, parameter index and receiver.

use super::*;
use crate::parser::TypePredicate;

struct Call<'a> {
    target: &'a [Token],
    arguments: Vec<&'a [Token]>,
    parameters: Vec<Parameter>,
    result: Type,
}

fn resolve<'a>(scopes: &ScopeModel<'_>, state: &State, tokens: &'a [Token]) -> Option<Call<'a>> {
    let tokens = super::super::targets::strip(tokens);
    let open = super::super::targets::top_level(tokens, "(")?;
    if super::super::targets::close(tokens, open)? != tokens.len() - 1 {
        return None;
    }
    let target = &tokens[..open];
    let (id, path) = predicates::reference(scopes, target, 0)?;
    let declared = current(scopes, state, &id);
    let function = predicates::expand(scopes, &properties::project(scopes, &declared, &path)?);
    let Type::Function { parameters, result } = function else {
        return None;
    };
    let mut rest = &tokens[open + 1..tokens.len() - 1];
    let mut arguments = Vec::new();
    while !rest.is_empty() {
        let end = super::super::targets::top_level(rest, ",").unwrap_or(rest.len());
        arguments.push(&rest[..end]);
        rest = rest.get(end + 1..).unwrap_or(&[]);
    }
    Some(Call {
        target,
        arguments,
        parameters,
        result: *result,
    })
}

fn subject<'a>(call: &'a Call<'a>, predicate: &TypePredicate) -> Option<&'a [Token]> {
    if predicate.parameter == "this" {
        return call.target.get(..call.target.len().checked_sub(2)?);
    }
    let index = call
        .parameters
        .iter()
        .position(|parameter| parameter.name == predicate.parameter)?;
    call.arguments.get(index).copied()
}

pub(super) fn predicate(
    scopes: &ScopeModel<'_>,
    state: &State,
    tokens: &[Token],
    positive: bool,
) -> Option<State> {
    let call = resolve(scopes, state, tokens)?;
    let Type::Predicate(predicate) = &call.result else {
        return None;
    };
    if predicate.asserts {
        return None;
    }
    apply(scopes, state, &call, predicate, positive)
}

fn apply(
    scopes: &ScopeModel<'_>,
    state: &State,
    call: &Call<'_>,
    predicate: &TypePredicate,
    positive: bool,
) -> Option<State> {
    let subject = subject(call, predicate)?;
    let Some(target) = &predicate.target else {
        return predicates::narrow(scopes, state, subject, positive);
    };
    let (id, path) = predicates::reference(scopes, subject, 0)?;
    let actual = properties::read(scopes, state, &id, &path)?;
    let substitutions = BTreeMap::from([("this".into(), actual)]);
    let target = substitute_type(target, &substitutions);
    let mut next = predicates::filter(scopes, state, id.clone(), &path, |value| {
        narrow_type(scopes, value, &target, positive)
    })?;
    if positive && path.is_empty() && predicate.parameter == "this" {
        record_properties(scopes, &mut next, &id, &target, &[], 0);
    }
    Some(next)
}

fn record_properties(
    scopes: &ScopeModel<'_>,
    state: &mut State,
    id: &BindingId,
    target: &Type,
    path: &[(String, bool)],
    depth: usize,
) {
    if depth > scopes.max_type_expansions.max(32) {
        scopes.flow_limit(
            &[],
            "predicate properties exceed their bounded nesting limit",
        );
        return;
    }
    match target {
        Type::Intersection(parts) => {
            for part in parts {
                record_properties(scopes, state, id, part, path, depth + 1);
            }
        }
        Type::Record(fields) => {
            for field in fields {
                let mut child = path.to_vec();
                child.push((field.name.clone(), false));
                if properties::project(scopes, &scopes.flow_declared(id), &child).is_some() {
                    state
                        .properties
                        .insert((id.clone(), properties::names(&child)), field.value.clone());
                    record_properties(scopes, state, id, &field.value, &child, depth + 1);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn check_cases(
    scopes: &ScopeModel<'_>,
    model: &mut FlowModel,
    graph: &graph::Graph,
    infer: &impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
) {
    for (selector, case) in &graph.comparisons {
        let (Some(first), Some(last), Some(subject)) =
            (case.first(), case.last(), selector.first())
        else {
            continue;
        };
        let state = model.facts.get(&subject.start).cloned().unwrap_or_default();
        let scope = values(scopes, &state, subject.start);
        let expected = infer(selector, &scope, &Type::Unknown);
        let mut actual = match case.as_slice() {
            [token]
                if matches!(token.kind, TokenKind::String | TokenKind::Number)
                    || token.is("true")
                    || token.is("false") =>
            {
                Type::Literal(token.text.clone())
            }
            _ => infer(case, &scope, &expected),
        };
        if !matches!(predicates::expand(scopes, &expected), Type::Union(_)) {
            if let Type::Literal(text) = &actual {
                actual = literal_primitive(text);
            }
        }
        if matches!(actual, Type::Unknown | Type::Any)
            || matches!(expected, Type::Unknown | Type::Any)
            || assignable(scopes, &actual, &expected)
            || assignable(scopes, &expected, &actual)
        {
            continue;
        }
        let span = SourceSpan::new(&scopes.module.id, first.start, last.end);
        if model
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span == span)
        {
            continue;
        }
        model.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                span,
                "switch case is not comparable to its selector",
            )
            .with_typescript(
                2678,
                vec![
                    crate::diagnostic::type_text::render_in(&actual, scopes.project),
                    crate::diagnostic::type_text::render_in(&expected, scopes.project),
                ],
            ),
        );
    }
}

fn assignable(scopes: &ScopeModel<'_>, actual: &Type, expected: &Type) -> bool {
    let mut budget = TypeExpansionBudget::new(scopes.max_type_expansions);
    let result = is_assignable(
        actual,
        expected,
        &scopes.type_definitions,
        &mut HashSet::new(),
        &mut budget,
    );
    if budget.exhausted {
        scopes.flow_limit(&[], "call predicate exceeds its generic-expansion limit");
        return false;
    }
    result
}

fn narrow_type(
    scopes: &ScopeModel<'_>,
    value: &Type,
    target: &Type,
    positive: bool,
) -> Option<Type> {
    if matches!(value, Type::Any | Type::Unknown) {
        return Some(if positive {
            target.clone()
        } else {
            value.clone()
        });
    }
    let contained = assignable(scopes, value, target);
    if !positive {
        return (!contained).then(|| value.clone());
    }
    if contained {
        return Some(value.clone());
    }
    if assignable(scopes, target, value) {
        return Some(target.clone());
    }
    if let Type::Intersection(parts) = target {
        if parts.iter().any(|part| assignable(scopes, value, part)) {
            return Some(target.clone());
        }
    }
    None
}

fn explicit_target(scopes: &ScopeModel<'_>, target: &[Token]) -> bool {
    let Some(first) = target.first() else {
        return false;
    };
    if first.is("this") {
        return true;
    }
    let Some(id) = scopes.flow_binding(&first.text, first.start) else {
        return false;
    };
    let Some(binding) = scopes.scopes[id.0].values.get(&id.1) else {
        return false;
    };
    binding.annotated
        || matches!(
            binding.kind,
            BindingKind::Function | BindingKind::Class | BindingKind::Namespace
        )
        || imported_function(scopes, first)
}

fn imported_function(scopes: &ScopeModel<'_>, name: &Token) -> bool {
    scopes.module.declarations.iter().any(|declaration| {
        let Declaration::Import(import) = declaration else {
            return false;
        };
        import
            .bindings
            .iter()
            .filter(|binding| binding.local == name.text)
            .any(|binding| {
                scopes
                    .project
                    .resolved_import(&scopes.module.id, import)
                    .and_then(|id| scopes.project.modules.get(id))
                    .is_some_and(|module| {
                        module
                            .declarations
                            .iter()
                            .any(|declaration| match declaration {
                                Declaration::Function(function) => {
                                    function.name == binding.imported
                                }
                                Declaration::Variable(variable) => {
                                    variable.name == binding.imported
                                        && variable.annotation.is_some()
                                }
                                _ => false,
                            })
                    })
            })
    })
}

fn annotation_error(scopes: &ScopeModel<'_>, model: &mut FlowModel, target: &[Token]) {
    let (Some(first), Some(last)) = (target.first(), target.last()) else {
        return;
    };
    let span = SourceSpan::new(&scopes.module.id, first.start, last.end);
    if model.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .typescript
            .as_ref()
            .is_some_and(|ts| ts.span == span)
            && diagnostic
                .typescript
                .as_ref()
                .is_some_and(|ts| ts.code == 2775)
    }) {
        return;
    }
    let mut diagnostic = Diagnostic::error(
        DiagnosticCode::TypeMismatch,
        scopes.scopes[scopes.flow_execution(first.start)]
            .span
            .clone(),
        "assertion call target requires an explicit type annotation",
    )
    .with_typescript(2775, Vec::new());
    if let Some(ts) = diagnostic.typescript.as_mut() {
        ts.span = span;
    }
    if let Some(id) = scopes.flow_binding(&first.text, first.start) {
        if let Some(name) = scopes
            .declaration_names
            .iter()
            .filter_map(|offset| scopes.tokens.iter().find(|token| token.start == *offset))
            .find(|token| {
                token.text == first.text
                    && scopes.flow_binding(&token.text, token.start).as_ref() == Some(&id)
            })
        {
            let hint = Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                name.span(&scopes.module.id),
                "",
            )
            .with_typescript(2782, vec![name.text.clone()]);
            if let (Some(ts), Some(hint)) = (diagnostic.typescript.as_mut(), hint.typescript) {
                ts.related_information
                    .push(crate::TypeScriptRelatedInformation {
                        code: hint.code,
                        message: hint.message,
                        span: hint.span.clone(),
                        position: crate::diagnostic::positions::from_source(
                            &scopes.module.source,
                            &hint.span,
                        ),
                    });
            }
        }
    }
    model.diagnostics.push(diagnostic);
}

pub(super) fn effect(
    scopes: &ScopeModel<'_>,
    model: &mut FlowModel,
    tokens: &[Token],
    state: &State,
) -> Option<State> {
    let call = resolve(scopes, state, tokens)?;
    match &call.result {
        Type::Predicate(predicate) if predicate.asserts => {
            if !explicit_target(scopes, call.target) {
                annotation_error(scopes, model, call.target);
                return Some(state.clone());
            }
            apply(scopes, state, &call, predicate, true)
        }
        Type::Never if explicit_target(scopes, call.target) => {
            let mut next = state.clone();
            next.reachable = false;
            Some(next)
        }
        _ => None,
    }
}
