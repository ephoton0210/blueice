// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Context refines template arguments without changing BTS prose or verdicts.
use super::Diagnostic;
use crate::{Declaration, Project};
mod classes;
mod context;
mod decorators;

pub(crate) fn attach(project: &Project, diagnostics: &mut [Diagnostic]) {
    for diagnostic in diagnostics {
        let Some(counterpart) = &diagnostic.typescript else {
            continue;
        };
        let code = counterpart.code;
        let mut args = counterpart.arguments.clone();
        let raw = &diagnostic.message;
        let previous = counterpart.message.clone();
        let source = project.source(&counterpart.span.module).unwrap_or("");
        let selected = source
            .get(counterpart.span.start..counterpart.span.end)
            .unwrap_or("");
        match code {
            2348 => {
                if let Some(name) = raw
                    .strip_prefix("class ")
                    .and_then(|rest| rest.split_whitespace().next())
                {
                    args = vec![format!("typeof {name}")];
                }
            }
            2322 if args.len() == 3 => {
                args = if raw.starts_with("attribute ") {
                    vec![args[1].clone(), args[2].clone()]
                } else {
                    vec![args[0].clone(), args[2].clone()]
                };
            }
            2449 if args.is_empty() => {
                if let Some(name) = raw
                    .strip_prefix("TS2449: class ")
                    .and_then(|rest| rest.split_whitespace().next())
                {
                    args = vec![name.into()];
                }
            }
            2506 => {
                args = raw
                    .strip_prefix("class ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(|name| vec![name.into()])
                    .unwrap_or(args)
            }
            2673 | 2674 => {
                args = raw
                    .strip_prefix("constructor of class ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(|name| vec![name.into()])
                    .unwrap_or(args)
            }
            18033 => {
                if let Some(value) = args.first().cloned() {
                    args = vec![value, "number".into()]
                }
            }
            1029 => {
                let previous = source
                    .get(..counterpart.span.start)
                    .unwrap_or("")
                    .split(|character: char| !character.is_alphanumeric())
                    .rfind(|part| !part.is_empty())
                    .unwrap_or("readonly");
                args = vec![selected.into(), previous.into()];
            }
            7026 => args = vec!["IntrinsicElements".into()],
            7023 => args = vec![selected.into()],
            2708 => {
                if let Some(name) = args.first_mut() {
                    *name = name.split('.').next().unwrap_or(name).to_string()
                }
            }
            2305 | 2459 => {
                if let Some(module) = args.first_mut() {
                    *module = format!("\"{module}\"")
                }
            }
            2724 if args.first().is_some_and(|arg| arg.starts_with('.')) => {
                args[0] = format!("\"{}\"", args[0])
            }
            1329 => {
                args = vec![selected
                    .trim_start_matches('@')
                    .trim_end_matches("()")
                    .into()]
            }
            2554 if args.is_empty() => {
                let values = raw
                    .split_whitespace()
                    .filter_map(|word| word.parse::<usize>().ok())
                    .collect::<Vec<_>>();
                if values.len() == 3 {
                    args = vec![values[0].to_string(), values[2].to_string()];
                }
            }
            2610 | 2611 | 2612 | 2415 => {
                if let Some(class) =
                    project
                        .modules
                        .get(&counterpart.span.module)
                        .and_then(|module| {
                            module
                                .declarations
                                .iter()
                                .find_map(|declaration| match declaration {
                                    Declaration::Class(class)
                                        if class.span.start <= counterpart.span.start
                                            && class.span.end >= counterpart.span.end =>
                                    {
                                        Some(class)
                                    }
                                    _ => None,
                                })
                        })
                {
                    let base = class.extends_name.clone().unwrap_or_default();
                    let name = raw.split('`').nth(1).unwrap_or(selected);
                    args = match code {
                        2610 | 2611 => vec![name.into(), base, class.name.clone()],
                        2612 => vec![name.into(), base],
                        _ => vec![class.name.clone(), base],
                    };
                }
            }
            _ => {}
        }
        let span = counterpart.span.clone();
        if let Some(rendered) = super::mapping::build(code, &span, args.clone(), raw) {
            let counterpart = diagnostic.typescript.as_mut().unwrap();
            counterpart.arguments = args;
            counterpart.message = rendered.message;
            if let Some((_, detail)) = previous.split_once('\n') {
                counterpart.message.push('\n');
                counterpart.message.push_str(detail);
            }
        }
        classes::refine(project, diagnostic);
        decorators::refine(project, diagnostic);
        context::refine(project, diagnostic);
    }
}
