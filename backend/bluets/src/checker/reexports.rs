// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closed module exports retain binding origins across aliases and star paths.

use super::*;

type Origin = (String, String);
pub(super) type Origins = BTreeMap<String, BTreeMap<String, BTreeSet<Origin>>>;

pub(super) fn origins(project: &Project) -> Origins {
    let names = project::local_exported_names(project);
    let mut explicit = BTreeMap::<String, BTreeSet<String>>::new();
    let mut result = Origins::new();
    for (id, module) in &project.modules {
        let mut own = BTreeMap::new();
        let mut protected = names[id].0.clone();
        for name in &protected {
            own.insert(name.clone(), BTreeSet::from([(id.clone(), name.clone())]));
        }
        for declaration in &module.declarations {
            match declaration {
                Declaration::DefaultExport(export) => {
                    own.insert(
                        "default".to_string(),
                        BTreeSet::from([(id.clone(), export.name.clone())]),
                    );
                }
                Declaration::Class(class) if class.default_export => {
                    own.insert(
                        "default".to_string(),
                        BTreeSet::from([(id.clone(), class.name.clone())]),
                    );
                }
                Declaration::Function(function) if function.default_export => {
                    own.insert(
                        "default".to_string(),
                        BTreeSet::from([(id.clone(), function.name.clone())]),
                    );
                }
                Declaration::ValueExport(export) => {
                    for binding in &export.bindings {
                        protected.insert(binding.exported.clone());
                        if export.specifier.is_some() {
                            own.remove(&binding.exported);
                        } else {
                            own.insert(
                                binding.exported.clone(),
                                BTreeSet::from([(id.clone(), binding.local.clone())]),
                            );
                        }
                    }
                    if let Some(name) = &export.namespace {
                        protected.insert(name.clone());
                        if let Some(target) = export.specifier.as_ref().and_then(|specifier| {
                            project.resolutions.get(&(id.clone(), specifier.clone()))
                        }) {
                            own.insert(
                                name.clone(),
                                BTreeSet::from([(target.clone(), "*".to_string())]),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        explicit.insert(id.clone(), protected);
        result.insert(id.clone(), own);
    }
    // Each pass advances one graph edge. Origins only grow, so cycles terminate
    // within the number of modules; no unresolved edge grants an open surface.
    for _ in 0..project.modules.len() {
        let previous = result.clone();
        for (id, module) in &project.modules {
            for declaration in &module.declarations {
                let Declaration::ValueExport(export) = declaration else {
                    continue;
                };
                let Some(target) = export.specifier.as_ref().and_then(|specifier| {
                    project.resolutions.get(&(id.clone(), specifier.clone()))
                }) else {
                    continue;
                };
                let source = &previous[target];
                let own = result.get_mut(id).expect("retained module");
                if export.star && export.namespace.is_none() {
                    for (name, origins) in source {
                        if name != "default" && name != "export=" && !explicit[id].contains(name) {
                            own.entry(name.clone())
                                .or_default()
                                .extend(origins.iter().cloned());
                        }
                    }
                } else if !export.star {
                    for binding in &export.bindings {
                        if let Some(origins) = source.get(&binding.local) {
                            own.entry(binding.exported.clone())
                                .or_default()
                                .extend(origins.iter().cloned());
                        }
                    }
                }
            }
        }
        if result == previous {
            break;
        }
    }
    result
}

pub(super) fn forward<T: Clone>(
    origins: &Origins,
    surfaces: &BTreeMap<String, BTreeMap<String, T>>,
) -> BTreeMap<String, BTreeMap<String, T>> {
    let mut values = BTreeMap::new();
    for (id, bindings) in surfaces {
        for (name, value) in bindings {
            if let Some(origin) = origins
                .get(id)
                .and_then(|names| names.get(name))
                .filter(|origins| origins.len() == 1)
                .and_then(|origins| origins.first())
            {
                if &origin.0 == id {
                    values
                        .entry(origin.clone())
                        .or_insert_with(|| value.clone());
                }
            }
        }
    }
    let mut result = surfaces.clone();
    for (id, bindings) in origins {
        let target = result.entry(id.clone()).or_default();
        for (name, candidates) in bindings {
            if candidates.len() == 1 {
                if let Some(value) = candidates.first().and_then(|origin| values.get(origin)) {
                    target.insert(name.clone(), value.clone());
                }
            } else {
                target.remove(name);
            }
        }
    }
    result
}

pub(super) fn exported_names(project: &Project) -> BTreeMap<String, (BTreeSet<String>, bool)> {
    let local = project::local_exported_names(project);
    origins(project)
        .into_iter()
        .map(|(id, names)| {
            let open = local[&id].1;
            (
                id,
                (
                    names
                        .into_iter()
                        .filter_map(|(name, origins)| (origins.len() == 1).then_some(name))
                        .collect(),
                    open,
                ),
            )
        })
        .collect()
}

/// Type-only forwarding retains the declaration's private type identities and
/// permits type queries without publishing a runtime value.
pub(super) fn forward_type_values(
    project: &Project,
    values: &mut BTreeMap<String, BTreeMap<String, module::ExportedValue>>,
) {
    for _ in 0..project.modules.len() {
        let mut forwarded = false;
        for (id, module) in &project.modules {
            let mut additions = BTreeMap::new();
            for declaration in &module.declarations {
                let Declaration::TypeExport(export) = declaration else {
                    continue;
                };
                let Some(source) = project
                    .resolved_type_export(id, export)
                    .and_then(|target| values.get(target))
                else {
                    continue;
                };
                if export.star && export.namespace.is_none() {
                    additions.extend(
                        source
                            .iter()
                            .filter(|(name, _)| name.as_str() != "default")
                            .map(|(name, value)| (name.clone(), value.type_only(&export.span))),
                    );
                }
                for binding in &export.bindings {
                    if let Some(value) = source.get(&binding.local) {
                        additions.insert(binding.exported.clone(), value.type_only(&binding.span));
                    }
                }
            }
            forwarded |= !additions.is_empty();
            let target = values.entry(id.clone()).or_default();
            for (name, value) in additions {
                target.insert(name, value);
            }
        }
        if !forwarded {
            break;
        }
    }
}
