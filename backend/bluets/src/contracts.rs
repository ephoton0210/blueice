// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure, bounded runtime-contract plans for reifiable BlueTS types.

use crate::parser::{Type, TypeField};
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// A data-only value accepted by the pure contract validator.  It deliberately
/// cannot represent a JavaScript getter, proxy, function, or host object.
#[derive(Debug, Clone, PartialEq)]
pub enum ContractValue {
    Null,
    Undefined,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<ContractValue>),
    Object(BTreeMap<String, ContractValue>),
}

impl ContractValue {
    fn category(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Undefined => "undefined",
            Self::Boolean(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contract {
    Null,
    Undefined,
    Boolean,
    Number,
    String,
    Literal(String),
    Array(Box<Contract>),
    Tuple(Vec<Contract>),
    Record(Vec<ContractField>),
    Union(Vec<Contract>),
    Reference(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractField {
    pub name: String,
    pub optional: bool,
    pub contract: Contract,
}

/// An immutable, named contract table.  References are table references, not
/// executable callbacks, so recursive interfaces remain cycle-safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractPlan {
    pub id: String,
    pub root: Contract,
    pub definitions: BTreeMap<String, Contract>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    pub message: String,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ContractError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub expected: String,
    pub observed: String,
}

impl ContractPlan {
    /// Lowers a supported static type into a pure runtime plan.  Callers supply
    /// the checker-approved named type table; unresolved or erased types are
    /// rejected rather than treated as an unchecked escape hatch.
    pub fn from_type(
        id: impl Into<String>,
        value: &Type,
        named_types: &BTreeMap<String, Type>,
    ) -> Result<Self, ContractError> {
        let id = id.into();
        let mut definitions = BTreeMap::new();
        let root = lower(value, named_types, &mut definitions, &mut HashSet::new())?;
        let fingerprint = stable_fingerprint(&id, &root, &definitions);
        Ok(Self {
            id,
            root,
            definitions,
            fingerprint,
        })
    }

    /// Validates a JSON-like value without invoking user code.  Validation is
    /// bounded by a fixed recursion depth and reports a compact data path.
    pub fn validate(&self, value: &ContractValue) -> Result<(), ValidationError> {
        validate_contract(&self.root, value, &self.definitions, "$", 0)
    }
}

const MAX_VALIDATION_DEPTH: usize = 128;

fn lower(
    value: &Type,
    named_types: &BTreeMap<String, Type>,
    definitions: &mut BTreeMap<String, Contract>,
    active: &mut HashSet<String>,
) -> Result<Contract, ContractError> {
    match value {
        Type::Null => Ok(Contract::Null),
        Type::Undefined => Ok(Contract::Undefined),
        Type::Boolean => Ok(Contract::Boolean),
        Type::Number => Ok(Contract::Number),
        Type::String => Ok(Contract::String),
        Type::Literal(value) => Ok(Contract::Literal(value.clone())),
        Type::Array(value) => Ok(Contract::Array(Box::new(lower(
            value,
            named_types,
            definitions,
            active,
        )?))),
        Type::Tuple(values) => values
            .iter()
            .map(|value| lower(value, named_types, definitions, active))
            .collect::<Result<Vec<_>, _>>()
            .map(Contract::Tuple),
        Type::Record(fields) => {
            lower_fields(fields, named_types, definitions, active).map(Contract::Record)
        }
        Type::Union(values) => values
            .iter()
            .map(|value| lower(value, named_types, definitions, active))
            .collect::<Result<Vec<_>, _>>()
            .map(Contract::Union),
        Type::Named { name, arguments } if arguments.is_empty() => {
            if definitions.contains_key(name) || active.contains(name) {
                return Ok(Contract::Reference(name.clone()));
            }
            let Some(named) = named_types.get(name) else {
                return Err(ContractError {
                    message: format!("type `{name}` is not a reifiable named contract"),
                });
            };
            active.insert(name.clone());
            let definition = lower(named, named_types, definitions, active)?;
            active.remove(name);
            definitions.insert(name.clone(), definition);
            Ok(Contract::Reference(name.clone()))
        }
        Type::Any | Type::Unknown | Type::Never => Err(ContractError {
            message: "any, unknown, and never are not automatic runtime contracts".to_string(),
        }),
        Type::Void => Err(ContractError {
            message: "void is not a data-boundary runtime contract".to_string(),
        }),
        Type::Named { name, .. } => Err(ContractError {
            message: format!("generic type `{name}` needs an explicit reifiable contract"),
        }),
        Type::Intersection(_) => Err(ContractError {
            message: "intersections need a developer-supplied runtime contract".to_string(),
        }),
    }
}

fn lower_fields(
    fields: &[TypeField],
    named_types: &BTreeMap<String, Type>,
    definitions: &mut BTreeMap<String, Contract>,
    active: &mut HashSet<String>,
) -> Result<Vec<ContractField>, ContractError> {
    fields
        .iter()
        .map(|field| {
            Ok(ContractField {
                name: field.name.clone(),
                optional: field.optional,
                contract: lower(&field.value, named_types, definitions, active)?,
            })
        })
        .collect()
}

fn validate_contract(
    contract: &Contract,
    value: &ContractValue,
    definitions: &BTreeMap<String, Contract>,
    path: &str,
    depth: usize,
) -> Result<(), ValidationError> {
    if depth > MAX_VALIDATION_DEPTH {
        return Err(ValidationError {
            path: path.to_string(),
            expected: "a contract value within the recursion limit".to_string(),
            observed: value.category().to_string(),
        });
    }
    match contract {
        Contract::Null if matches!(value, ContractValue::Null) => Ok(()),
        Contract::Undefined if matches!(value, ContractValue::Undefined) => Ok(()),
        Contract::Boolean if matches!(value, ContractValue::Boolean(_)) => Ok(()),
        Contract::Number if matches!(value, ContractValue::Number(_)) => Ok(()),
        Contract::String if matches!(value, ContractValue::String(_)) => Ok(()),
        Contract::Literal(expected) if matches_literal(expected, value) => Ok(()),
        Contract::Array(item) => {
            let ContractValue::Array(values) = value else {
                return mismatch(path, "array", value);
            };
            for (index, item_value) in values.iter().enumerate() {
                validate_contract(
                    item,
                    item_value,
                    definitions,
                    &format!("{path}[{index}]"),
                    depth + 1,
                )?;
            }
            Ok(())
        }
        Contract::Tuple(items) => {
            let ContractValue::Array(values) = value else {
                return mismatch(path, "tuple", value);
            };
            if values.len() != items.len() {
                return Err(ValidationError {
                    path: path.to_string(),
                    expected: format!("tuple of length {}", items.len()),
                    observed: format!("array of length {}", values.len()),
                });
            }
            for (index, (item, item_value)) in items.iter().zip(values).enumerate() {
                validate_contract(
                    item,
                    item_value,
                    definitions,
                    &format!("{path}[{index}]"),
                    depth + 1,
                )?;
            }
            Ok(())
        }
        Contract::Record(fields) => {
            let ContractValue::Object(values) = value else {
                return mismatch(path, "object", value);
            };
            for field in fields {
                match values.get(&field.name) {
                    Some(field_value) => validate_contract(
                        &field.contract,
                        field_value,
                        definitions,
                        &format!("{path}.{}", field.name),
                        depth + 1,
                    )?,
                    None if field.optional => {}
                    None => {
                        return Err(ValidationError {
                            path: format!("{path}.{}", field.name),
                            expected: "required property".to_string(),
                            observed: "missing".to_string(),
                        })
                    }
                }
            }
            Ok(())
        }
        Contract::Union(options) => {
            if options.iter().any(|option| {
                validate_contract(option, value, definitions, path, depth + 1).is_ok()
            }) {
                Ok(())
            } else {
                Err(ValidationError {
                    path: path.to_string(),
                    expected: "a member of the declared union".to_string(),
                    observed: value.category().to_string(),
                })
            }
        }
        Contract::Reference(name) => {
            let Some(target) = definitions.get(name) else {
                return Err(ValidationError {
                    path: path.to_string(),
                    expected: format!("defined contract `{name}`"),
                    observed: value.category().to_string(),
                });
            };
            validate_contract(target, value, definitions, path, depth + 1)
        }
        _ => mismatch(path, contract_label(contract), value),
    }
}

fn mismatch(
    path: &str,
    expected: impl Into<String>,
    value: &ContractValue,
) -> Result<(), ValidationError> {
    Err(ValidationError {
        path: path.to_string(),
        expected: expected.into(),
        observed: value.category().to_string(),
    })
}

fn matches_literal(expected: &str, value: &ContractValue) -> bool {
    match value {
        ContractValue::String(value) => {
            (expected.starts_with('\'') || expected.starts_with('\"'))
                && expected.get(1..expected.len().saturating_sub(1)) == Some(value)
        }
        ContractValue::Number(value) => expected
            .parse::<f64>()
            .is_ok_and(|expected| expected == *value),
        ContractValue::Boolean(value) => {
            matches!((expected, value), ("true", true) | ("false", false))
        }
        _ => false,
    }
}

fn contract_label(contract: &Contract) -> String {
    match contract {
        Contract::Null => "null",
        Contract::Undefined => "undefined",
        Contract::Boolean => "boolean",
        Contract::Number => "number",
        Contract::String => "string",
        Contract::Literal(value) => value,
        Contract::Array(_) => "array",
        Contract::Tuple(_) => "tuple",
        Contract::Record(_) => "object",
        Contract::Union(_) => "union",
        Contract::Reference(name) => name,
    }
    .to_string()
}

fn stable_fingerprint(
    id: &str,
    root: &Contract,
    definitions: &BTreeMap<String, Contract>,
) -> String {
    let value = format!("blue-ts-contract-v1|{id}|{root:?}|{definitions:?}");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("bts-contract-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_a_required_record_field_at_a_bounded_path() {
        let plan = ContractPlan::from_type(
            "User",
            &Type::Record(vec![TypeField {
                name: "id".to_string(),
                optional: false,
                value: Type::String,
                span: crate::diagnostic::SourceSpan::new("test", 0, 0),
            }]),
            &BTreeMap::new(),
        )
        .unwrap();
        let error = plan
            .validate(&ContractValue::Object(BTreeMap::new()))
            .unwrap_err();
        assert_eq!(error.path, "$.id");
        assert_eq!(error.observed, "missing");
    }

    #[test]
    fn rejects_unreifiable_any() {
        let error = ContractPlan::from_type("unsafe", &Type::Any, &BTreeMap::new()).unwrap_err();
        assert!(error.message.contains("not automatic"));
    }
}
