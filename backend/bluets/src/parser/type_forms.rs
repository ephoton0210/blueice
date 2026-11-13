// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static operator forms retain syntax, lexical binders and declaration origins.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variance {
    In,
    Out,
    InOut,
}

impl Variance {
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::In => "in ",
            Self::Out => "out ",
            Self::InOut => "in out ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSignature {
    pub name: String,
    pub key: Type,
    pub value: Type,
    /// Mapped keys retain their original domain (and aliases) for `keyof`.
    /// An explicit string index also admits numeric keys; a mapped string
    /// domain does not acquire that additional constituent.
    pub mapped_domain: Option<Type>,
    pub readonly: bool,
    pub span: SourceSpan,
    pub key_span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalType {
    pub check: Type,
    pub extends: Type,
    pub when_true: Type,
    pub when_false: Type,
    pub span: SourceSpan,
    pub when_true_span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedModifier {
    Preserve,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedType {
    pub parameter: TypeParameter,
    pub name_type: Option<Type>,
    pub value: Type,
    pub readonly: MappedModifier,
    pub optional: MappedModifier,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateLiteralType {
    pub head: String,
    pub spans: Vec<(Type, String)>,
    pub span: SourceSpan,
    /// Generated chunks retain escaped semantic units until presentation.
    pub generated: bool,
}

impl Type {
    pub(crate) fn inference_barrier(value: Type) -> Type {
        if value.inference_barrier_target() {
            Self::InferenceBarrier(Box::new(value))
        } else {
            value
        }
    }

    fn inference_barrier_target(&self) -> bool {
        match self {
            Self::InferenceBarrier(_) => false,
            Self::Record(fields) => !fields.is_empty(),
            Self::Union(parts) | Self::Intersection(parts) => {
                parts.iter().any(Self::inference_barrier_target)
            }
            Self::TemplateLiteral(template) => template
                .spans
                .iter()
                .any(|(value, _)| value.inference_barrier_target()),
            Self::Named { .. }
            | Self::Array(_)
            | Self::Readonly(_)
            | Self::Tuple(_)
            | Self::IndexedRecord { .. }
            | Self::CallableRecord { .. }
            | Self::Function { .. }
            | Self::GenericFunction { .. }
            | Self::KeyOf(_)
            | Self::IndexedAccess { .. }
            | Self::Conditional(_)
            | Self::Infer(_)
            | Self::Mapped(_) => true,
            Self::StringMapping(_) => true,
            _ => false,
        }
    }

    pub(crate) fn object_type(&self) -> &Self {
        let mut current = self;
        loop {
            match current {
                Self::IndexedRecord { object, .. } | Self::InferenceBarrier(object) => {
                    current = object
                }
                _ => return current,
            }
        }
    }

    pub(crate) fn object_type_mut(&mut self) -> &mut Self {
        let mut current = self;
        loop {
            match current {
                Self::IndexedRecord { object, .. } | Self::InferenceBarrier(object) => {
                    current = object
                }
                _ => return current,
            }
        }
    }

    /// Low-precedence operands need an explicit grouping after a key operator.
    pub(crate) fn needs_parentheses_after_keyof(&self) -> bool {
        matches!(
            self,
            Self::Union(_)
                | Self::Intersection(_)
                | Self::Conditional(_)
                | Self::Function { .. }
                | Self::GenericFunction { .. }
        ) || matches!(self, Self::CallableRecord {fields,signatures}
                if fields.is_empty() && signatures.len()==1 && signatures[0].constructor_arrow)
    }

    /// A shallow visitor, used where operator children have no local binding.
    pub(crate) fn operator_children(&self) -> Option<Vec<&Type>> {
        Some(match self {
            Self::Readonly(value) | Self::InferenceBarrier(value) => vec![value],
            Self::StringMapping(value) => vec![&value.operand],
            Self::IndexedRecord { object, indices } => std::iter::once(object.as_ref())
                .chain(indices.iter().flat_map(|index| {
                    [&index.key, &index.value]
                        .into_iter()
                        .chain(index.mapped_domain.iter())
                }))
                .collect(),
            Self::Conditional(value) => vec![
                &value.check,
                &value.extends,
                &value.when_true,
                &value.when_false,
            ],
            Self::Infer(parameter) => parameter.constraint.iter().collect(),
            Self::Mapped(value) => value
                .parameter
                .constraint
                .iter()
                .chain(value.name_type.iter())
                .chain(std::iter::once(&value.value))
                .collect(),
            Self::TemplateLiteral(value) => value.spans.iter().map(|(value, _)| value).collect(),
            _ => return None,
        })
    }

    /// Recursively collect `infer` declarations from an extends pattern.
    pub(crate) fn infer_parameters(&self) -> Vec<TypeParameter> {
        let mut parameters = Vec::new();
        self.collect_infer(&mut parameters);
        parameters
    }

    fn collect_infer(&self, into: &mut Vec<TypeParameter>) {
        match self {
            Self::Infer(parameter) => into.push((**parameter).clone()),
            Self::Array(value) | Self::KeyOf(value) => value.collect_infer(into),
            Self::Tuple(values) => values
                .iter()
                .for_each(|value| value.annotation.collect_infer(into)),
            Self::Record(fields) | Self::CallableRecord { fields, .. } => {
                fields.iter().for_each(|field| {
                    field.value.collect_infer(into);
                    if let Some(value) = &field.accessor_write_type {
                        value.collect_infer(into);
                    }
                });
                if let Self::CallableRecord { signatures, .. } = self {
                    for signature in signatures {
                        signature.function_type().collect_infer(into);
                    }
                }
            }
            Self::Function { parameters, result }
            | Self::GenericFunction {
                parameters, result, ..
            } => {
                for parameter in parameters {
                    if let Some(value) = &parameter.annotation {
                        value.collect_infer(into);
                    }
                }
                result.collect_infer(into);
            }
            Self::Union(values)
            | Self::Intersection(values)
            | Self::Named {
                arguments: values, ..
            } => values.iter().for_each(|value| value.collect_infer(into)),
            Self::IndexedAccess { object, index, .. } => {
                object.collect_infer(into);
                index.collect_infer(into);
            }
            _ => {
                if let Some(children) = self.operator_children() {
                    children.iter().for_each(|value| value.collect_infer(into));
                }
            }
        }
    }

    /// Transform children with the names bound in each child's lexical region.
    pub(crate) fn map_operator(
        &self,
        mut map: impl FnMut(&Type, &[String]) -> Type,
    ) -> Option<Type> {
        Some(match self {
            Self::Readonly(value) => Self::Readonly(Box::new(map(value, &[]))),
            Self::InferenceBarrier(value) => Self::InferenceBarrier(Box::new(map(value, &[]))),
            Self::StringMapping(value) => Self::StringMapping(Box::new(StringMappingType {
                operand: map(&value.operand, &[]),
                ..(**value).clone()
            })),
            Self::IndexedRecord { object, indices } => Self::IndexedRecord {
                object: Box::new(map(object, &[])),
                indices: indices
                    .iter()
                    .map(|index| IndexSignature {
                        key: map(&index.key, &[]),
                        value: map(&index.value, &[]),
                        mapped_domain: index.mapped_domain.as_ref().map(|value| map(value, &[])),
                        ..index.clone()
                    })
                    .collect(),
            },
            Self::Conditional(value) => {
                let bound = value
                    .extends
                    .infer_parameters()
                    .into_iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>();
                Self::Conditional(Box::new(ConditionalType {
                    check: map(&value.check, &[]),
                    extends: map(&value.extends, &[]),
                    when_true: map(&value.when_true, &bound),
                    when_false: map(&value.when_false, &[]),
                    span: value.span.clone(),
                    when_true_span: value.when_true_span.clone(),
                }))
            }
            Self::Infer(parameter) => Self::Infer(Box::new(TypeParameter {
                constraint: parameter.constraint.as_ref().map(|value| map(value, &[])),
                ..(**parameter).clone()
            })),
            Self::Mapped(value) => {
                let bound = vec![value.parameter.name.clone()];
                Self::Mapped(Box::new(MappedType {
                    parameter: TypeParameter {
                        constraint: value
                            .parameter
                            .constraint
                            .as_ref()
                            .map(|value| map(value, &[])),
                        ..value.parameter.clone()
                    },
                    name_type: value.name_type.as_ref().map(|value| map(value, &bound)),
                    value: map(&value.value, &bound),
                    ..(**value).clone()
                }))
            }
            Self::TemplateLiteral(value) => Self::TemplateLiteral(TemplateLiteralType {
                spans: value
                    .spans
                    .iter()
                    .map(|(value, tail)| (map(value, &[]), tail.clone()))
                    .collect(),
                ..value.clone()
            }),
            _ => return None,
        })
    }

    pub(crate) fn operator_text(&self, render: impl Fn(&Type) -> String) -> Option<String> {
        Some(match self {
            Self::InferenceBarrier(value) => format!("NoInfer<{}>", render(value)),
            Self::StringMapping(value) => {
                format!("{}<{}>", value.operation.name(), render(&value.operand))
            }
            Self::Readonly(value) => format!("readonly {}", render(value)),
            Self::IndexedRecord { object, indices } => {
                let members = indices
                    .iter()
                    .map(|index| {
                        format!(
                            "{}[{}: {}]: {};",
                            if index.readonly { "readonly " } else { "" },
                            index.name,
                            render(&index.key),
                            render(&index.value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let object = render(object);
                format!(
                    "{{ {}{} }}",
                    members,
                    object.trim().trim_start_matches('{').trim_end_matches('}')
                )
            }
            Self::Conditional(value) => format!(
                "{} extends {} ? {} : {}",
                render(&value.check),
                render(&value.extends),
                render(&value.when_true),
                render(&value.when_false)
            ),
            Self::Infer(parameter) => format!(
                "infer {}{}",
                parameter.name,
                parameter
                    .constraint
                    .as_ref()
                    .map(|value| format!(" extends {}", render(value)))
                    .unwrap_or_default()
            ),
            Self::Mapped(value) => {
                let readonly = match value.readonly {
                    MappedModifier::Preserve => "",
                    MappedModifier::Add => "readonly ",
                    MappedModifier::Remove => "-readonly ",
                };
                let optional = match value.optional {
                    MappedModifier::Preserve => "",
                    MappedModifier::Add => "?",
                    MappedModifier::Remove => "-?",
                };
                format!(
                    "{{ {readonly}[{} in {}{}]{optional}: {}; }}",
                    value.parameter.name,
                    value
                        .parameter
                        .constraint
                        .as_ref()
                        .map(&render)
                        .unwrap_or_default(),
                    value
                        .name_type
                        .as_ref()
                        .map(|value| format!(" as {}", render(value)))
                        .unwrap_or_default(),
                    render(&value.value)
                )
            }
            Self::TemplateLiteral(value) => {
                let chunk = |raw: &str| {
                    if value.generated {
                        crate::utf16::Utf16String::parse_template_chunk(raw)
                            .expect("generated template units")
                            .native_template_chunk()
                    } else {
                        raw.to_string()
                    }
                };
                let mut text = format!("`{}", chunk(&value.head));
                for (value, tail) in &value.spans {
                    text.push_str(&format!("${{{}}}{}", render(value), chunk(tail)));
                }
                text.push('`');
                text
            }
            _ => return None,
        })
    }
}
