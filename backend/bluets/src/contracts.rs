// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure, bounded runtime-contract plans for reifiable BlueTS types.

use crate::parser::{TupleTypeElement, Type, TypeField};
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
    OptionalTuple {
        items: Vec<Contract>,
        required: usize,
    },
    RestTuple {
        items: Vec<Contract>,
        required: usize,
        rest: Box<Contract>,
        suffix: Vec<Contract>,
    },
    Record(Vec<ContractField>),
    Union(Vec<Contract>),
    /// Every component must validate. This keeps inherited record contracts
    /// pure and data-only while preserving TypeScript interface heritage.
    Intersection(Vec<Contract>),
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

/// Explicit resource limits for pure contract validation. These limits apply
/// before a value is accepted by a contract and never invoke user code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationLimits {
    /// Maximum number of contract edges between the root and a visited value.
    pub max_depth: usize,
    /// Maximum number of values in any one array or object.
    pub max_collection_entries: usize,
    /// Maximum number of values visited across the entire validation attempt.
    pub max_nodes: usize,
    /// Maximum UTF-8 byte length of a string value.
    pub max_string_bytes: usize,
}

/// Work performed by one pure validation attempt, including a rejected node.
/// A boundary owner can charge this to the exact caller without retaining the
/// inspected value or exposing it in a diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationUsage {
    pub visited_nodes: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_collection_entries: 10_000,
            max_nodes: 100_000,
            max_string_bytes: 1_048_576,
        }
    }
}

impl ContractPlan {
    /// Owned heap payload retained by this plan, excluding allocator and
    /// BTreeMap node overhead. This is a checked accounting measure, not RSS.
    pub(crate) fn owned_heap_payload_bytes(&self) -> Option<usize> {
        let mut bytes = self
            .id
            .capacity()
            .checked_add(self.fingerprint.capacity())?;
        bytes = bytes.checked_add(contract_heap_payload_bytes(&self.root)?)?;
        bytes = bytes.checked_add(
            self.definitions
                .len()
                .checked_mul(std::mem::size_of::<(String, Contract)>())?,
        )?;
        for (name, contract) in &self.definitions {
            bytes = bytes.checked_add(name.capacity())?;
            bytes = bytes.checked_add(contract_heap_payload_bytes(contract)?)?;
        }
        Some(bytes)
    }

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

    /// Validates a JSON-like value without invoking user code under the
    /// default resource limits.
    pub fn validate(&self, value: &ContractValue) -> Result<(), ValidationError> {
        self.validate_with_limits(value, ValidationLimits::default())
    }

    /// Validates a JSON-like value without invoking user code. The caller can
    /// set explicit depth, collection, fuel, and string-byte bounds for a
    /// particular trust boundary.
    pub fn validate_with_limits(
        &self,
        value: &ContractValue,
        limits: ValidationLimits,
    ) -> Result<(), ValidationError> {
        self.validate_with_limits_metered(value, limits).0
    }

    /// Returns the bounded work done even when validation fails.
    pub fn validate_with_limits_metered(
        &self,
        value: &ContractValue,
        limits: ValidationLimits,
    ) -> (Result<(), ValidationError>, ValidationUsage) {
        let mut state = ValidationState {
            limits,
            visited_nodes: 0,
            attempted_nodes: 0,
        };
        let result = validate_contract(&self.root, value, &self.definitions, "$", 0, &mut state);
        (
            result,
            ValidationUsage {
                visited_nodes: state.attempted_nodes,
            },
        )
    }
}

fn contract_heap_payload_bytes(contract: &Contract) -> Option<usize> {
    match contract {
        Contract::Null
        | Contract::Undefined
        | Contract::Boolean
        | Contract::Number
        | Contract::String => Some(0),
        Contract::Literal(value) | Contract::Reference(value) => Some(value.capacity()),
        Contract::Array(item) => {
            std::mem::size_of::<Contract>().checked_add(contract_heap_payload_bytes(item)?)
        }
        Contract::Tuple(items)
        | Contract::OptionalTuple { items, .. }
        | Contract::Union(items)
        | Contract::Intersection(items) => {
            let mut bytes = items
                .capacity()
                .checked_mul(std::mem::size_of::<Contract>())?;
            for item in items {
                bytes = bytes.checked_add(contract_heap_payload_bytes(item)?)?;
            }
            Some(bytes)
        }
        Contract::RestTuple {
            items,
            rest,
            suffix,
            ..
        } => {
            let mut bytes = items
                .capacity()
                .checked_mul(std::mem::size_of::<Contract>())?;
            for item in items {
                bytes = bytes.checked_add(contract_heap_payload_bytes(item)?)?;
            }
            bytes = bytes.checked_add(
                suffix
                    .capacity()
                    .checked_mul(std::mem::size_of::<Contract>())?,
            )?;
            for item in suffix {
                bytes = bytes.checked_add(contract_heap_payload_bytes(item)?)?;
            }
            bytes
                .checked_add(std::mem::size_of::<Contract>())?
                .checked_add(contract_heap_payload_bytes(rest)?)
        }
        Contract::Record(fields) => {
            let mut bytes = fields
                .capacity()
                .checked_mul(std::mem::size_of::<ContractField>())?;
            for field in fields {
                bytes = bytes.checked_add(field.name.capacity())?;
                bytes = bytes.checked_add(contract_heap_payload_bytes(&field.contract)?)?;
            }
            Some(bytes)
        }
    }
}

struct ValidationState {
    limits: ValidationLimits,
    visited_nodes: usize,
    attempted_nodes: usize,
}

impl ValidationState {
    fn observe(
        &mut self,
        value: &ContractValue,
        path: &str,
        depth: usize,
    ) -> Result<(), ValidationError> {
        self.attempted_nodes = self.attempted_nodes.saturating_add(1);
        if depth > self.limits.max_depth {
            return Err(limit_error(
                path,
                format!("a contract value within depth {}", self.limits.max_depth),
                value.category(),
            ));
        }
        if self.visited_nodes >= self.limits.max_nodes {
            return Err(limit_error(
                path,
                format!("a contract value within fuel {}", self.limits.max_nodes),
                value.category(),
            ));
        }
        self.visited_nodes += 1;
        match value {
            ContractValue::String(value) if value.len() > self.limits.max_string_bytes => {
                Err(limit_error(
                    path,
                    format!("string within {} bytes", self.limits.max_string_bytes),
                    format!("string of {} bytes", value.len()),
                ))
            }
            ContractValue::Array(values) if values.len() > self.limits.max_collection_entries => {
                Err(limit_error(
                    path,
                    format!(
                        "array within {} entries",
                        self.limits.max_collection_entries
                    ),
                    format!("array of {} entries", values.len()),
                ))
            }
            ContractValue::Object(values) if values.len() > self.limits.max_collection_entries => {
                Err(limit_error(
                    path,
                    format!(
                        "object within {} entries",
                        self.limits.max_collection_entries
                    ),
                    format!("object of {} entries", values.len()),
                ))
            }
            _ => Ok(()),
        }
    }
}

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
        Type::Tuple(values) => {
            let values = expand_contract_tuple_spreads(values, named_types, active, &mut 1_024)?;
            if values.iter().filter(|value| value.rest).count() > 1 {
                return Err(ContractError {
                    message: "a tuple contract cannot contain multiple rest elements".to_string(),
                });
            }
            let rest_index = values.iter().position(|value| value.rest);
            let fixed = &values[..rest_index.unwrap_or(values.len())];
            let items = fixed
                .iter()
                .map(|value| {
                    let item = lower(&value.annotation, named_types, definitions, active)?;
                    Ok(if value.optional {
                        Contract::Union(vec![item, Contract::Undefined])
                    } else {
                        item
                    })
                })
                .collect::<Result<Vec<_>, ContractError>>()?;
            if let Some(rest_index) = rest_index {
                let rest = &values[rest_index];
                let Type::Array(element) = &rest.annotation else {
                    return Err(ContractError {
                        message: "tuple rest contract requires an array type".to_string(),
                    });
                };
                let suffix = values[rest_index + 1..]
                    .iter()
                    .map(|value| lower(&value.annotation, named_types, definitions, active))
                    .collect::<Result<Vec<_>, ContractError>>()?;
                Ok(Contract::RestTuple {
                    items,
                    required: fixed.iter().filter(|value| !value.optional).count() + suffix.len(),
                    rest: Box::new(lower(element, named_types, definitions, active)?),
                    suffix,
                })
            } else if values.iter().any(|value| value.optional) {
                Ok(Contract::OptionalTuple {
                    items,
                    required: values.iter().filter(|value| !value.optional).count(),
                })
            } else {
                Ok(Contract::Tuple(items))
            }
        }
        Type::Record(fields) => {
            lower_fields(fields, named_types, definitions, active).map(Contract::Record)
        }
        Type::Union(values) => values
            .iter()
            .map(|value| lower(value, named_types, definitions, active))
            .collect::<Result<Vec<_>, _>>()
            .map(Contract::Union),
        Type::Intersection(values) => values
            .iter()
            .map(|value| lower(value, named_types, definitions, active))
            .collect::<Result<Vec<_>, _>>()
            .map(Contract::Intersection),
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
        Type::Function { .. } => Err(ContractError {
            message: "function members are not data-boundary runtime contracts".to_string(),
        }),
        Type::Void => Err(ContractError {
            message: "void is not a data-boundary runtime contract".to_string(),
        }),
        Type::Named { name, .. } => Err(ContractError {
            message: format!("generic type `{name}` needs an explicit reifiable contract"),
        }),
    }
}

fn expand_contract_tuple_spreads(
    values: &[TupleTypeElement],
    named_types: &BTreeMap<String, Type>,
    active: &mut HashSet<String>,
    remaining: &mut usize,
) -> Result<Vec<TupleTypeElement>, ContractError> {
    let mut expanded = Vec::new();
    for value in values {
        *remaining = remaining.checked_sub(1).ok_or_else(|| ContractError {
            message: "tuple spread contract exceeds the expansion limit".to_string(),
        })?;
        if !value.rest || matches!(value.annotation, Type::Array(_)) {
            expanded.push(value.clone());
            continue;
        }
        let Type::Named { name, arguments } = &value.annotation else {
            return Err(ContractError {
                message: "tuple spread contract requires a concrete tuple or array type"
                    .to_string(),
            });
        };
        if !arguments.is_empty() || !active.insert(name.clone()) {
            return Err(ContractError {
                message: "tuple spread contract is generic or cyclic".to_string(),
            });
        }
        let replacement = match named_types.get(name) {
            Some(Type::Tuple(items)) => {
                expand_contract_tuple_spreads(items, named_types, active, remaining)
            }
            Some(Type::Array(_)) => Ok(vec![TupleTypeElement {
                annotation: named_types[name].clone(),
                optional: false,
                label: None,
                rest: true,
            }]),
            Some(Type::Named { .. }) => expand_contract_tuple_spreads(
                &[TupleTypeElement {
                    annotation: named_types[name].clone(),
                    optional: false,
                    label: None,
                    rest: true,
                }],
                named_types,
                active,
                remaining,
            ),
            _ => Err(ContractError {
                message: "tuple spread contract requires a concrete tuple or array type"
                    .to_string(),
            }),
        };
        active.remove(name);
        expanded.extend(replacement?);
    }
    if expanded.iter().filter(|value| value.rest).count() > 1 {
        return Err(ContractError {
            message: "a tuple contract cannot contain multiple rest elements".to_string(),
        });
    }
    crate::parser::require_tuple_positions_before_suffix(&mut expanded);
    Ok(expanded)
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
    state: &mut ValidationState,
) -> Result<(), ValidationError> {
    state.observe(value, path, depth)?;
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
                    state,
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
                    state,
                )?;
            }
            Ok(())
        }
        Contract::OptionalTuple { items, required } => {
            let ContractValue::Array(values) = value else {
                return mismatch(path, "tuple", value);
            };
            if values.len() < *required || values.len() > items.len() {
                return Err(ValidationError {
                    path: path.to_string(),
                    expected: format!("tuple of length {}..={}", required, items.len()),
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
                    state,
                )?;
            }
            Ok(())
        }
        Contract::RestTuple {
            items,
            required,
            rest,
            suffix,
        } => {
            let ContractValue::Array(values) = value else {
                return mismatch(path, "tuple", value);
            };
            if values.len() < *required {
                return Err(ValidationError {
                    path: path.to_string(),
                    expected: format!("tuple of length at least {required}"),
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
                    state,
                )?;
            }
            let suffix_start = values.len() - suffix.len();
            for (index, item_value) in values
                .iter()
                .enumerate()
                .take(suffix_start)
                .skip(items.len())
            {
                validate_contract(
                    rest,
                    item_value,
                    definitions,
                    &format!("{path}[{index}]"),
                    depth + 1,
                    state,
                )?;
            }
            for (index, (item, item_value)) in
                suffix.iter().zip(&values[suffix_start..]).enumerate()
            {
                let index = suffix_start + index;
                validate_contract(
                    item,
                    item_value,
                    definitions,
                    &format!("{path}[{index}]"),
                    depth + 1,
                    state,
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
                        state,
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
            for option in options {
                if validate_contract(option, value, definitions, path, depth + 1, state).is_ok() {
                    return Ok(());
                }
            }
            Err(ValidationError {
                path: path.to_string(),
                expected: "a member of the declared union".to_string(),
                observed: value.category().to_string(),
            })
        }
        Contract::Intersection(parts) => {
            for part in parts {
                validate_contract(part, value, definitions, path, depth + 1, state)?;
            }
            Ok(())
        }
        Contract::Reference(name) => {
            let Some(target) = definitions.get(name) else {
                return Err(ValidationError {
                    path: path.to_string(),
                    expected: format!("defined contract `{name}`"),
                    observed: value.category().to_string(),
                });
            };
            validate_contract(target, value, definitions, path, depth + 1, state)
        }
        _ => mismatch(path, contract_label(contract), value),
    }
}

fn limit_error(
    path: &str,
    expected: impl Into<String>,
    observed: impl Into<String>,
) -> ValidationError {
    ValidationError {
        path: path.to_string(),
        expected: expected.into(),
        observed: observed.into(),
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
        Contract::Tuple(_) | Contract::OptionalTuple { .. } | Contract::RestTuple { .. } => "tuple",
        Contract::Record(_) => "object",
        Contract::Union(_) => "union",
        Contract::Intersection(_) => "intersection",
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
                readonly: false,
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

    #[test]
    fn validates_an_inherited_record_as_a_reifiable_intersection() {
        let field = |name: &str, value: Type| TypeField {
            name: name.to_string(),
            readonly: false,
            optional: false,
            value,
            span: crate::diagnostic::SourceSpan::new("test", 0, 0),
        };
        let named = BTreeMap::from([(
            "Labeled".to_string(),
            Type::Intersection(vec![
                Type::Record(vec![field("id", Type::String)]),
                Type::Record(vec![field("label", Type::String)]),
            ]),
        )]);
        let plan = ContractPlan::from_type(
            "Labeled",
            &Type::Named {
                name: "Labeled".to_string(),
                arguments: Vec::new(),
            },
            &named,
        )
        .unwrap();
        let valid = ContractValue::Object(BTreeMap::from([
            ("id".to_string(), ContractValue::String("ada".to_string())),
            (
                "label".to_string(),
                ContractValue::String("user".to_string()),
            ),
        ]));
        assert!(plan.validate(&valid).is_ok());
        let missing = ContractValue::Object(BTreeMap::from([(
            "id".to_string(),
            ContractValue::String("ada".to_string()),
        )]));
        assert_eq!(plan.validate(&missing).unwrap_err().path, "$.label");
    }

    #[test]
    fn validates_with_explicit_string_collection_and_fuel_limits() {
        let string_plan = ContractPlan::from_type("Text", &Type::String, &BTreeMap::new()).unwrap();
        let string_error = string_plan
            .validate_with_limits(
                &ContractValue::String("abc".to_string()),
                ValidationLimits {
                    max_string_bytes: 2,
                    ..ValidationLimits::default()
                },
            )
            .unwrap_err();
        assert_eq!(string_error.path, "$");
        assert!(string_error.expected.contains("2 bytes"));

        let array_plan = ContractPlan::from_type(
            "Names",
            &Type::Array(Box::new(Type::String)),
            &BTreeMap::new(),
        )
        .unwrap();
        let collection_error = array_plan
            .validate_with_limits(
                &ContractValue::Array(vec![
                    ContractValue::String("a".to_string()),
                    ContractValue::String("b".to_string()),
                ]),
                ValidationLimits {
                    max_collection_entries: 1,
                    ..ValidationLimits::default()
                },
            )
            .unwrap_err();
        assert!(collection_error.expected.contains("1 entries"));

        let fuel_error = array_plan
            .validate_with_limits(
                &ContractValue::Array(vec![ContractValue::String("a".to_string())]),
                ValidationLimits {
                    max_nodes: 1,
                    ..ValidationLimits::default()
                },
            )
            .unwrap_err();
        assert_eq!(fuel_error.path, "$[0]");
        assert!(fuel_error.expected.contains("fuel 1"));

        let (metered_error, usage) = array_plan.validate_with_limits_metered(
            &ContractValue::Array(vec![ContractValue::String("a".to_string())]),
            ValidationLimits {
                max_nodes: 1,
                ..ValidationLimits::default()
            },
        );
        assert_eq!(metered_error.unwrap_err(), fuel_error);
        assert_eq!(usage.visited_nodes, 2, "the rejected node also costs work");
    }
}
