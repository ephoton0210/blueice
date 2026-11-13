// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Homomorphic reverse inference retains each property's or sequence slot's shape.
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
    let (actual, readonly) = match actual {
        Type::Readonly(value) => (value.as_ref(), true),
        value => (value, false),
    };
    let mut reverse = Reverser {
        mapped,
        name,
        aliases,
        budget,
    };
    let value = match actual {
        Type::Record(actuals) if !actuals.is_empty() => {
            let mut fields = Vec::new();
            for field in actuals {
                let object = Type::Record(vec![TypeField {
                    value: slot(),
                    ..field.clone()
                }]);
                let value = reverse.at(
                    Type::Literal(format!("{:?}", field.name)),
                    object,
                    &field.value,
                )?;
                fields.push(TypeField {
                    value,
                    ..field.clone()
                });
            }
            Type::Record(fields)
        }
        Type::Array(element) => Type::Array(Box::new(reverse.at(
            Type::Number,
            Type::Array(Box::new(slot())),
            element,
        )?)),
        Type::Tuple(elements) => {
            let mut values = Vec::new();
            for (index, element) in elements.iter().enumerate() {
                let actual = element.indexed_type();
                let value = reverse.at(
                    Type::Literal(format!("\"{index}\"")),
                    Type::Array(Box::new(slot())),
                    &actual,
                )?;
                values.push(TupleTypeElement {
                    annotation: if element.rest {
                        Type::Array(Box::new(value))
                    } else {
                        value
                    },
                    ..element.clone()
                });
            }
            Type::Tuple(values)
        }
        _ => return None,
    };
    Some((
        name.clone(),
        if readonly {
            Type::Readonly(Box::new(value))
        } else {
            value
        },
    ))
}

const SLOT: &str = "\0bluets:reverse-mapped-slot";

fn slot() -> Type {
    Type::Named {
        name: SLOT.into(),
        arguments: vec![],
    }
}

struct Reverser<'a> {
    mapped: &'a crate::parser::MappedType,
    name: &'a String,
    aliases: &'a BTreeMap<String, TypeDefinition>,
    budget: &'a mut TypeExpansionBudget,
}

impl Reverser<'_> {
    fn at(&mut self, key: Type, object: Type, actual: &Type) -> Option<Type> {
        if !self.budget.consume() {
            return None;
        }
        let locals = BTreeSet::from([SLOT.to_string()]);
        let substitutions = BTreeMap::from([
            (self.name.clone(), object),
            (self.mapped.parameter.name.clone(), key),
        ]);
        let template = substitute_type(&self.mapped.value, &substitutions);
        let mut collector = collect::Collector {
            parameters: &locals,
            candidates: BTreeMap::new(),
            aliases: self.aliases,
            budget: self.budget,
            active: HashSet::new(),
            dependencies: BTreeMap::new(),
            evidence: 0,
        };
        collector.collect(&template, actual, false);
        if collector.budget.exhausted {
            return None;
        }
        match collector.candidates.get(SLOT) {
            Some(candidates) => candidates.reverse(self.aliases, collector.budget),
            None => Some(Type::StrictUnknown),
        }
    }
}
