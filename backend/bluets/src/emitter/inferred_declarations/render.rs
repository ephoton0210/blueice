// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration rendering retains checked identities and source diagnostics.
use super::*;

type RenderedFields = (Vec<crate::parser::TypeField>, BTreeMap<usize, SourceSpan>);

impl Context<'_> {
    fn jsx_runtime_type(&self, member: &str, span: &SourceSpan) -> Result<String, Diagnostic> {
        let Some(provider) = self.project.jsx_runtime_namespace_provider(&self.module.id) else {
            return Err(self.cannot_name(member, &self.module.id, span));
        };
        let pragmas = crate::jsx::Pragmas::of(&self.module.source);
        let source = pragmas
            .import_source
            .as_deref()
            .or(self.output_options.jsx_import_source.as_deref())
            .unwrap_or("react");
        let runtime = if self.output_options.jsx == Some(crate::JsxMode::ReactJsxDev) {
            "jsx-dev-runtime"
        } else {
            "jsx-runtime"
        };
        let specifier =
            crate::checker::preferred_namespace_import(self.project, self.module, provider, "JSX")
                .unwrap_or_else(|| format!("{source}/{runtime}"));
        let specifier = serde_json::to_string(&specifier).expect("JSX runtime specifier JSON");
        Ok(format!("import({specifier}).{member}"))
    }

    fn render_operator(
        &mut self,
        value: &Type,
        indent: usize,
        span: &SourceSpan,
    ) -> Option<Result<String, Diagnostic>> {
        if let Type::StringMapping(mapping) = value {
            return Some(self.render_string_mapping(mapping, indent, span));
        }
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
        self.render_fields_with_origins(fields, indent, span)
            .map(|(fields, _)| fields)
    }

    fn render_fields_with_origins(
        &mut self,
        fields: &[crate::parser::TypeField],
        indent: usize,
        span: &SourceSpan,
    ) -> Result<RenderedFields, Diagnostic> {
        let mut rendered = Vec::with_capacity(fields.len());
        let mut origins = BTreeMap::new();
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
            let origin = field.span.clone();
            let mut field = field.clone();
            if let Some(owner) = self.project.modules.get(&field.span.module) {
                let inferred_literal = owner
                    .classes()
                    .chain(
                        owner
                            .class_expressions
                            .values()
                            .map(|expression| &expression.class),
                    )
                    .flat_map(|class| &class.members)
                    .filter_map(|member| member.field.as_ref())
                    .any(|source| {
                        (source.span == field.span || source.name_span == field.span)
                            && source.annotation.is_none()
                            && source.readonly
                            && source
                                .initializer
                                .as_ref()
                                .is_some_and(|tokens| !tokens.is_empty())
                    });
                if inferred_literal {
                    if let Type::Literal(text) = &mut field.value {
                        if text.starts_with('\'') && text.ends_with('\'') {
                            if let Some(value) = crate::enum_eval::decode_plain_string(text) {
                                *text = serde_json::to_string(&value).expect("literal string JSON");
                            }
                        }
                    }
                }
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
            origins.insert(field.span.start, origin);
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
        Ok((rendered, origins))
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
                } else if let Some(member) = name.strip_prefix("\0jsx-runtime.") {
                    self.jsx_runtime_type(member, span)?
                } else if name.starts_with("typeof ") {
                    let spelling = crate::parser::source_type_name(name);
                    let query = spelling.strip_prefix("typeof ").expect("value query");
                    if query
                        .rsplit_once('@')
                        .is_some_and(|(_, id)| self.project.modules.contains_key(id))
                    {
                        format!(
                            "typeof {}",
                            self.render(
                                &Type::Named {
                                    name: query.to_string(),
                                    arguments: Vec::new(),
                                },
                                indent,
                                span
                            )?
                        )
                    } else {
                        self.retain(spelling);
                        spelling.to_string()
                    }
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
                    self.retain_type(name);
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
                let class_fields = fields.iter().any(|field| {
                    self.module.class_expressions.values().any(|expression| {
                        expression
                            .class
                            .members
                            .iter()
                            .any(|member| member.span == field.span)
                    })
                });
                let (fields, origins) = self.render_fields_with_origins(fields, indent, span)?;
                if class_fields {
                    callable_objects::render_class_fields(
                        &fields,
                        &origins,
                        self.module,
                        &self.output_options,
                        indent,
                    )
                } else {
                    callable_objects::render_with_indent(&fields, &[], &[], indent)
                }
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
                            ) || matches!(option, Type::Conditional(_))
                                || (matches!(value, Type::Intersection(_))
                                    && matches!(option, Type::Union(_) | Type::Intersection(_)))
                            {
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
            Type::KeyOf(value) => {
                let text = self.render(value, indent, span)?;
                if value.needs_parentheses_after_keyof() {
                    format!("keyof ({text})")
                } else {
                    format!("keyof {text}")
                }
            }
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
