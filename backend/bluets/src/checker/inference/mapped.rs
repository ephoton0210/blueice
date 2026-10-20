// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Homomorphic mapped inputs infer one owned property at a time.
use super::*;

pub(super) fn infer(
    mapped: &crate::parser::MappedType,
    actual: &Type,
    parameters: &BTreeSet<String>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<(String, Type)> {
    let Type::KeyOf(object) = mapped.parameter.constraint.as_ref()? else {
        return None;
    };
    let Type::Named { name, arguments } = object.as_ref() else {
        return None;
    };
    if !arguments.is_empty() || !parameters.contains(name) || mapped.name_type.is_some() {
        return None;
    }
    let Type::Record(actuals) = actual else {
        return None;
    };
    let slot = "\0bluets:reverse-mapped-slot";
    let locals = BTreeSet::from([slot.to_string()]);
    let mut fields = Vec::new();
    for field in actuals {
        if !budget.consume() {
            return None;
        }
        let member = TypeField {
            value: Type::Named {
                name: slot.into(),
                arguments: vec![],
            },
            ..field.clone()
        };
        let substitutions = BTreeMap::from([
            (name.clone(), Type::Record(vec![member])),
            (
                mapped.parameter.name.clone(),
                Type::Literal(format!("{:?}", field.name)),
            ),
        ]);
        let template = substitute_type(&mapped.value, &substitutions);
        let mut collector = collect::Collector {
            parameters: &locals,
            candidates: BTreeMap::new(),
            aliases,
            budget,
            active: HashSet::new(),
        };
        collector.collect(&template, &field.value, false);
        let value = collector
            .candidates
            .get(slot)?
            .solve(false, aliases, collector.budget)?;
        fields.push(TypeField {
            value,
            ..field.clone()
        });
    }
    Some((name.clone(), Type::Record(fields)))
}
