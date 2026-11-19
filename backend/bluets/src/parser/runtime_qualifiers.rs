// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime-origin evidence stays distinct from a value's semantic type.

use super::Type;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeQualifiers {
    pub(crate) opaque: bool,
    pub(crate) may_readonly: bool,
}

impl RuntimeQualifiers {
    pub(crate) fn join(self, other: Self) -> Self {
        Self {
            opaque: self.opaque || other.opaque,
            may_readonly: self.may_readonly || other.may_readonly,
        }
    }

    pub(crate) fn is_plain(self) -> bool {
        !self.opaque && !self.may_readonly
    }
}

impl Type {
    pub(crate) fn semantic_type(&self) -> &Type {
        let mut value = self;
        while let Self::RuntimeQualified { semantic, .. } = value {
            value = semantic;
        }
        value
    }

    pub(crate) fn runtime_qualifiers(&self) -> RuntimeQualifiers {
        match self {
            Self::RuntimeQualified {
                opaque,
                may_readonly,
                ..
            } => RuntimeQualifiers {
                opaque: *opaque,
                may_readonly: *may_readonly,
            },
            Self::Union(parts) | Self::Intersection(parts) => parts
                .iter()
                .fold(RuntimeQualifiers::default(), |qualifiers, part| {
                    qualifiers.join(part.runtime_qualifiers())
                }),
            _ => RuntimeQualifiers::default(),
        }
    }

    pub(crate) fn with_runtime_qualifiers(self, qualifiers: RuntimeQualifiers) -> Type {
        if qualifiers.is_plain() {
            return self;
        }
        let qualifiers = qualifiers.join(self.runtime_qualifiers());
        let semantic = match self {
            Self::RuntimeQualified { semantic, .. } => semantic,
            value => Box::new(value),
        };
        Self::RuntimeQualified {
            semantic,
            opaque: qualifiers.opaque,
            may_readonly: qualifiers.may_readonly,
        }
    }

    /// Retain value facts at matching structural positions without importing
    /// the source's semantic type or changing either callable's lexical binders.
    /// Named aliases remain opaque here; this operation never expands a type.
    pub(crate) fn retaining_runtime_qualifiers(self, source: &Type) -> Type {
        let mut qualifiers = self.runtime_qualifiers().join(source.runtime_qualifiers());
        let mut value = match self {
            Self::RuntimeQualified { semantic, .. } => *semantic,
            value => value,
        };
        match (&mut value, source.semantic_type()) {
            (Self::Named { .. }, _) => {
                // An opaque alias keeps its authored semantic identity. Its
                // closed value facts must survive until lookup expands it.
                qualifiers = qualifiers.join(source.descendant_runtime_qualifiers());
            }
            (
                Self::Function { result, .. } | Self::GenericFunction { result, .. },
                Self::Function { result: source, .. }
                | Self::GenericFunction { result: source, .. },
            ) => {
                **result = result.as_ref().clone().retaining_runtime_qualifiers(source);
            }
            (Self::Array(value), Self::Array(source))
            | (Self::Readonly(value), Self::Readonly(source)) => {
                **value = value.as_ref().clone().retaining_runtime_qualifiers(source);
            }
            (Self::Union(values), Self::Union(sources))
            | (Self::Intersection(values), Self::Intersection(sources)) => {
                for (value, source) in values.iter_mut().zip(sources) {
                    *value = value.clone().retaining_runtime_qualifiers(source);
                }
            }
            (
                Self::IndexedRecord { object, indices },
                Self::IndexedRecord {
                    object: source,
                    indices: sources,
                },
            ) => {
                **object = object.as_ref().clone().retaining_runtime_qualifiers(source);
                for (index, source) in indices.iter_mut().zip(sources) {
                    index.value = index
                        .value
                        .clone()
                        .retaining_runtime_qualifiers(&source.value);
                }
            }
            (Self::Tuple(values), Self::Tuple(sources)) => {
                for (value, source) in values.iter_mut().zip(sources) {
                    value.annotation = value
                        .annotation
                        .clone()
                        .retaining_runtime_qualifiers(&source.annotation);
                }
            }
            (
                Self::Record(fields) | Self::CallableRecord { fields, .. },
                Self::Record(sources)
                | Self::CallableRecord {
                    fields: sources, ..
                },
            ) => {
                for field in fields {
                    if let Some(source) = sources.iter().find(|source| source.name == field.name) {
                        field.value = field
                            .value
                            .clone()
                            .retaining_runtime_qualifiers(&source.value);
                    }
                }
            }
            _ => {}
        }
        if let (
            Self::CallableRecord { signatures, .. },
            Self::CallableRecord {
                signatures: sources,
                ..
            },
        ) = (&mut value, source.semantic_type())
        {
            for (signature, source) in signatures.iter_mut().zip(sources) {
                signature.result = signature
                    .result
                    .clone()
                    .retaining_runtime_qualifiers(&source.result);
            }
        }
        value.with_runtime_qualifiers(qualifiers)
    }

    fn descendant_runtime_qualifiers(&self) -> RuntimeQualifiers {
        let mut result = self.runtime_qualifiers();
        match self {
            Self::RuntimeQualified { semantic, .. } => {
                result = result.join(semantic.descendant_runtime_qualifiers());
            }
            Self::Function { result: value, .. }
            | Self::GenericFunction { result: value, .. }
            | Self::Array(value) => result = result.join(value.descendant_runtime_qualifiers()),
            Self::Record(fields) | Self::CallableRecord { fields, .. } => {
                for field in fields {
                    result = result.join(field.value.descendant_runtime_qualifiers());
                }
                if let Self::CallableRecord { signatures, .. } = self {
                    for signature in signatures {
                        result = result.join(signature.result.descendant_runtime_qualifiers());
                    }
                }
            }
            Self::Tuple(elements) => {
                for element in elements {
                    result = result.join(element.annotation.descendant_runtime_qualifiers());
                }
            }
            Self::Named { arguments, .. }
            | Self::Union(arguments)
            | Self::Intersection(arguments) => {
                for value in arguments {
                    result = result.join(value.descendant_runtime_qualifiers());
                }
            }
            value => {
                if let Some(children) = value.operator_children() {
                    for child in children {
                        result = result.join(child.descendant_runtime_qualifiers());
                    }
                }
            }
        }
        result
    }

    /// Public type presentation omits internal runtime-origin bookkeeping.
    pub(crate) fn semantic_clone(&self) -> Type {
        self.semantic_type()
            .map_children(|child, _| child.semantic_clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_joins_retain_nested_union_and_readonly_origins() {
        let origin = Type::Union(vec![
            Type::Readonly(Box::new(Type::Array(Box::new(
                Type::Number.with_runtime_qualifiers(RuntimeQualifiers {
                    opaque: true,
                    may_readonly: true,
                }),
            )))),
            Type::String,
        ]);
        let merged = origin
            .semantic_clone()
            .retaining_runtime_qualifiers(&origin);
        let Type::Union(parts) = merged else {
            panic!("union shape lost")
        };
        let Type::Readonly(array) = &parts[0] else {
            panic!("readonly shape lost")
        };
        let Type::Array(element) = array.as_ref() else {
            panic!("array shape lost")
        };
        assert!(element.runtime_qualifiers().opaque);
        assert!(element.runtime_qualifiers().may_readonly);
        assert!(parts[1].runtime_qualifiers().is_plain());
    }
}
