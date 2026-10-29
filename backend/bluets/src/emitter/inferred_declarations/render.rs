// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration rendering retains checked identities and source diagnostics.
use super::*;

impl Context<'_> {
    fn render_operator(
        &mut self,
        value: &Type,
        indent: usize,
        span: &SourceSpan,
    ) -> Option<Result<String, Diagnostic>> {
        if let Type::IndexedRecord { object, indices } = value {
            return Some((|| {
                let mut indices = indices.clone();
                for index in &mut indices {
                    index.key = Type::Literal(self.render(&index.key, indent + 4, span)?);
                    index.value = Type::Literal(self.render(&index.value, indent + 4, span)?);
                }
                let fields = match object.as_ref() {
                    Type::Record(fields) => fields,
                    _ => return Ok(type_to_ts(value)),
                };
                let fields = self.render_fields(fields, indent, span)?;
                Ok(callable_objects::render_with_indent(
                    &fields,
                    &[],
                    &indices,
                    indent,
                ))
            })());
        }
        let mut failure = None;
        let mapped = value.map_operator(|child, _| {
            match self.render(
                child,
                indent + usize::from(matches!(value, Type::Mapped(_))) * 4,
                span,
            ) {
                Ok(text) => Type::Literal(text),
                Err(error) => {
                    failure = Some(error);
                    Type::Unknown
                }
            }
        })?;
        if let Some(error) = failure {
            return Some(Err(error));
        }
        let text = mapped
            .operator_text(|value| {
                if let Type::Literal(text) = value {
                    text.clone()
                } else {
                    type_to_ts(value)
                }
            })
            .expect("mapped operator");
        Some(Ok(if matches!(value, Type::Mapped(_)) {
            let member = text
                .strip_prefix("{ ")
                .and_then(|text| text.strip_suffix(" }"))
                .expect("mapped spelling");
            format!(
                "{{\n{}{member}\n{}}}",
                " ".repeat(indent + 4),
                " ".repeat(indent)
            )
        } else {
            text
        }))
    }

    pub(super) fn render_fields(
        &mut self,
        fields: &[crate::parser::TypeField],
        indent: usize,
        span: &SourceSpan,
    ) -> Result<Vec<crate::parser::TypeField>, Diagnostic> {
        let mut rendered = Vec::with_capacity(fields.len());
        let inferred = fields.iter().all(|field| field.span.module == "<inferred>");
        let computed_class = self
            .module
            .class_expressions
            .values()
            .filter(|expression| {
                expression.class.extends_span.as_ref().is_some_and(|span| {
                    self.module
                        .source
                        .get(span.start..span.end)
                        .is_some_and(|source| source.contains('(') || source.contains('['))
                }) && fields.iter().any(|field| {
                    expression
                        .class
                        .members
                        .iter()
                        .any(|member| field.span == member.span)
                })
            })
            .max_by_key(|expression| expression.class.span.start);
        let mut ordered = fields.iter().collect::<Vec<_>>();
        if let Some(expression) = computed_class {
            ordered.sort_by_key(|field| {
                let own = expression.class.span.start <= field.span.start
                    && field.span.end <= expression.class.span.end;
                (usize::from(!own), field.span.start)
            });
        }
        for (index, field) in ordered.into_iter().enumerate() {
            let mut field = field.clone();
            if let Some(owner) = self.project.modules.get(&field.span.module) {
                let source_key = owner
                    .classes()
                    .chain(
                        owner
                            .class_expressions
                            .values()
                            .map(|expression| &expression.class),
                    )
                    .flat_map(|class| &class.members)
                    .find_map(|member| {
                        let named = member
                            .method
                            .as_ref()
                            .map(|value| (&value.name, &value.name_span))
                            .or_else(|| {
                                member
                                    .accessor
                                    .as_ref()
                                    .map(|value| (&value.name, &value.name_span))
                            })
                            .or_else(|| {
                                member
                                    .field
                                    .as_ref()
                                    .map(|value| (&value.name, &value.name_span))
                            });
                        let (name, span) = named?;
                        if name == &field.name && (field.span == member.span || field.span == *span)
                        {
                            member
                                .key
                                .first()
                                .filter(|key| key.kind == crate::TokenKind::String)
                                .map(|key| key.text.clone())
                        } else {
                            None
                        }
                    });
                if let Some(key) = source_key {
                    field.name = key;
                }
            }
            if inferred || computed_class.is_some() {
                // Printer-only order markers; checked symbols keep their spans.
                field.span.start = index;
                field.span.end = index;
            }
            match &mut field.value {
                Type::Function { parameters, result }
                | Type::GenericFunction {
                    parameters, result, ..
                } if field.method => {
                    for parameter in parameters {
                        parameter.annotation = parameter
                            .annotation
                            .as_ref()
                            .map(|value| self.render(value, indent + 4, span).map(Type::Literal))
                            .transpose()?;
                    }
                    **result = Type::Literal(self.render(result, indent + 4, span)?);
                }
                value => *value = Type::Literal(self.render(value, indent + 4, span)?),
            }
            rendered.push(field);
        }
        Ok(rendered)
    }

    pub(super) fn render(
        &mut self,
        value: &Type,
        indent: usize,
        span: &SourceSpan,
    ) -> Result<String, Diagnostic> {
        if let Some(rendered) = self.render_operator(value, indent, span) {
            return rendered;
        }
        Ok(match value {
            Type::Predicate(predicate) => {
                let target = predicate
                    .target
                    .as_deref()
                    .map(|target| self.render(target, indent, span))
                    .transpose()?;
                predicate.text(|_| target.unwrap_or_default())
            }
            Type::CallableRecord { fields, signatures } => {
                let fields = self.render_fields(fields, indent, span)?;
                let mut mapped = Vec::new();
                for signature in signatures {
                    let mut signature = signature.clone();
                    for parameter in &mut signature.parameters {
                        parameter.annotation = parameter
                            .annotation
                            .as_ref()
                            .map(|value| self.render(value, indent + 4, span).map(Type::Literal))
                            .transpose()?;
                    }
                    signature.result =
                        Type::Literal(self.render(&signature.result, indent + 4, span)?);
                    mapped.push(signature);
                }
                if fields.is_empty() && mapped.len() == 1 && mapped[0].constructor_arrow {
                    callable_objects::render(&fields, &mapped)
                } else {
                    callable_objects::render_with_indent(&fields, &mapped, &[], indent)
                }
            }
            Type::Named { name, arguments } => {
                if let Some((owner, spelling)) = name
                    .strip_prefix("\0import:")
                    .and_then(|name| name.split_once('\0'))
                {
                    if owner != self.module.id {
                        if !spelling.contains('(') {
                            return self.render(
                                &Type::Named {
                                    name: format!("{spelling}@{owner}"),
                                    arguments: arguments.clone(),
                                },
                                indent,
                                span,
                            );
                        }
                        return Err(self.cannot_name(spelling, owner, span));
                    }
                    self.retain(spelling);
                }
                let name = if name.starts_with("\0import:") {
                    if crate::parser::is_javascript(&self.module.id) {
                        declarations::canonical_import_type(crate::parser::source_type_name(name))
                    } else {
                        crate::parser::source_type_name(name).to_string()
                    }
                } else if name.starts_with("typeof ") {
                    self.retain(name);
                    crate::parser::source_type_name(name).to_string()
                } else if name.starts_with("#typeparam@") {
                    crate::parser::source_type_name(name).to_string()
                } else if let Some((bare, id)) = name.split_once('@') {
                    let (root, suffix) = bare
                        .split_once('.')
                        .map_or((bare, "".to_string()), |(root, rest)| {
                            (root, format!(".{rest}"))
                        });
                    let source = self.project.modules.get(id);
                    let public = source.and_then(|source| public_type_name(source, root));
                    let specifier =
                        self.module
                            .declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                Declaration::Import(import)
                                    if self
                                        .project
                                        .resolved_import(&self.module.id, import)
                                        .is_some_and(|source| source == id) =>
                                {
                                    Some(import.specifier.clone())
                                }
                                _ => None,
                            });
                    let Some((public, specifier)) = public.zip(specifier) else {
                        return Err(self.cannot_name(root, id, span));
                    };
                    let direct = self.module.declarations.iter().find_map(|declaration| {
                        let Declaration::Import(import) = declaration else {
                            return None;
                        };
                        if self
                            .project
                            .resolved_import(&self.module.id, import)
                            .is_none_or(|source| source != id)
                        {
                            return None;
                        }
                        import.bindings.iter().find_map(|binding| {
                            if binding.imported == public {
                                Some(format!("{}{suffix}", binding.local))
                            } else if binding.imported == "*" {
                                Some(format!("{}.{public}{suffix}", binding.local))
                            } else {
                                None
                            }
                        })
                    });
                    if let Some(direct) = direct {
                        self.retain(&direct);
                        direct
                    } else {
                        format!("import(\"{specifier}\").{public}{suffix}")
                    }
                } else {
                    self.retain(name);
                    crate::parser::source_type_name(name).to_string()
                };
                if arguments.is_empty() {
                    name
                } else {
                    format!(
                        "{name}<{}>",
                        arguments
                            .iter()
                            .map(|value| self.render(value, indent, span))
                            .collect::<Result<Vec<_>, _>>()?
                            .join(", ")
                    )
                }
            }
            Type::Record(fields) => {
                let fields = self.render_fields(fields, indent, span)?;
                callable_objects::render_with_indent(&fields, &[], &[], indent)
            }
            Type::Array(element) => {
                let text = self.render(element, indent, span)?;
                format!(
                    "{}[]",
                    if matches!(
                        element.as_ref(),
                        Type::Union(_)
                            | Type::Intersection(_)
                            | Type::Function { .. }
                            | Type::Infer(_)
                    ) {
                        format!("({text})")
                    } else {
                        text
                    }
                )
            }
            Type::Readonly(inner) => format!("readonly {}", self.render(inner, indent, span)?),
            Type::Tuple(elements) => {
                let mut members = Vec::new();
                for element in elements {
                    let annotation = self.render(&element.annotation, indent, span)?;
                    members.push(if let Some(label) = &element.label {
                        format!(
                            "{}{label}{}: {annotation}",
                            if element.rest { "..." } else { "" },
                            if element.optional { "?" } else { "" },
                        )
                    } else {
                        format!(
                            "{}{annotation}{}",
                            if element.rest { "..." } else { "" },
                            if element.optional { "?" } else { "" },
                        )
                    });
                }
                format!("[{}]", members.join(", "))
            }
            Type::Function { parameters, result }
            | Type::GenericFunction {
                parameters, result, ..
            } => {
                let mut binders = String::new();
                if let Type::GenericFunction {
                    type_parameters, ..
                } = value
                {
                    emit_type_parameters(&mut binders, type_parameters);
                }
                let mut members = Vec::new();
                for parameter in parameters {
                    let annotation = self.render(
                        parameter.annotation.as_ref().unwrap_or(&Type::Unknown),
                        indent,
                        span,
                    )?;
                    members.push(format!(
                        "{}{}{}: {annotation}",
                        if parameter.rest { "..." } else { "" },
                        parameter.name,
                        if parameter.optional { "?" } else { "" },
                    ));
                }
                format!(
                    "{binders}({}) => {}",
                    members.join(", "),
                    self.render(result, indent, span)?
                )
            }
            Type::Union(options) | Type::Intersection(options) => {
                let mut options = options.clone();
                if matches!(value, Type::Union(_)) {
                    let mut budget = self.max_expansions;
                    if !super::unions::complete(
                        &mut options,
                        self.checking.exact_optional_property_types,
                        &mut budget,
                    ) {
                        return Err(Diagnostic::error(DiagnosticCode::ResourceLimit,span.clone(),"anonymous declaration completion exceeds the configured expansion limit"));
                    }
                }
                options
                    .iter()
                    .map(|option| {
                        self.render(option, indent, span).map(|text| {
                            if matches!(
                                option,
                                Type::Function { .. } | Type::GenericFunction { .. }
                            ) {
                                format!("({text})")
                            } else {
                                text
                            }
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(if matches!(value, Type::Union(_)) {
                        " | "
                    } else {
                        " & "
                    })
            }
            Type::KeyOf(value) => format!("keyof {}", self.render(value, indent, span)?),
            Type::IndexedAccess { object, index, .. } => format!(
                "{}[{}]",
                self.render(object, indent, span)?,
                self.render(index, indent, span)?
            ),
            Type::Literal(name) => {
                if let Some((_, owner)) = name.split_once('@') {
                    if self.project.modules.contains_key(owner) {
                        return self.render(
                            &Type::Named {
                                name: name.clone(),
                                arguments: Vec::new(),
                            },
                            indent,
                            span,
                        );
                    }
                }
                self.retain(name);
                name.clone()
            }
            _ => {
                self.references(value);
                type_to_ts(value)
            }
        })
    }
}
