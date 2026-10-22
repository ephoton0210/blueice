// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Index signatures contribute their actual element/property evidence.
use super::*;

impl collect::Collector<'_> {
    pub(super) fn collect_indices(&mut self, template: &Type, actual: &Type, contra: bool) -> bool {
        let Type::IndexedRecord { object, indices } = template else {
            return false;
        };
        if let Type::Readonly(actual) = actual {
            return self.collect_indices(template, actual, contra);
        }
        match actual {
            Type::Record(fields) | Type::CallableRecord { fields, .. } => {
                self.collect(object, actual, contra);
                for index in indices {
                    for field in fields {
                        if super::super::computed_keys::applies(&field.name, &index.key) {
                            self.collect(&index.value, &field.value, contra);
                        }
                    }
                }
            }
            Type::Array(element) => {
                for index in indices.iter().filter(|index| index.key == Type::Number) {
                    self.collect(&index.value, element, contra);
                }
            }
            Type::Tuple(values) => {
                for index in indices.iter().filter(|index| index.key == Type::Number) {
                    for value in values {
                        self.collect(&index.value, &value.annotation, contra);
                    }
                }
            }
            Type::IndexedRecord {
                object: actual_object,
                indices: actual_indices,
            } => {
                self.collect(object, actual_object, contra);
                for index in indices {
                    for actual in actual_indices
                        .iter()
                        .filter(|actual| actual.key == index.key)
                    {
                        self.collect(&index.value, &actual.value, contra);
                    }
                }
            }
            Type::String | Type::Literal(_)
                if matches!(actual, Type::String)
                    || matches!(actual, Type::Literal(text) if text.starts_with(['\'', '"', '`'])) =>
            {
                for index in indices.iter().filter(|index| index.key == Type::Number) {
                    self.collect(&index.value, &Type::String, contra);
                }
            }
            _ => return false,
        }
        true
    }
}
