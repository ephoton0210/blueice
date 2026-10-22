// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic availability and namespace names from the static declaration set.
use super::*;
use crate::{Declaration, Type};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Catalog {
    globals: BTreeMap<String, EcmaTarget>,
    members: BTreeMap<(String, String), EcmaTarget>,
    names: BTreeSet<String>,
    aliases: BTreeMap<String, (Vec<crate::TypeParameter>, Type)>,
}

fn catalog() -> &'static Catalog {
    static CACHE: OnceLock<Catalog> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut result = Catalog::default();
        for edition in [
            EcmaTarget::Es5,
            EcmaTarget::Es2015,
            EcmaTarget::Es2016,
            EcmaTarget::Es2017,
            EcmaTarget::Es2018,
            EcmaTarget::Es2019,
            EcmaTarget::Es2020,
            EcmaTarget::Es2021,
            EcmaTarget::Es2022,
        ] {
            for module in modules(edition) {
                visit(&module.declarations, "", edition, &mut result);
            }
        }
        result
    })
}

fn visit(items: &[Declaration], prefix: &str, edition: EcmaTarget, catalog: &mut Catalog) {
    for item in items {
        match item {
            Declaration::Variable(value) if prefix.is_empty() => {
                catalog.globals.entry(value.name.clone()).or_insert(edition);
            }
            Declaration::Function(value) if prefix.is_empty() => {
                catalog.globals.entry(value.name.clone()).or_insert(edition);
            }
            Declaration::Interface(value) => {
                let name = format!("{prefix}{}", value.name);
                if !prefix.is_empty() {
                    catalog.names.insert(name.clone());
                }
                for field in &value.fields {
                    catalog
                        .members
                        .entry((name.clone(), field.name.clone()))
                        .or_insert(edition);
                }
            }
            Declaration::TypeAlias(value) => {
                catalog.aliases.insert(
                    format!("{prefix}{}", value.name),
                    (value.type_parameters.clone(), value.value.clone()),
                );
            }
            Declaration::Namespace(value) => visit(
                &value.body,
                &format!("{prefix}{}.", value.name),
                edition,
                catalog,
            ),
            _ => {}
        }
    }
}

pub(crate) fn global_edition(name: &str) -> Option<EcmaTarget> {
    catalog()
        .globals
        .get(name)
        .copied()
        .filter(|edition| *edition > EcmaTarget::Es5)
}

pub(crate) fn member_edition(value: &Type, member: &str) -> Option<EcmaTarget> {
    let name = match value {
        Type::Named { name, .. } => name.as_str(),
        Type::Array(_) => "Array",
        Type::String => "String",
        _ => return None,
    };
    catalog()
        .members
        .get(&(name.into(), member.into()))
        .copied()
        .filter(|edition| *edition > EcmaTarget::Es5)
}

pub(crate) fn namespace_name(name: &str) -> Option<&str> {
    catalog()
        .names
        .contains(name)
        .then(|| name.rsplit('.').next())
        .flatten()
}

pub(crate) fn alias_value(value: &Type, project: &crate::Project) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    fn owned(items: &[Declaration], prefix: &str, name: &str) -> bool {
        items.iter().any(|item| match item {
            Declaration::TypeAlias(value) => format!("{prefix}{}", value.name) == name,
            Declaration::Interface(value) => format!("{prefix}{}", value.name) == name,
            Declaration::Namespace(value) => {
                owned(&value.body, &format!("{prefix}{}.", value.name), name)
            }
            _ => false,
        })
    }
    if project
        .modules
        .values()
        .any(|module| owned(&module.declarations, "", name))
    {
        return None;
    }
    let (parameters, value) = catalog().aliases.get(name)?;
    let arguments = crate::checker::complete_type_arguments(parameters, arguments)?;
    let substitutions = parameters
        .iter()
        .map(|p| p.name.clone())
        .zip(arguments)
        .collect();
    Some(crate::checker::substitute_type(value, &substitutions))
}
