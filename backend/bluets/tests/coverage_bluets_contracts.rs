// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-API coverage for BlueTS runtime contracts: lowering checker-approved
//! types into a [`ContractPlan`] and validating data-only values against it.
//!
//! Record types cannot be constructed outside the crate, so every record and
//! alias here comes from a real compilation of TypeScript source, exactly as a
//! host would obtain them.

use blueice_bluets::{
    compile, CompilerOptions, Contract, ContractError, ContractPlan, ContractValue, Declaration,
    MapLoader, ModuleSource, Type, ValidationError, ValidationLimits,
};
use std::collections::BTreeMap;

const SCHEMA: &str = "\
export interface Address { street: string; zip?: string }
export interface User {
  id: number;
  name: string;
  active: boolean;
  address: Address;
  billing: Address;
  tags: string[];
  pair: [string, number];
  role: 'admin' | \"guest\";
  nickname: string | null;
  level: 1 | 2;
  flag: true;
}
export interface Node { value: number; next: Node | null }
export type Id = number | string;
export type Verified = Address & { verified: boolean };
";

/// Compiles `source` and returns a named-type table: interfaces become record
/// types and aliases become their aliased type.
fn named_types(source: &str) -> BTreeMap<String, Type> {
    let result = compile(
        "main.ts",
        &MapLoader::from([ModuleSource::new("main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let mut named = BTreeMap::new();
    for declaration in &result.project.modules["main.ts"].declarations {
        match declaration {
            Declaration::Interface(interface) => {
                named.insert(
                    interface.name.clone(),
                    Type::Record(interface.fields.clone()),
                );
            }
            Declaration::TypeAlias(alias) => {
                named.insert(alias.name.clone(), alias.value.clone());
            }
            _ => {}
        }
    }
    named
}

fn schema() -> BTreeMap<String, Type> {
    named_types(SCHEMA)
}

fn named(name: &str) -> Type {
    Type::Named {
        name: name.to_string(),
        arguments: Vec::new(),
    }
}

fn plan_for(name: &str) -> ContractPlan {
    ContractPlan::from_type(name, &named(name), &schema()).unwrap()
}

fn plan_of(value: &Type) -> ContractPlan {
    ContractPlan::from_type("test", value, &schema()).unwrap()
}

fn string(value: &str) -> ContractValue {
    ContractValue::String(value.to_string())
}

fn number(value: f64) -> ContractValue {
    ContractValue::Number(value)
}

fn array(values: Vec<ContractValue>) -> ContractValue {
    ContractValue::Array(values)
}

fn object(fields: Vec<(&str, ContractValue)>) -> ContractValue {
    ContractValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

fn address(street: &str) -> ContractValue {
    object(vec![("street", string(street))])
}

fn user() -> Vec<(&'static str, ContractValue)> {
    vec![
        ("id", number(1.0)),
        ("name", string("Ada")),
        ("active", ContractValue::Boolean(true)),
        ("address", address("1 Main")),
        ("billing", address("2 Side")),
        ("tags", array(vec![string("x"), string("y")])),
        ("pair", array(vec![string("k"), number(9.0)])),
        ("role", string("admin")),
        ("nickname", ContractValue::Null),
        ("level", number(2.0)),
        ("flag", ContractValue::Boolean(true)),
    ]
}

/// Replaces one field of the well-formed user value.
fn user_with(field: &str, value: ContractValue) -> ContractValue {
    let mut fields = user();
    for entry in &mut fields {
        if entry.0 == field {
            entry.1 = value.clone();
        }
    }
    object(fields)
}

fn user_without(field: &str) -> ContractValue {
    object(
        user()
            .into_iter()
            .filter(|entry| entry.0 != field)
            .collect(),
    )
}

#[track_caller]
fn rejected(plan: &ContractPlan, value: &ContractValue) -> ValidationError {
    plan.validate(value).unwrap_err()
}

#[track_caller]
fn assert_error(error: &ValidationError, path: &str, expected: &str, observed: &str) {
    assert_eq!(
        (
            error.path.as_str(),
            error.expected.as_str(),
            error.observed.as_str()
        ),
        (path, expected, observed)
    );
}

#[test]
fn primitive_types_lower_to_matching_contracts_and_accept_only_their_own_kind() {
    let values = [
        ("null", ContractValue::Null),
        ("undefined", ContractValue::Undefined),
        ("boolean", ContractValue::Boolean(false)),
        ("number", number(0.5)),
        ("string", string("")),
        ("array", array(vec![])),
        ("object", object(vec![])),
    ];
    let cases = [
        (Type::Null, Contract::Null, "null"),
        (Type::Undefined, Contract::Undefined, "undefined"),
        (Type::Boolean, Contract::Boolean, "boolean"),
        (Type::Number, Contract::Number, "number"),
        (Type::String, Contract::String, "string"),
    ];
    for (source, contract, label) in cases {
        let plan = plan_of(&source);
        assert_eq!(plan.root, contract);
        assert!(plan.definitions.is_empty());
        for (category, value) in &values {
            if *category == label {
                assert_eq!(plan.validate(value), Ok(()), "{label} accepts {category}");
            } else {
                // The observed side names every ContractValue category.
                assert_error(&rejected(&plan, value), "$", label, category);
            }
        }
    }
}

#[test]
fn literal_contracts_match_by_kind_and_spelling() {
    let string_literal = plan_of(&Type::Literal("'admin'".to_string()));
    assert_eq!(string_literal.validate(&string("admin")), Ok(()));
    assert_error(
        &rejected(&string_literal, &string("root")),
        "$",
        "'admin'",
        "string",
    );
    // A number is never the string literal.
    assert_error(
        &rejected(&string_literal, &number(1.0)),
        "$",
        "'admin'",
        "number",
    );

    let double_quoted = plan_of(&Type::Literal("\"guest\"".to_string()));
    assert_eq!(double_quoted.validate(&string("guest")), Ok(()));
    assert!(double_quoted.validate(&string("\"guest\"")).is_err());

    // The empty string literal and degenerate one-character spellings are safe.
    let empty = plan_of(&Type::Literal("''".to_string()));
    assert_eq!(empty.validate(&string("")), Ok(()));
    assert!(empty.validate(&string("x")).is_err());
    let lone_quote = plan_of(&Type::Literal("'".to_string()));
    assert!(lone_quote.validate(&string("")).is_err());
    assert!(lone_quote.validate(&string("'")).is_err());
    // An unquoted spelling is not a string literal.
    let bare = plan_of(&Type::Literal("admin".to_string()));
    assert!(bare.validate(&string("admin")).is_err());

    let numeric = plan_of(&Type::Literal("42".to_string()));
    assert_eq!(numeric.validate(&number(42.0)), Ok(()));
    assert!(numeric.validate(&number(43.0)).is_err());
    assert!(numeric.validate(&string("42")).is_err());
    let fractional = plan_of(&Type::Literal("-1.5".to_string()));
    assert_eq!(fractional.validate(&number(-1.5)), Ok(()));

    let truthy = plan_of(&Type::Literal("true".to_string()));
    assert_eq!(truthy.validate(&ContractValue::Boolean(true)), Ok(()));
    assert!(truthy.validate(&ContractValue::Boolean(false)).is_err());
    let falsy = plan_of(&Type::Literal("false".to_string()));
    assert_eq!(falsy.validate(&ContractValue::Boolean(false)), Ok(()));
    assert!(falsy.validate(&ContractValue::Boolean(true)).is_err());

    // Null, undefined and collections are never literals.
    for value in [
        ContractValue::Null,
        ContractValue::Undefined,
        array(vec![]),
        object(vec![]),
    ] {
        assert!(numeric.validate(&value).is_err());
        assert!(truthy.validate(&value).is_err());
    }
}

#[test]
fn arrays_report_the_path_of_the_first_bad_element() {
    let plan = plan_of(&Type::Array(Box::new(Type::Number)));
    assert_eq!(plan.validate(&array(vec![])), Ok(()));
    assert_eq!(
        plan.validate(&array(vec![number(1.0), number(2.0)])),
        Ok(())
    );
    assert_error(
        &rejected(
            &plan,
            &array(vec![number(1.0), string("two"), string("three")]),
        ),
        "$[1]",
        "number",
        "string",
    );
    assert_error(&rejected(&plan, &string("nope")), "$", "array", "string");
    assert_error(&rejected(&plan, &object(vec![])), "$", "array", "object");

    let nested = plan_of(&Type::Array(Box::new(Type::Array(Box::new(Type::Boolean)))));
    let bad = array(vec![
        array(vec![ContractValue::Boolean(true)]),
        array(vec![ContractValue::Boolean(true), number(0.0)]),
    ]);
    assert_error(&rejected(&nested, &bad), "$[1][1]", "boolean", "number");
}

#[test]
fn tuples_require_the_exact_length_and_position_wise_types() {
    let plan = plan_of(&Type::Tuple(vec![Type::String, Type::Number]));
    assert_eq!(
        plan.validate(&array(vec![string("k"), number(1.0)])),
        Ok(())
    );
    assert_error(
        &rejected(&plan, &array(vec![string("k")])),
        "$",
        "tuple of length 2",
        "array of length 1",
    );
    assert_error(
        &rejected(&plan, &array(vec![string("k"), number(1.0), number(2.0)])),
        "$",
        "tuple of length 2",
        "array of length 3",
    );
    assert_error(
        &rejected(&plan, &array(vec![string("k"), string("v")])),
        "$[1]",
        "number",
        "string",
    );
    assert_error(&rejected(&plan, &number(1.0)), "$", "tuple", "number");
    // The empty tuple only accepts the empty array.
    let empty = plan_of(&Type::Tuple(Vec::new()));
    assert_eq!(empty.validate(&array(vec![])), Ok(()));
    assert!(empty.validate(&array(vec![number(1.0)])).is_err());
}

#[test]
fn records_check_required_optional_and_nested_fields() {
    let plan = plan_for("Address");
    assert_eq!(plan.validate(&address("Main")), Ok(()));
    // An optional field may be absent or present with the right type.
    assert_eq!(
        plan.validate(&object(vec![
            ("street", string("Main")),
            ("zip", string("100"))
        ])),
        Ok(())
    );
    assert_error(
        &rejected(
            &plan,
            &object(vec![("street", string("Main")), ("zip", number(100.0))]),
        ),
        "$.zip",
        "string",
        "number",
    );
    assert_error(
        &rejected(&plan, &object(vec![("zip", string("100"))])),
        "$.street",
        "required property",
        "missing",
    );
    // Only objects are records.
    assert_error(&rejected(&plan, &array(vec![])), "$", "object", "array");
    assert_error(
        &rejected(&plan, &ContractValue::Null),
        "$",
        "object",
        "null",
    );
    // A property that is present but `undefined` is not silently a missing one.
    assert_error(
        &rejected(&plan, &object(vec![("street", ContractValue::Undefined)])),
        "$.street",
        "string",
        "undefined",
    );
}

#[test]
fn a_full_interface_lowers_to_definitions_and_validates_end_to_end() {
    let plan = plan_for("User");
    assert_eq!(plan.id, "User");
    assert_eq!(plan.root, Contract::Reference("User".to_string()));
    // `Address` is used twice but defined once; both uses are references.
    let names: Vec<&str> = plan.definitions.keys().map(String::as_str).collect();
    assert_eq!(names, ["Address", "User"]);

    let valid = object(user());
    assert_eq!(plan.validate(&valid), Ok(()));
    // The record contract only checks its declared fields.
    let mut extra = user();
    extra.push(("unexpected", number(1.0)));
    assert_eq!(plan.validate(&object(extra)), Ok(()));

    assert_error(
        &rejected(&plan, &user_without("id")),
        "$.id",
        "required property",
        "missing",
    );
    assert_error(
        &rejected(&plan, &user_with("name", number(1.0))),
        "$.name",
        "string",
        "number",
    );
    assert_error(
        &rejected(&plan, &user_with("active", string("yes"))),
        "$.active",
        "boolean",
        "string",
    );
    assert_error(
        &rejected(&plan, &user_with("address", object(vec![]))),
        "$.address.street",
        "required property",
        "missing",
    );
    assert_error(
        &rejected(&plan, &user_with("billing", string("x"))),
        "$.billing",
        "object",
        "string",
    );
    assert_error(
        &rejected(
            &plan,
            &user_with("tags", array(vec![string("x"), number(1.0)])),
        ),
        "$.tags[1]",
        "string",
        "number",
    );
    assert_error(
        &rejected(&plan, &user_with("pair", array(vec![string("k")]))),
        "$.pair",
        "tuple of length 2",
        "array of length 1",
    );
    // Unions report their own expectation, not any single member's.
    assert_error(
        &rejected(&plan, &user_with("role", string("root"))),
        "$.role",
        "a member of the declared union",
        "string",
    );
    assert_error(
        &rejected(&plan, &user_with("nickname", number(1.0))),
        "$.nickname",
        "a member of the declared union",
        "number",
    );
    assert_error(
        &rejected(&plan, &user_with("level", number(3.0))),
        "$.level",
        "a member of the declared union",
        "number",
    );
    assert_error(
        &rejected(&plan, &user_with("flag", ContractValue::Boolean(false))),
        "$.flag",
        "true",
        "boolean",
    );
}

#[test]
fn unions_accept_any_member_and_intersections_require_every_member() {
    let id = plan_for("Id");
    assert_eq!(id.validate(&number(1.0)), Ok(()));
    assert_eq!(id.validate(&string("one")), Ok(()));
    assert_error(
        &rejected(&id, &ContractValue::Boolean(true)),
        "$",
        "a member of the declared union",
        "boolean",
    );

    let verified = plan_for("Verified");
    assert!(matches!(
        verified.definitions["Verified"],
        Contract::Intersection(_)
    ));
    let ok = object(vec![
        ("street", string("Main")),
        ("verified", ContractValue::Boolean(true)),
    ]);
    assert_eq!(verified.validate(&ok), Ok(()));
    assert_error(
        &rejected(
            &verified,
            &object(vec![("verified", ContractValue::Boolean(true))]),
        ),
        "$.street",
        "required property",
        "missing",
    );
    assert_error(
        &rejected(&verified, &address("Main")),
        "$.verified",
        "required property",
        "missing",
    );
}

#[test]
fn recursive_interfaces_lower_once_and_validate_at_any_depth() {
    let plan = plan_for("Node");
    let Contract::Record(fields) = &plan.definitions["Node"] else {
        panic!("Node lowers to a record contract");
    };
    let Contract::Union(options) = &fields[1].contract else {
        panic!("`next` lowers to a union");
    };
    assert_eq!(options[0], Contract::Reference("Node".to_string()));
    assert_eq!(plan.definitions.len(), 1);

    let mut chain = ContractValue::Null;
    for value in [3.0, 2.0, 1.0] {
        chain = object(vec![("value", number(value)), ("next", chain)]);
    }
    assert_eq!(plan.validate(&chain), Ok(()));

    // The failure path walks through the recursion. The innermost bad link
    // is inside a union, so only the union's own message is reported there.
    let broken = object(vec![
        ("value", number(1.0)),
        (
            "next",
            object(vec![
                ("value", string("bad")),
                ("next", ContractValue::Null),
            ]),
        ),
    ]);
    assert_error(
        &rejected(&plan, &broken),
        "$.next",
        "a member of the declared union",
        "object",
    );
    assert_error(
        &rejected(&plan, &object(vec![("value", number(1.0))])),
        "$.next",
        "required property",
        "missing",
    );
}

#[test]
fn unreifiable_types_are_rejected_with_an_explanation() {
    let table = schema();
    let unresolved = ContractPlan::from_type("x", &named("Missing"), &table).unwrap_err();
    assert_eq!(
        unresolved.message,
        "type `Missing` is not a reifiable named contract"
    );
    for erased in [Type::Any, Type::Unknown, Type::Never] {
        let error = ContractPlan::from_type("x", &erased, &table).unwrap_err();
        assert_eq!(
            error.message,
            "any, unknown, and never are not automatic runtime contracts"
        );
    }
    let void = ContractPlan::from_type("x", &Type::Void, &table).unwrap_err();
    assert_eq!(void.message, "void is not a data-boundary runtime contract");
    let generic = ContractPlan::from_type(
        "x",
        &Type::Named {
            name: "Box".to_string(),
            arguments: vec![Type::Number],
        },
        &table,
    )
    .unwrap_err();
    assert_eq!(
        generic.message,
        "generic type `Box` needs an explicit reifiable contract"
    );

    // The rejection surfaces from any nesting position.
    let nested = [
        Type::Array(Box::new(Type::Any)),
        Type::Tuple(vec![Type::String, Type::Void]),
        Type::Union(vec![Type::String, Type::Unknown]),
        Type::Intersection(vec![Type::Never, Type::String]),
        Type::Array(Box::new(named("Missing"))),
    ];
    for value in nested {
        assert!(
            ContractPlan::from_type("x", &value, &table).is_err(),
            "{value:?}"
        );
    }
    // ...including from a field of a named record.
    let with_erased = named_types("export interface Bad { value: any }");
    let error = ContractPlan::from_type("Bad", &named("Bad"), &with_erased).unwrap_err();
    assert!(error.message.contains("not automatic"));
}

#[test]
fn contract_errors_are_displayable_standard_errors() {
    let error = ContractPlan::from_type("x", &Type::Void, &BTreeMap::new()).unwrap_err();
    assert_eq!(error.to_string(), error.message);
    let boxed: Box<dyn std::error::Error> = Box::new(ContractError {
        message: "custom".to_string(),
    });
    assert_eq!(boxed.to_string(), "custom");
}

#[test]
fn fingerprints_are_stable_and_track_identity_and_structure() {
    let first = plan_for("User");
    let second = plan_for("User");
    assert_eq!(first, second);
    assert_eq!(first.fingerprint, second.fingerprint);
    let hex = first.fingerprint.strip_prefix("bts-contract-").unwrap();
    assert_eq!(hex.len(), 16);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));

    // A different id, root or definition table changes the fingerprint.
    let renamed = ContractPlan::from_type("Account", &named("User"), &schema()).unwrap();
    assert_ne!(renamed.fingerprint, first.fingerprint);
    assert_ne!(plan_for("Address").fingerprint, first.fingerprint);
    assert_ne!(
        plan_of(&Type::String).fingerprint,
        plan_of(&Type::Number).fingerprint
    );
    let changed = named_types(&SCHEMA.replace("street: string", "street: number"));
    let changed_plan = ContractPlan::from_type("User", &named("User"), &changed).unwrap();
    assert_ne!(changed_plan.fingerprint, first.fingerprint);
}

#[test]
fn a_reference_to_an_undefined_contract_fails_closed() {
    // Plans are plain data, so a hostile or corrupted one can be constructed.
    let plan = ContractPlan {
        id: "dangling".to_string(),
        root: Contract::Reference("Ghost".to_string()),
        definitions: BTreeMap::new(),
        fingerprint: String::new(),
    };
    assert_error(
        &rejected(&plan, &number(1.0)),
        "$",
        "defined contract `Ghost`",
        "number",
    );
}

fn tree_plan() -> ContractPlan {
    // type Tree = Tree[] | number, built directly.
    let table = BTreeMap::from([(
        "Tree".to_string(),
        Type::Union(vec![Type::Array(Box::new(named("Tree"))), Type::Number]),
    )]);
    ContractPlan::from_type("Tree", &named("Tree"), &table).unwrap()
}

fn nested(depth: usize) -> ContractValue {
    let mut value = number(1.0);
    for _ in 0..depth {
        value = array(vec![value]);
    }
    value
}

#[test]
fn default_limits_are_documented_values() {
    let limits = ValidationLimits::default();
    assert_eq!(limits.max_depth, 128);
    assert_eq!(limits.max_collection_entries, 10_000);
    assert_eq!(limits.max_nodes, 100_000);
    assert_eq!(limits.max_string_bytes, 1_048_576);
}

#[test]
fn depth_limit_bounds_recursion_through_recursive_contracts() {
    let plan = tree_plan();
    // Shallow values pass under the default limits.
    assert_eq!(plan.validate(&nested(10)), Ok(()));
    // A hostile, very deep value is refused instead of exhausting the stack.
    let error = plan
        .validate_with_limits(
            &nested(400),
            ValidationLimits {
                max_depth: 128,
                ..ValidationLimits::default()
            },
        )
        .unwrap_err();
    // The depth failure is inside the recursive union, whose own alternatives
    // are all exhausted, so the outermost union reports the failure.
    assert_error(&error, "$", "a member of the declared union", "array");

    // With a direct (non-union) contract the depth message itself surfaces.
    let direct = plan_of(&Type::Array(Box::new(Type::Array(Box::new(Type::Number)))));
    let limits = ValidationLimits {
        max_depth: 1,
        ..ValidationLimits::default()
    };
    assert_eq!(
        direct.validate_with_limits(&array(vec![array(vec![])]), limits),
        Ok(())
    );
    let error = direct
        .validate_with_limits(&array(vec![array(vec![number(1.0)])]), limits)
        .unwrap_err();
    assert_error(
        &error,
        "$[0][0]",
        "a contract value within depth 1",
        "number",
    );
}

#[test]
fn node_fuel_limit_counts_every_visited_value() {
    let plan = plan_of(&Type::Array(Box::new(Type::Number)));
    let value = array(vec![number(1.0), number(2.0), number(3.0)]);
    // The array and its three elements are four visited values.
    let exact = ValidationLimits {
        max_nodes: 4,
        ..ValidationLimits::default()
    };
    assert_eq!(plan.validate_with_limits(&value, exact), Ok(()));
    let short = ValidationLimits {
        max_nodes: 3,
        ..ValidationLimits::default()
    };
    assert_error(
        &plan.validate_with_limits(&value, short).unwrap_err(),
        "$[2]",
        "a contract value within fuel 3",
        "number",
    );
    let none = ValidationLimits {
        max_nodes: 0,
        ..ValidationLimits::default()
    };
    assert_error(
        &plan_of(&Type::Number)
            .validate_with_limits(&number(1.0), none)
            .unwrap_err(),
        "$",
        "a contract value within fuel 0",
        "number",
    );
}

#[test]
fn string_and_collection_limits_are_reported_with_the_observed_size() {
    let strings = plan_of(&Type::String);
    let limits = ValidationLimits {
        max_string_bytes: 3,
        ..ValidationLimits::default()
    };
    assert_eq!(strings.validate_with_limits(&string("abc"), limits), Ok(()));
    assert_error(
        &strings
            .validate_with_limits(&string("abcd"), limits)
            .unwrap_err(),
        "$",
        "string within 3 bytes",
        "string of 4 bytes",
    );
    // The bound is on UTF-8 bytes, not characters.
    assert!(strings
        .validate_with_limits(&string("\u{20ac}\u{20ac}"), limits)
        .is_err());

    let arrays = plan_of(&Type::Array(Box::new(Type::Number)));
    let limits = ValidationLimits {
        max_collection_entries: 2,
        ..ValidationLimits::default()
    };
    assert_eq!(
        arrays.validate_with_limits(&array(vec![number(1.0), number(2.0)]), limits),
        Ok(())
    );
    assert_error(
        &arrays
            .validate_with_limits(&array(vec![number(1.0); 3]), limits)
            .unwrap_err(),
        "$",
        "array within 2 entries",
        "array of 3 entries",
    );

    let records = plan_for("Address");
    let limits = ValidationLimits {
        max_collection_entries: 1,
        ..ValidationLimits::default()
    };
    assert_eq!(
        records.validate_with_limits(&address("Main"), limits),
        Ok(())
    );
    assert_error(
        &records
            .validate_with_limits(
                &object(vec![("street", string("Main")), ("zip", string("1"))]),
                limits,
            )
            .unwrap_err(),
        "$",
        "object within 1 entries",
        "object of 2 entries",
    );
}
