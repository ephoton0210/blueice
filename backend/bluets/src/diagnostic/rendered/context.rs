// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextual names and selected type causes from retained authorized inputs.
use super::*;

pub(super) fn refine(project: &Project, diagnostic: &mut Diagnostic) {
    let Some(counterpart) = diagnostic.typescript.as_ref() else {
        return;
    };
    let code = counterpart.code;
    let mut args = counterpart.arguments.clone();
    let mut detail = None;
    let source = project.source(&counterpart.span.module).unwrap_or("");
    let original = source
        .get(diagnostic.span.start..diagnostic.span.end)
        .unwrap_or("");
    let selected = source
        .get(counterpart.span.start..counterpart.span.end)
        .unwrap_or("");
    let module = project.modules.get(&counterpart.span.module);
    let path = |id: &str| {
        id.strip_suffix(".tsx")
            .or_else(|| id.strip_suffix(".ts"))
            .unwrap_or(id)
            .to_string()
    };
    if matches!(code, 2322 | 2339 | 2613 | 2724) {
        if let Some(module) = module {
            for declaration in &module.declarations {
                let Declaration::Import(import) = declaration else {
                    continue;
                };
                let Some(resolved) = project
                    .resolutions
                    .get(&(module.id.clone(), import.specifier.clone()))
                else {
                    continue;
                };
                let Some(dependency) = project.modules.get(resolved) else {
                    continue;
                };
                let target = path(resolved);
                if code == 2613 && args.first() == Some(&import.specifier) {
                    args = vec![format!("\"{target}\""), args[1].clone(), target.clone()];
                }
                for binding in &import.bindings {
                    if code == 2339
                        && binding.imported == "*"
                        && source.get(..counterpart.span.start).is_some_and(|before| {
                            before.trim_end().ends_with(&format!("{}.", binding.local))
                        })
                        && args.len() == 2
                    {
                        args[1] = format!("typeof import(\"{target}\")");
                    }
                    if code == 2724 && args.first() == Some(&binding.local) {
                        args[0] = format!("\"{target}\".{}", binding.imported);
                    }
                    if code == 2322 && args.len() == 2 {
                        for item in &dependency.declarations {
                            let Declaration::Enum(group) = item else {
                                continue;
                            };
                            if args[0].starts_with(&format!("{}.", group.name))
                                || args[0]
                                    .starts_with(&format!("{}.{}.", binding.local, group.name))
                            {
                                let default=dependency.declarations.iter().any(|item|matches!(item,Declaration::DefaultExport(export) if export.name==group.name));
                                args[0] = format!(
                                    "import(\"{target}\").{}",
                                    if default { "default" } else { &group.name }
                                );
                            }
                        }
                    }
                }
            }
            if code == 2322 && args.len() == 2 {
                for item in &module.declarations {
                    let Declaration::Enum(group) = item else {
                        continue;
                    };
                    if args[0].starts_with(&format!("{}.", group.name))
                        && !args[1].starts_with(&format!("{}.", group.name))
                    {
                        args[0] = group.name.clone();
                    }
                    if args[1] == group.name && args[0] == "string" {
                        if let Some((_, value)) = original.split_once('=') {
                            let value = value.trim().trim_end_matches(';').trim();
                            if value.starts_with(['\'', '"']) && value.ends_with(['\'', '"']) {
                                args[0] =
                                    serde_json::to_string(&value[1..value.len() - 1]).unwrap();
                            }
                        }
                    }
                }
            }
        }
    }
    match code {
        2786 if diagnostic.message.starts_with("this component's return type")=>{
            args=vec![selected.into()];
            let result=diagnostic.message.split('`').nth(1).unwrap_or("unknown");
            detail=Some(format!("\n  Its return type '{result}' is not a valid JSX element."));
        }
        2786=>{
            if let Some(name)=args.first() {
                let missing=project.modules.values().flat_map(|module|&module.declarations).find_map(|item|match item {Declaration::Namespace(ns) if ns.name=="JSX"=>ns.body.iter().find_map(|item|match item {Declaration::Interface(interface) if interface.name=="ElementClass"=>interface.fields.first().map(|field|field.name.as_str()),_=>None}),_=>None});
                if let Some(field)=missing {detail=Some(format!("\n  Its instance type '{name}' is not a valid JSX element.\n    Property '{field}' is missing in type '{name}' but required in type 'ElementClass'."));}
            }
        }
        7053 if diagnostic.message.starts_with("an index of type") && args.len()==2=>{
            if let Some((_,index))=selected.rsplit_once('[') {args[0]=index.trim_end_matches(']').into();args[1]=format!("typeof {}",args[1]);detail=Some(format!("\n  Property '{}' does not exist on type '{}'.",args[0],args[1]));}
        }
        2322 if args.len()==2 && original.trim().trim_end_matches(';').trim()=="return this"=>{detail=Some(format!("\n  Type '{}' is not assignable to type '{}'.",args[0],args[1]));args[0]="this".into();}
        2322 if args.len()==2 && args.iter().all(|arg|arg.starts_with('[')&&arg.ends_with(']')) && !selected.contains(['[',']'])=>{
            let index=source.get(..counterpart.span.start).and_then(|before|before.rsplit_once('[')).map(|(_,elements)|elements.matches(',').count()).unwrap_or(0);
            let element=|text:&str|text[1..text.len()-1].split(',').nth(index).map(|value|value.trim().to_string());
            if let (Some(actual),Some(expected))=(element(&args[0]),element(&args[1])) {args=vec![actual,expected];detail=Some(String::new());}
        }
        2322 if args.len()==2 && args[0].contains('|') && module.is_some_and(|module|module.declarations.iter().any(|item|matches!(item,Declaration::Function(function) if function.return_type.is_none() && original.contains(&format!("{}(",function.name)))))=>{
            let widen=|value:&str|if value.starts_with('"'){"string".to_string()}else if value.parse::<f64>().is_ok(){"number".to_string()}else{value.to_string()};
            let mut values=args[0].split(" | ").map(widen).collect::<Vec<_>>();values.sort_by_key(|value|match value.as_str(){"string"=>0,"number"=>1,_=>2});values.dedup();
            args[0]=values.join(" | ");
            detail=Some(counterpart.message.split_once('\n').map(|(_,reason)|{let mut reason=reason.to_string();for value in counterpart.arguments[0].split(" | "){reason=reason.replace(&format!("'{value}'"),&format!("'{}'",widen(value)));}format!("\n{reason}")}).unwrap_or_default());
        }
        2339 if args.first().is_some_and(|name|name.starts_with('#')) && source.get(counterpart.span.end..).is_some_and(|rest|rest.trim_start().starts_with("in ")) && args.len()==2=>{args[1]="any".into();}
        1382=>args=vec![selected.into(),if selected==">"{"gt".into()}else{"rbrace".into()}],
        17008=>args=vec![selected.into()],
        17002=>{
            let name=source.get(..counterpart.span.start).unwrap_or("").rsplit('<').find_map(|part|{if part.starts_with('/') {return None;}let name=part.split(|ch:char|!ch.is_alphanumeric() && ch!='_' && ch!='.').next()?;(!name.is_empty()).then_some(name.to_string())});
            if let Some(name)=name {args=vec![name];}
        }
        2345 if args.len()==2 && matches!(selected,"true"|"false") && args[1].contains('|')=>args[0]=selected.into(),
        2551 if args.len()==3=>{if let Some(name)=diagnostic.message.split('`').nth(3).filter(|name|name.contains('.')) {args[1]=name.into();}}
        _=>{},
    }
    if args != counterpart.arguments || detail.is_some() {
        let mut rendered = super::super::mapping::build(
            code,
            &counterpart.span,
            args.clone(),
            &diagnostic.message,
        )
        .unwrap();
        if let Some(detail) = detail {
            rendered.message.push_str(&detail);
        } else if let Some((_, detail)) = counterpart.message.split_once('\n') {
            rendered.message.push('\n');
            rendered.message.push_str(detail);
        }
        let counterpart = diagnostic.typescript.as_mut().unwrap();
        counterpart.arguments = args;
        counterpart.message = rendered.message;
    }
}
