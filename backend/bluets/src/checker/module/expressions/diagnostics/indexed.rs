// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain a rejected wider indexed result through its rigid key parameter.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn keyof_constraint_detail(&self, actual: &Type, expected: &Type) -> Option<String> {
        let Type::KeyOf(object) = expected else {
            return None;
        };
        let Type::Named { name, .. } = &**object else {
            return None;
        };
        let parameter = self.bound_parameters.get(name)?;
        let constraint = parameter.constraint.as_ref()?;
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let shape = instantiate_named(
            constraint,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
            "diagnostic keyof constraint",
        )?;
        let Type::Mapped(mapped) = shape else {
            return None;
        };
        let domain = mapped.parameter.constraint.as_ref()?;
        Some(format!(
            "\n  Type '{}' is not assignable to type '{}'.",
            crate::diagnostic::type_text::argument(actual, domain),
            crate::diagnostic::type_text::render_in(domain, self.project)
        ))
    }

    pub(super) fn indexed_assignment_detail(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<String> {
        let actual = canonical_parameter_references(actual, &self.types);
        let expected = canonical_parameter_references(expected, &self.types);
        let (
            Type::IndexedAccess {
                object: source,
                index: actual,
                ..
            },
            Type::IndexedAccess {
                object: target,
                index: expected,
                ..
            },
        ) = (&actual, &expected)
        else {
            return None;
        };
        if type_identity(source) != type_identity(target) {
            return None;
        }
        let Type::Named { name, arguments } = &**expected else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let parameter = self.bound_parameters.get(name)?;
        let constraint =
            canonical_parameter_references(parameter.constraint.as_ref()?, &self.types);
        let Type::KeyOf(object) = &constraint else {
            return None;
        };
        if !matches!(&**object, Type::Named { name, arguments } if arguments.is_empty() && self.bound_parameters.contains_key(name))
            || type_identity(actual) != type_identity(&constraint)
        {
            return None;
        }
        // A constrained generic object can have additional keys beyond its
        // constraint's declared fields. The upper bound of keyof T is the
        // complete property-key domain, rather than keyof its constraint.
        let bound = Type::Union(vec![Type::String, Type::Number, Type::Symbol]);
        let text = |value: &Type| crate::diagnostic::type_text::render_in(value, self.project);
        let target = text(expected);
        let bound_text = text(&bound);
        let mut detail = String::new();
        for (depth, source) in [(1, &**actual), (3, &bound), (5, &Type::String)] {
            let source = text(source);
            detail.push_str(&format!(
                "\n{}Type '{source}' is not assignable to type '{target}'.",
                "  ".repeat(depth)
            ));
            detail.push_str(&format!("\n{}'{source}' is assignable to the constraint of type '{target}', but '{target}' could be instantiated with a different subtype of constraint '{bound_text}'.", "  ".repeat(depth + 1)));
        }
        Some(detail)
    }
}
