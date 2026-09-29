// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first immutable local `typeof` guard recognized by function checking.

use super::*;
use crate::parser::VariableKind;

pub(super) fn selected_guard_local(items: &[FunctionBodyItem]) -> Option<&str> {
    let mut candidates = items.iter().filter_map(|item| {
        let FunctionBodyItem::Variable(variable) = item else {
            return None;
        };
        (variable.kind == VariableKind::Const
            && variable
                .annotation
                .as_ref()
                .is_some_and(is_string_number_union))
        .then_some(variable.name.as_str())
    });
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

pub(super) fn narrowed_guard_scopes(
    statement: &FunctionIfStatement,
    scope: &BTreeMap<String, Type>,
    selected_local: Option<&str>,
) -> Option<(BTreeMap<String, Type>, BTreeMap<String, Type>)> {
    let selected_local = selected_local?;
    if !matches!(
        statement.alternate,
        None | Some(FunctionElseBranch::Braced(_))
    ) {
        return None;
    }
    let [keyword, name, comparison, literal] = statement.test.as_slice() else {
        return None;
    };
    if !keyword.is("typeof")
        || name.text != selected_local
        || !matches!(comparison.text.as_str(), "===" | "!==")
        || !matches!(literal.text.as_str(), "'string'" | "\"string\"")
        || !scope
            .get(selected_local)
            .is_some_and(is_string_number_union)
    {
        return None;
    }
    let (then_type, else_type) = if comparison.is("===") {
        (Type::String, Type::Number)
    } else {
        (Type::Number, Type::String)
    };
    let mut then_scope = scope.clone();
    then_scope.insert(selected_local.to_string(), then_type);
    let mut else_scope = scope.clone();
    else_scope.insert(selected_local.to_string(), else_type);
    Some((then_scope, else_scope))
}

fn is_string_number_union(value: &Type) -> bool {
    matches!(value, Type::Union(options)
        if options.len() == 2
            && options.contains(&Type::String)
            && options.contains(&Type::Number))
}
