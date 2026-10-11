// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Related declarations are selected from authorized syntax, never fixture names.
use super::{Diagnostic, SourceSpan, TypeScriptRelatedInformation};
use crate::syntax::Token;
use crate::{Declaration, Project, Type};
use std::collections::BTreeMap;
mod declarations;
mod jsx;
use declarations::Index;

pub(crate) fn attach(project: &Project, diagnostics: &mut [Diagnostic]) {
    let index = Index::new(project);
    for diagnostic in diagnostics {
        let Some(counterpart) = &diagnostic.typescript else {
            continue;
        };
        let mut related = counterpart.related_information.clone();
        for item in &mut related {
            if item.position.is_none() {
                item.position = project
                    .source(&item.span.module)
                    .and_then(|source| super::positions::from_source(source, &item.span));
            }
            if item.code == 6212 {
                item.span = counterpart.span.clone();
                item.position = project
                    .source(&item.span.module)
                    .and_then(|source| super::positions::from_source(source, &item.span));
            }
        }
        let code = counterpart.code;
        let args = &counterpart.arguments;
        let span = &counterpart.span;
        match code {
            2448 | 2449 | 2729 | 2459 | 2724 | 1361 | 2717 => {
                let name = if matches!(code, 2459 | 2724) {
                    args.last()
                } else {
                    args.first()
                };
                if let Some(name) = name.filter(|_| {
                    code != 2724 || args.first().is_some_and(|name| name.ends_with('"'))
                }) {
                    let name = name.split('.').next().unwrap_or(name);
                    let kind = match code {
                        2449 => "class",
                        1361 => "import",
                        2729 | 2717 => "property",
                        _ => "binding",
                    };
                    if let Some(found) = index.named(name, span, kind, code == 2717) {
                        let found = if code == 1361 {
                            project
                                .modules
                                .get(&span.module)
                                .and_then(|module| {
                                    module.declarations.iter().find_map(|declaration| {
                                        let crate::parser::Declaration::Import(import) =
                                            declaration
                                        else {
                                            return None;
                                        };
                                        if import.equals_require {
                                            return None;
                                        }
                                        import
                                            .bindings
                                            .iter()
                                            .find(|binding| {
                                                binding.local == name
                                                    && binding.span.start <= found.start
                                                    && found.end <= binding.span.end
                                            })
                                            .map(|binding| binding.span.clone())
                                    })
                                })
                                .unwrap_or(found)
                        } else {
                            found
                        };
                        let code = match code {
                            1361 => 1376,
                            2717 => 6203,
                            _ => 2728,
                        };
                        push(project, &mut related, code, vec![name.into()], found);
                    }
                }
            }
            2551 | 2552 => {
                if let Some(name) = args.last() {
                    let key = if code == 2551 {
                        format!("String.{name}")
                    } else {
                        name.clone()
                    };
                    if let Some(declaration) = super::templates::library_declaration(&key) {
                        let span =
                            SourceSpan::new(&declaration.file, declaration.start, declaration.end);
                        let message = super::mapping::build(2728, &span, vec![name.clone()], "")
                            .unwrap()
                            .message;
                        related.push(TypeScriptRelatedInformation {
                            code: 2728,
                            message,
                            span,
                            position: Some(super::TypeScriptPosition {
                                line: declaration.line,
                                column: declaration.column,
                                length: declaration.length,
                            }),
                        });
                    }
                }
            }
            2741 => {
                if let (Some(name), Some(owner)) = (args.first(), args.get(2)) {
                    if let Some(field) = index.field(name, owner, &span.module) {
                        push(
                            project,
                            &mut related,
                            2728,
                            vec![name.clone()],
                            field.related_span(),
                        );
                    }
                    if args.get(1).is_some_and(|actual| {
                        actual.starts_with("typeof ") || actual.contains("readonly prototype:")
                    }) {
                        push(project, &mut related, 6213, Vec::new(), span.clone());
                    }
                }
            }
            2394 => {
                if let Some((_, name)) = index.implementation(span) {
                    push(project, &mut related, 2750, Vec::new(), name);
                }
            }
            2554 | 1238 | 1240 => {
                if let Some(parameter) = index.missing_parameter(diagnostic) {
                    push(
                        project,
                        &mut related,
                        6210,
                        vec![parameter.name.clone()],
                        parameter.span.clone(),
                    );
                }
            }
            2769 if !related.iter().any(|item| item.code == 2793) => {
                if let Some((parameters, name)) = index.call_implementation(span) {
                    let source = project.source(&span.module).unwrap_or("");
                    let actual = source.get(span.start..span.end).unwrap_or("");
                    if parameters.iter().any(|parameter| {
                        parameter
                            .annotation
                            .as_ref()
                            .is_some_and(|ty| accepts_literal(ty, actual))
                    }) {
                        push(project, &mut related, 2793, Vec::new(), name);
                    }
                }
            }
            2322 if !related.iter().any(|item| item.code == 6500) => {
                let source = project.source(&span.module).unwrap_or("");
                let selected = source.get(span.start..span.end).unwrap_or("");
                let child_type = diagnostic
                    .message
                    .starts_with("the element's children have type `JSX.Element`");
                let attribute_type = diagnostic.message.starts_with("attribute ")
                    && diagnostic.message.contains("has type");
                let nested_record = diagnostic.message.contains("`record`")
                    && !diagnostic.message.contains("function")
                    && project.source(&span.module).is_some_and(|source| {
                        source
                            .get(diagnostic.span.start..diagnostic.span.end)
                            .is_some_and(|text| {
                                if diagnostic.message.starts_with("argument ") {
                                    text.contains('{')
                                } else {
                                    text.split_once('=').is_some_and(|(_, value)| {
                                        value.trim_start().starts_with('{')
                                    })
                                }
                            })
                    });
                let name = if child_type { "children" } else { selected };
                let props = index.jsx_owner(span, child_type).unwrap_or_default();
                if let Some(field) = index
                    .field(name, &props, &span.module)
                    .filter(|field| (child_type || attribute_type || nested_record)
                        && !(nested_record && project.modules.get(&field.span.module).is_some_and(|module| {
                            module.declarations.iter().any(|declaration| matches!(declaration,
                                crate::Declaration::TypeAlias(alias) if alias.name == field.owner
                                    && matches!(&alias.value, crate::Type::Union(parts) if parts.iter().any(|part| !matches!(part, crate::Type::Record(_))))))
                        })))
                {
                    let owner = index.expected_owner(field, span);
                    push(
                        project,
                        &mut related,
                        6500,
                        vec![name.into(), owner],
                        field.name_span.clone(),
                    );
                } else if diagnostic.message.starts_with("missing required props: ")
                    && !related.iter().any(|item| item.code == 2728) {
                    if let Some(name) = diagnostic.message.strip_prefix("missing required props: ")
                    {
                        if let Some(field) = index.field(name, &props, &span.module) {
                            push(
                                project,
                                &mut related,
                                2728,
                                vec![name.into()],
                                field.related_span(),
                            );
                        }
                    }
                }
            }
            2786 if !diagnostic.message.contains("declared ElementType") => {
                if let Some(field) = index.field("render", "ElementClass", &span.module) {
                    push(
                        project,
                        &mut related,
                        2728,
                        vec!["render".into()],
                        field.related_span(),
                    );
                }
            }
            _ => {}
        }
        diagnostic.typescript.as_mut().unwrap().related_information = related;
    }
}

fn accepts_literal(ty: &Type, value: &str) -> bool {
    match ty {
        Type::Boolean => value == "true" || value == "false",
        Type::Number => value.parse::<f64>().is_ok(),
        Type::String => value.starts_with(['\'', '"']),
        Type::Union(values) => values.iter().any(|ty| accepts_literal(ty, value)),
        Type::Any | Type::Unknown => true,
        _ => false,
    }
}

fn push(
    project: &Project,
    result: &mut Vec<TypeScriptRelatedInformation>,
    code: u32,
    args: Vec<String>,
    span: SourceSpan,
) {
    let message = super::mapping::build(code, &span, args, "")
        .unwrap()
        .message;
    let position = project
        .source(&span.module)
        .and_then(|source| super::positions::from_source(source, &span));
    result.push(TypeScriptRelatedInformation {
        code,
        message,
        span,
        position,
    });
}
