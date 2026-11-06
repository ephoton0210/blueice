// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Audited harness-to-selector contracts. Denominators come from selected source rows.
use super::{Domain, Rule, SAFE_HOME, STATIC_SUPER};
macro_rules! wrapped {
    ($name:literal,$script:literal,$form:literal) => {
        Rule {
            name: $name,
            script: $script,
            domain: Domain::Form($form),
            direct: false,
        }
    };
}
macro_rules! direct {
    ($name:literal,$script:literal,$form:literal) => {
        Rule {
            name: $name,
            script: $script,
            domain: Domain::Form($form),
            direct: true,
        }
    };
}
macro_rules! other {
    ($name:literal,$script:literal,$domain:expr) => {
        Rule {
            name: $name,
            script: $script,
            domain: $domain,
            direct: false,
        }
    };
}
pub(super) fn rules(stem: &str) -> Option<&'static [Rule]> {
    Some(match stem {
        "ge5_async_arrow_family" => &[wrapped!(
            "async_arrow_default_block_all_frozen_products",
            "verify_ge5_async_arrow_roles.cjs",
            "ge1_downlevel/async-arrow-default-block"
        )],
        "ge5_async_arrow_finally_endpoints" => &[wrapped!(
            "async_arrow_finally_endpoints_all_frozen_products",
            "verify_ge5_async_arrow_finally_endpoints.cjs",
            "ge2_module_helpers/async-arrow-finally"
        )],
        "ge5_async_arrow_finally_family" => &[wrapped!(
            "async_arrow_finally_all_frozen_products",
            "verify_ge5_async_arrow_finally_roles.cjs",
            "ge2_module_helpers/async-arrow-finally"
        )],
        "ge5_async_class_method_family" => &[wrapped!(
            "async_class_method_all_frozen_products",
            "verify_ge5_async_class_method_roles.cjs",
            "ge1_downlevel/async-class-method"
        )],
        "ge5_async_generator_family" => &[wrapped!(
            "async_generator_exception_template_all_frozen_products",
            "verify_ge5_async_generator_roles.cjs",
            "ge1_downlevel/async-generator-exception-template"
        )],
        "ge5_async_generator_g2_family" => &[wrapped!(
            "async_generator_finally_all_frozen_products",
            "verify_ge5_async_generator_g2_roles.cjs",
            "ge2_module_helpers/async-generator-finally"
        )],
        "ge5_async_generator_g2_endpoints" => &[
            wrapped!(
                "exported_async_generator_endpoints_all_frozen_products",
                "verify_ge5_async_generator_g2_endpoints.cjs",
                "ge2_module_helpers/async-generator-finally"
            ),
            wrapped!(
                "borrowed_async_generator_methods_match_every_native_target_profile",
                "verify_ge5_async_generator_g2_borrowed.cjs",
                "ge2_module_helpers/async-generator-finally"
            ),
            other!(
                "borrowed_methods_preserve_source_receiver_and_arguments",
                "verify_ge5_async_generator_g2_capture.cjs",
                Domain::Capture("ge2_module_helpers/async-generator-finally")
            ),
        ],
        "ge5_async_generator_operands" => &[wrapped!(
            "async_generator_operands_all_frozen_products",
            "verify_ge5_async_generator_operands.cjs",
            "ge1_downlevel/async-generator-exception-template"
        )],
        "ge5_async_generator_runtime" => &[wrapped!(
            "async_generator_runtime_all_frozen_products",
            "verify_ge5_async_generator_runtime.cjs",
            "ge1_downlevel/async-generator-exception-template"
        )],
        "ge5_async_method_family" => &[wrapped!(
            "async_object_method_all_frozen_products",
            "verify_ge5_async_method_roles.cjs",
            "ge1_downlevel/async-object-method"
        )],
        "ge5_async_super_family" => &[wrapped!(
            "async_super_method_all_frozen_products",
            "verify_ge5_async_super_roles.cjs",
            "ge1_downlevel/async-super-method"
        )],
        "ge5_decorator_adjacent_observations" => &[other!(
            "collect_both_complete_adjacent_matrices_with_two_public_builds",
            "",
            Domain::Adjacent
        )],
        "ge5_generator_switch_family" => &[wrapped!(
            "generator_switch_delegation_all_frozen_products",
            "verify_ge5_generator_switch_roles.cjs",
            "ge2_module_helpers/generator-switch-delegation"
        )],
        "ge5_generator_switch_endpoints" => &[
            wrapped!(
                "generator_switch_all_source_endpoints_keep_native_phases",
                "verify_ge5_generator_switch_endpoints.cjs",
                "ge2_module_helpers/generator-switch-delegation"
            ),
            wrapped!(
                "generator_switch_borrowed_methods_keep_the_target_receiver_abi",
                "verify_ge5_generator_switch_borrowed.cjs",
                "ge2_module_helpers/generator-switch-delegation"
            ),
            wrapped!(
                "generator_switch_getters_and_abrupt_completion_keep_the_target_abi",
                "verify_ge5_generator_switch_runtime.cjs",
                "ge2_module_helpers/generator-switch-delegation"
            ),
            other!(
                "generator_switch_borrowed_callers_preserve_source_this_and_arguments",
                "verify_ge5_generator_switch_capture.cjs",
                Domain::Capture("ge2_module_helpers/generator-switch-delegation")
            ),
        ],
        "ge5_helper_collision_family" => &[wrapped!(
            "helper_collision_all_frozen_products",
            "verify_ge5_helper_collision_roles.cjs",
            "ge2_module_helpers/helper-name-collision"
        )],
        "ge5_helper_collision_maps" => &[wrapped!(
            "helper_collision_headers_and_await_endpoints_all_frozen_products",
            "verify_ge5_helper_collision_boundaries.cjs",
            "ge2_module_helpers/helper-name-collision"
        )],
        "ge5_helper_provider_globals" => &[other!(
            "private_ambient_and_imported_helpers_keep_their_own_bindings",
            "verify_ge5_helper_provider_globals.cjs",
            Domain::Providers
        )],
        "ge5_interop_expression_maps" => &[direct!(
            "all_interop_expressions_preserve_complete_native_roles",
            "verify_ge5_interop_expression_maps.cjs",
            "ge2_module_helpers/interop-exportstar-cycle"
        )],
        "ge5_interop_module_maps" => &[direct!(
            "all_interop_modules_preserve_complete_native_roles",
            "verify_ge5_interop_module_acceptance.cjs",
            "ge2_module_helpers/interop-exportstar-cycle"
        )],
        "ge5_legacy_class_phase" => &[other!(
            "native_cycle_products_preserve_all_class_initialization_phases",
            "verify_ge5_decorator_class_phase.cjs",
            Domain::LegacyPhase
        )],
        "ge5_legacy_decorator_module_maps" => &[direct!(
            "legacy_parameter_decorator_preserves_every_complete_module_role",
            "verify_ge5_decorator_legacy_roles.cjs",
            "ge2_module_helpers/legacy-parameter-decorator"
        )],
        "ge5_lexical_arrow_driver" => &[wrapped!(
            "lexical_arrow_all_frozen_driver_boundaries",
            "verify_ge5_lexical_arrow_driver.cjs",
            "ge1_downlevel/async-arrow-lexical-this"
        )],
        "ge5_lexical_arrow_family" => &[wrapped!(
            "async_arrow_lexical_this_all_frozen_products",
            "verify_ge5_lexical_arrow_roles.cjs",
            "ge1_downlevel/async-arrow-lexical-this"
        )],
        "ge5_literal_accessor_module_maps" => &[direct!(
            "literal_accessor_decorators_preserve_every_complete_module_role",
            "verify_ge5_decorator_member_roles.cjs",
            "ge3_decorator_compositions/literal-accessors"
        )],
        "ge5_literal_auto_accessor_module_maps" => &[direct!(
            "literal_auto_accessor_decorators_preserve_every_complete_module_role",
            "verify_ge5_auto_accessor_roles.cjs",
            "ge3_decorator_compositions/literal-auto-accessors"
        )],
        "ge5_literal_decorator_module_maps" => &[direct!(
            "literal_field_decorators_preserve_every_complete_module_role",
            "verify_ge5_decorator_literal_roles.cjs",
            "ge3_decorator_compositions/literal-fields"
        )],
        "ge5_literal_method_family" => &[wrapped!(
            "literal_methods_all_frozen_products",
            "verify_ge5_decorator_method_roles.cjs",
            "ge3_decorator_compositions/literal-methods"
        )],
        "ge5_literal_replacement_module_maps" => &[direct!(
            "literal_replacement_order_preserves_every_complete_module_role",
            "verify_ge5_decorator_replacement_roles.cjs",
            "ge3_decorator_compositions/literal-replacement-order"
        )],
        "ge5_metadata_callable" => &[direct!(
            "all_imported_callable_alias_products_match_native_roles",
            "verify_ge5_metadata_callable.cjs",
            "ge3_decorator_compositions/imported-callable-alias"
        )],
        "ge5_metadata_class_alias" => &[direct!(
            "all_imported_class_type_alias_products_match_native_roles",
            "verify_ge5_metadata_class_alias.cjs",
            "ge3_decorator_compositions/imported-class-type-alias"
        )],
        "ge5_metadata_import_type" => &[direct!(
            "all_import_type_class_products_match_native_roles",
            "verify_ge5_metadata_import_type.cjs",
            "ge3_decorator_compositions/import-type-class"
        )],
        "ge5_metadata_inline_type" => &[direct!(
            "all_inline_import_type_class_products_match_native_roles",
            "verify_ge5_metadata_inline_type.cjs",
            "ge3_decorator_compositions/inline-import-type-class"
        )],
        "ge5_metadata_interface" => &[direct!(
            "all_imported_interface_products_match_native_roles",
            "verify_ge5_metadata_interface.cjs",
            "ge3_decorator_compositions/imported-interface"
        )],
        "ge5_metadata_local_alias" => &[direct!(
            "all_local_class_type_alias_products_match_native_roles",
            "verify_ge5_metadata_local_alias.cjs",
            "ge3_decorator_compositions/local-class-type-alias"
        )],
        "ge5_metadata_namespace_reexport" => &[direct!(
            "all_namespace_reexport_products_match_native_roles",
            "verify_ge5_metadata_namespace_reexport.cjs",
            "ge3_decorator_compositions/namespace-reexported-class"
        )],
        "ge5_metadata_nullable" => &[direct!(
            "all_imported_nullable_alias_products_match_native_roles",
            "verify_ge5_metadata_nullable.cjs",
            "ge3_decorator_compositions/imported-nullable-alias"
        )],
        "ge5_metadata_numeric_enum" => &[direct!(
            "all_imported_numeric_enum_products_match_native_roles",
            "verify_ge5_metadata_numeric_enum.cjs",
            "ge3_decorator_compositions/imported-numeric-enum"
        )],
        "ge5_metadata_primitive" => &[direct!(
            "all_imported_primitive_alias_products_match_native_roles",
            "verify_ge5_metadata_primitive.cjs",
            "ge3_decorator_compositions/imported-primitive-alias"
        )],
        "ge5_metadata_reexport" => &[direct!(
            "all_imported_reexport_products_match_native_roles",
            "verify_ge5_metadata_reexport.cjs",
            "ge3_decorator_compositions/reexported-class"
        )],
        "ge5_metadata_string_enum" => &[direct!(
            "all_imported_string_enum_products_match_native_roles",
            "verify_ge5_metadata_string_enum.cjs",
            "ge3_decorator_compositions/imported-string-enum"
        )],
        "ge5_private_accessor_family" => &[
            other!(
                "private_accessor_public_cli_phases",
                "verify_ge5_private_accessor_public.cjs",
                Domain::Public("ge2_module_helpers/private-accessor")
            ),
            wrapped!(
                "private_accessor_all_frozen_products",
                "verify_ge5_private_accessor_roles.cjs",
                "ge2_module_helpers/private-accessor"
            ),
        ],
        "ge5_standard_decorator_family" => &[
            other!(
                "standard_decorator_public_cli_phases",
                "verify_ge5_standard_decorator_public.cjs",
                Domain::Public("ge2_module_helpers/standard-decorator")
            ),
            wrapped!(
                "standard_decorator_all_frozen_products",
                "verify_ge5_standard_decorator_roles.cjs",
                "ge2_module_helpers/standard-decorator"
            ),
        ],
        "ge5_source_maps" => &[other!(
            "complete_native_map_authority_is_reproducible",
            "record_ge5_maps.cjs",
            Domain::All
        )],
        "ge5_double_accessor_family" => &[wrapped!(
            "double_decorated_accessors_all_frozen_products",
            "verify_ge5_double_accessor_roles.cjs",
            "ge3_decorator_compositions/rejected-double-decorated-accessors"
        )],
        "ge5_safe_home_object" => &[other!(
            "safe_home_object_families_keep_complete_modules_and_logical_ends",
            "verify_ge5_safe_home_roles.cjs",
            Domain::Forms(&SAFE_HOME, "The four exact safe-home forms retain complete module roles, logical endpoints, raw declarations/maps and actual runtime; fresh controls and repeat builds do not multiply frozen configurations.")
        )],
        "ge5_static_super_family" => &[other!(
            "static_super_public_cli_reference_phases",
            "verify_ge5_static_super_public.cjs",
            Domain::Forms(&STATIC_SUPER, "Two public builds and declaration/map comparisons cover all six exact static-super forms; additional reference/dataflow/endpoint assertions apply only to the lowered standard subset, not complete module roles across every configuration.")
        )],
        "ge5_static_super_complete_family" => &[other!(
            "static_super_all_frozen_products",
            "verify_ge5_static_super_complete.cjs",
            Domain::Forms(&STATIC_SUPER, "All six exact static-super forms retain complete module roles, ordered gaps, logical endpoints, declarations/maps and actual runtime; controls/repeated builds do not multiply frozen configurations.")
        )],
        _ => return None,
    })
}

// Support modules own these ignored tests; source rows, never assertion counts,
// determine the configuration denominator.
struct SupportRule {
    name: &'static str,
    script: &'static str,
    form: Option<&'static str>,
}
macro_rules! support_form {
    ($name:literal,$script:literal,$form:literal) => {
        SupportRule {
            name: $name,
            script: $script,
            form: Some($form),
        }
    };
}
macro_rules! support_predicate {
    ($name:literal,$script:literal) => {
        SupportRule {
            name: $name,
            script: $script,
            form: None,
        }
    };
}
fn support_spec(
    stem: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static str,
    &'static [SupportRule],
)> {
    Some(match stem {
        "ge5_binding_maps" => ("binding_roles", "ge5_binding_roles", "family", &[
            support_form!("array_nested_binding_all_frozen_products", "verify_ge5_binding_roles.cjs", "ge1_downlevel/array-nested-binding"),
            support_form!("object_nested_binding_all_frozen_products", "verify_ge5_binding_roles.cjs", "ge1_downlevel/object-nested-binding"),
            support_form!("mixed_nested_default_all_frozen_products", "verify_ge5_binding_roles.cjs", "ge1_downlevel/mixed-nested-default"),
            support_form!("nested_parameter_all_frozen_products", "verify_ge5_binding_roles.cjs", "ge1_downlevel/nested-parameter"),
            support_form!("binding_iterator_finally_all_frozen_products", "verify_ge5_binding_roles.cjs", "ge1_downlevel/binding-iterator-finally"),
        ]),
        "ge5_iteration_maps" => ("iteration_roles", "ge5_iteration_roles", "family", &[
            support_form!("iterable_bindings_forof_all_frozen_products", "verify_ge5_iteration_roles.cjs", "ge2_module_helpers/iterable-bindings-forof"),
        ]),
        "ge5_object_maps" => ("object_roles", "ge5_object_roles", "family", &[
            support_form!("object_rest_spread_all_frozen_products", "verify_ge5_object_roles.cjs", "ge2_module_helpers/object-rest-spread"),
        ]),
        "ge5_retained_decorator_maps" => ("preserved_roles", "ge5_preserved_decorator_roles", "observe", &[
            support_form!("private_super_method_preserves_every_native_module_character_and_role", "verify_ge5_decorator_preserved_roles.cjs", "ge3_decorator_compositions/private-super-method"),
            support_form!("private_super_accessors_preserve_every_native_module_character_and_role", "verify_ge5_decorator_preserved_roles.cjs", "ge3_decorator_compositions/private-super-accessors"),
            support_form!("private_static_super_preserves_every_native_module_character_and_role", "verify_ge5_decorator_preserved_roles.cjs", "ge3_decorator_compositions/private-static-super"),
            support_form!("private_super_async_generator_preserves_every_native_module_character_and_role", "verify_ge5_decorator_preserved_roles.cjs", "ge3_decorator_compositions/private-super-async-generator"),
        ]),
        "ge5_class_maps" => ("class_roles", "ge5_class_roles", "", &[
            support_predicate!("all_class_super_targets_modules_providers_and_products_match_native_roles", "verify_ge5_class_roles.cjs"),
        ]),
        "ge5_class_retained" => ("class_retained", "ge5_class_retained", "", &[
            support_predicate!("retained_super_write_all_frozen_products_match_native_roles", "verify_ge5_class_retained.cjs"),
        ]),
        "ge5_class_expressions" => ("expressions", "ge5_class_expressions", "", &[
            support_predicate!("all_class_expression_products_match_native_roles", "verify_ge5_class_expressions.cjs"),
        ]),
        "ge5_metadata_classes" => ("metadata_classes", "ge5_metadata_classes", "", &[
            support_predicate!("all_metadata_class_dependency_products_match_native_roles", "verify_ge5_metadata_classes.cjs"),
            support_predicate!("adjacent_legacy_wrappers_keep_their_complete_native_class_roles", "verify_ge5_metadata_class_adjacent.cjs"),
        ]),
        _ => return None,
    })
}

pub(super) fn support(
    snapshot: &super::Snapshot,
    suite: &super::Suite,
    input: &serde_json::Value,
) -> super::Result<Option<Vec<super::Case>>> {
    let stem = suite
        .harness
        .rsplit('/')
        .next()
        .and_then(|s| s.strip_suffix(".rs"))
        .ok_or("invalid G-E5 harness")?;
    let Some((alias, file, helper, contracts)) = support_spec(stem) else {
        return Ok(None);
    };
    let root = snapshot.source(&suite.harness)?;
    let relative = format!("oracle_support/{file}.rs");
    let facts = super::super::rust_facts::parse(root)?;
    if facts
        .modules
        .iter()
        .filter(|m| {
            m.name == alias && m.scope.is_empty() && m.path.as_deref() == Some(relative.as_str())
        })
        .count()
        != 1
    {
        return Err("unknown G-E5 support module identity".into());
    }
    let parent = suite
        .harness
        .rsplit_once('/')
        .ok_or("invalid G-E5 harness")?
        .0;
    support_rows(
        alias,
        helper,
        contracts,
        snapshot.source(&format!("{parent}/{relative}"))?,
        &suite.ignored_tests,
        input,
    )
    .map(Some)
}

fn call(function: &syn::ItemFn) -> super::Result<&syn::ExprCall> {
    match function.block.stmts.as_slice() {
        [syn::Stmt::Expr(expression, _)] => match super::unwrapped(expression) {
            syn::Expr::Call(call) => Ok(call),
            _ => Err("unknown support wrapper body".into()),
        },
        _ => Err("partial/unknown support wrapper body".into()),
    }
}
fn script(function: &syn::ItemFn, expected: &str) -> super::Result<()> {
    use syn::visit::Visit;
    let mut facts = super::SelectorFacts::default();
    facts.visit_block(&function.block);
    let relative = format!("../../development/browser_core/phase-18-bluets/tools/{expected}");
    if facts.forbidden
        || facts.scripts.len() != 1
        || !facts
            .scripts
            .iter()
            .all(|s| s == expected || s == &relative)
        || !facts.predicates.is_empty()
    {
        return Err("partial/unknown G-E5 support script selector".into());
    }
    Ok(())
}
fn scalar(
    expression: &syn::Expr,
    row: &serde_json::Value,
    binding: &str,
) -> super::Result<serde_json::Value> {
    use syn::Expr;
    match super::unwrapped(expression) {
        Expr::Path(p) if p.path.is_ident(binding) => Ok(row.clone()),
        Expr::Lit(l) => match &l.lit {
            syn::Lit::Str(s) => Ok(s.value().into()),
            _ => Err("unknown selector literal".into()),
        },
        Expr::Index(i) => {
            let value = scalar(&i.expr, row, binding)?;
            let key = super::string(&i.index).ok_or("unknown selector index")?;
            Ok(value.get(&key).cloned().unwrap_or(serde_json::Value::Null))
        }
        Expr::MethodCall(m) if m.args.is_empty() && m.method == "as_str" => {
            let value = scalar(&m.receiver, row, binding)?;
            Ok(if value.is_string() {
                value
            } else {
                serde_json::Value::Null
            })
        }
        Expr::MethodCall(m) if m.args.is_empty() && m.method == "unwrap" => {
            let value = scalar(&m.receiver, row, binding)?;
            if value.is_null() {
                Err("empty selector unwrap".into())
            } else {
                Ok(value)
            }
        }
        _ => Err("unknown G-E5 selector value".into()),
    }
}
fn pattern(pattern: &syn::Pat, value: &serde_json::Value) -> super::Result<bool> {
    match pattern {
        syn::Pat::Lit(l) => {
            Ok(matches!(&l.lit,syn::Lit::Str(s) if value.as_str()==Some(s.value().as_str())))
        }
        syn::Pat::Or(or) => {
            for p in &or.cases {
                if pattern_matches(p, value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        syn::Pat::TupleStruct(s) if s.path.is_ident("Some") && s.elems.len() == 1 => {
            pattern_matches(s.elems.first().unwrap(), value)
        }
        _ => Err("unknown G-E5 selector pattern".into()),
    }
}
fn pattern_matches(p: &syn::Pat, value: &serde_json::Value) -> super::Result<bool> {
    pattern(p, value)
}
fn predicate(
    expression: &syn::Expr,
    row: &serde_json::Value,
    binding: &str,
) -> super::Result<bool> {
    use syn::{BinOp, Expr};
    match super::unwrapped(expression) {
        Expr::Binary(b) => match b.op {
            BinOp::And(_) => {
                Ok(predicate(&b.left, row, binding)? && predicate(&b.right, row, binding)?)
            }
            BinOp::Or(_) => {
                Ok(predicate(&b.left, row, binding)? || predicate(&b.right, row, binding)?)
            }
            BinOp::Eq(_) => Ok(scalar(&b.left, row, binding)? == scalar(&b.right, row, binding)?),
            BinOp::Ne(_) => Ok(scalar(&b.left, row, binding)? != scalar(&b.right, row, binding)?),
            _ => Err("unknown G-E5 selector operator".into()),
        },
        Expr::Macro(m) if m.mac.path.is_ident("matches") => {
            use syn::parse::Parser;
            let parser = |input: syn::parse::ParseStream<'_>| -> syn::Result<(Expr, syn::Pat)> {
                let value = input.parse()?;
                input.parse::<syn::Token![,]>()?;
                let p = syn::Pat::parse_multi_with_leading_vert(input)?;
                if input.peek(syn::Token![,]) {
                    input.parse::<syn::Token![,]>()?;
                }
                Ok((value, p))
            };
            let (value, p) = parser.parse2(m.mac.tokens.clone())?;
            pattern(&p, &scalar(&value, row, binding)?)
        }
        _ => Err("unknown G-E5 selector predicate".into()),
    }
}
fn class_selection(
    function: &syn::ItemFn,
    functions: &std::collections::BTreeMap<String, &syn::ItemFn>,
    row: &serde_json::Value,
) -> super::Result<bool> {
    let c = call(function)?;
    let syn::Expr::Path(p) = c.func.as_ref() else {
        return Err("unknown class selector call".into());
    };
    let names = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>();
    if ![vec!["class_cli", "run"], vec!["super", "class_cli", "run"]]
        .iter()
        .any(|expected| names.iter().map(String::as_str).collect::<Vec<_>>() == *expected)
        || c.args.len() != 4
    {
        return Err("unknown class selector delegation".into());
    }
    match c.args.first().ok_or("missing class selector")? {
        syn::Expr::Closure(cl) => predicate(
            &cl.body,
            row,
            &super::row_binding(cl).ok_or("unknown class row binding")?,
        ),
        syn::Expr::Path(p) if p.path.is_ident("selected") => {
            let f = functions
                .get("selected")
                .ok_or("missing named class selector")?;
            let Some(syn::FnArg::Typed(arg)) =
                f.sig.inputs.first().filter(|_| f.sig.inputs.len() == 1)
            else {
                return Err("unknown named selector arguments".into());
            };
            let syn::Pat::Ident(name) = arg.pat.as_ref() else {
                return Err("unknown named row binding".into());
            };
            let [syn::Stmt::Expr(expr, None)] = f.block.stmts.as_slice() else {
                return Err("unknown named selector body".into());
            };
            predicate(expr, row, &name.ident.to_string())
        }
        _ => Err("unknown class selector".into()),
    }
}

fn support_rows(
    alias: &str,
    helper: &str,
    contracts: &[SupportRule],
    source: &str,
    ignored: &[String],
    input: &serde_json::Value,
) -> super::Result<Vec<super::Case>> {
    let facts = super::super::rust_facts::parse(source)?;
    let names = contracts
        .iter()
        .map(|r| r.name.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let qualified = names
        .iter()
        .map(|n| format!("{alias}::{n}"))
        .collect::<std::collections::BTreeSet<_>>();
    if facts
        .ignored_tests
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        != names
        || facts.ignored_tests.len() != names.len()
        || ignored
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != qualified
        || ignored.len() != qualified.len()
    {
        return Err("partial/unknown support ignored-test selection".into());
    }
    let file = syn::parse_file(source)?;
    let functions = file
        .items
        .iter()
        .filter_map(|i| {
            if let syn::Item::Fn(f) = i {
                Some((f.sig.ident.to_string(), f))
            } else {
                None
            }
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let rows = input["cases"].as_array().ok_or("missing G-E5 input rows")?;
    let mut selected = std::collections::BTreeSet::new();
    for rule in contracts {
        let f = functions
            .get(rule.name)
            .ok_or("missing ignored support function")?;
        if let Some(form) = rule.form {
            let c = call(f)?;
            if !super::path(&c.func, helper)
                || c.args.len() != if helper == "observe" { 1 } else { 2 }
                || c.args.first().and_then(super::string).as_deref()
                    != form.rsplit_once('/').map(|(_, f)| f)
            {
                return Err("unknown support family argument".into());
            }
            let worker = functions
                .get(helper)
                .ok_or("missing support family helper")?;
            script(worker, rule.script)?;
            family_guard(worker, form)?;
            for (i, row) in rows.iter().enumerate() {
                if row["form"] == form {
                    selected.insert(i);
                }
            }
        } else {
            script(f, rule.script)?;
            for (i, row) in rows.iter().enumerate() {
                if class_selection(f, &functions, row)? {
                    selected.insert(i);
                }
            }
        }
    }
    if selected.is_empty() {
        return Err("empty G-E5 support source selection".into());
    }
    let cases = super::super::case_sources::json_cases(
        super::INPUT,
        &serde_json::to_string(input)?,
        "/cases",
        "program/configuration",
    )?;
    Ok(cases
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| selected.contains(&i).then_some(c))
        .collect())
}

#[derive(Default)]
struct FamilyFacts<'a> {
    loops: Vec<&'a syn::ExprForLoop>,
    guards: Vec<&'a syn::Expr>,
    ranges: Vec<&'a syn::ExprRange>,
}
impl<'a> syn::visit::Visit<'a> for FamilyFacts<'a> {
    fn visit_expr_for_loop(&mut self, node: &'a syn::ExprForLoop) {
        self.loops.push(node);
        syn::visit::visit_expr_for_loop(self, node);
    }
    fn visit_expr_if(&mut self, node: &'a syn::ExprIf) {
        if matches!(
            node.then_branch.stmts.as_slice(),
            [syn::Stmt::Expr(syn::Expr::Continue(_), _)]
        ) {
            self.guards.push(&node.cond);
        }
        syn::visit::visit_expr_if(self, node);
    }
    fn visit_expr_range(&mut self, node: &'a syn::ExprRange) {
        self.ranges.push(node);
        syn::visit::visit_expr_range(self, node);
    }
}
fn whole_rows(mut expression: &syn::Expr) -> bool {
    for name in ["enumerate", "iter", "unwrap", "as_array"] {
        let syn::Expr::MethodCall(m) = super::unwrapped(expression) else {
            return false;
        };
        if m.method != name || !m.args.is_empty() {
            return false;
        }
        expression = &m.receiver;
    }
    matches!(super::unwrapped(expression),syn::Expr::Index(i) if super::string(&i.index).as_deref()==Some("cases")
        && matches!(i.expr.as_ref(),syn::Expr::Call(c) if c.args.is_empty() && matches!(c.func.as_ref(),syn::Expr::Path(p) if p.path.segments.iter().map(|s|s.ident.to_string()).collect::<Vec<_>>()==["inputs","input"])))
}
fn integer(expression: &syn::Expr) -> Option<usize> {
    if let syn::Expr::Lit(l) = super::unwrapped(expression) {
        if let syn::Lit::Int(n) = &l.lit {
            return n.base10_parse().ok();
        }
    }
    None
}
fn family_guard(function: &syn::ItemFn, form: &str) -> super::Result<()> {
    use syn::visit::Visit;
    let Some(syn::FnArg::Typed(arg)) = function.sig.inputs.first() else {
        return Err("missing family parameter".into());
    };
    let syn::Pat::Ident(param) = arg.pat.as_ref() else {
        return Err("unknown family parameter".into());
    };
    let prefix = form.rsplit_once('/').ok_or("invalid form")?.0;
    let template = format!("{prefix}/{{{}}}", param.ident);
    let mut facts = FamilyFacts::default();
    facts.visit_block(&function.block);
    if facts.loops.len() != 1 || facts.guards.len() != 1 || !whole_rows(&facts.loops[0].expr) {
        return Err("partial/unknown family row iteration".into());
    }
    let syn::Pat::Tuple(tuple) = facts.loops[0].pat.as_ref() else {
        return Err("unknown family row tuple".into());
    };
    let names = tuple
        .elems
        .iter()
        .map(|p| {
            if let syn::Pat::Ident(i) = p {
                Some(i.ident.to_string())
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("unknown family row bindings")?;
    if names.len() != 2 {
        return Err("unknown family row tuple arity".into());
    }
    let is_form = |e: &syn::Expr| {
        matches!(super::unwrapped(e),syn::Expr::Binary(b) if matches!(b.op,syn::BinOp::Ne(_))
        && super::form_index(&b.left,&names[1]) && matches!(super::unwrapped(&b.right),syn::Expr::Macro(m) if m.mac.path.is_ident("format") && syn::parse2::<syn::LitStr>(m.mac.tokens.clone()).is_ok_and(|s|s.value()==template)))
    };
    let guard = super::unwrapped(facts.guards[0]);
    if is_form(guard) {
        return Ok(());
    }
    let syn::Expr::Binary(or) = guard else {
        return Err("unknown family guard".into());
    };
    if !matches!(or.op, syn::BinOp::Or(_)) {
        return Err("partial family guard".into());
    }
    let shard = if is_form(&or.left) {
        &or.right
    } else if is_form(&or.right) {
        &or.left
    } else {
        return Err("missing complete family form guard".into());
    };
    let syn::Expr::Binary(ne) = super::unwrapped(shard) else {
        return Err("unknown family shard".into());
    };
    let syn::Expr::Binary(rem) = super::unwrapped(&ne.left) else {
        return Err("unknown family shard modulus".into());
    };
    let divisor = integer(&rem.right)
        .filter(|n| *n > 0)
        .ok_or("unknown shard divisor")?;
    if !matches!(ne.op, syn::BinOp::Ne(_))
        || !matches!(rem.op, syn::BinOp::Rem(_))
        || !super::path(&rem.left, &names[0])
        || !super::path(&ne.right, "worker")
        || !facts.ranges.iter().any(|range| {
            matches!(range.limits, syn::RangeLimits::HalfOpen(_))
                && range.start.as_deref().and_then(integer) == Some(0)
                && range.end.as_deref().and_then(integer) == Some(divisor)
        })
    {
        return Err("incomplete family worker partition".into());
    }
    Ok(())
}

#[cfg(test)]
mod support_tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;
    fn frozen() -> Value {
        serde_json::from_str(include_str!("../ge5_source_maps/cases.json")).unwrap()
    }
    const SOURCES: &[(&str, &str)] = &[
        (
            "ge5_binding_maps",
            include_str!("../../oracle_support/ge5_binding_roles.rs"),
        ),
        (
            "ge5_iteration_maps",
            include_str!("../../oracle_support/ge5_iteration_roles.rs"),
        ),
        (
            "ge5_object_maps",
            include_str!("../../oracle_support/ge5_object_roles.rs"),
        ),
        (
            "ge5_retained_decorator_maps",
            include_str!("../../oracle_support/ge5_preserved_decorator_roles.rs"),
        ),
        (
            "ge5_class_maps",
            include_str!("../../oracle_support/ge5_class_roles.rs"),
        ),
        (
            "ge5_class_retained",
            include_str!("../../oracle_support/ge5_class_retained.rs"),
        ),
        (
            "ge5_class_expressions",
            include_str!("../../oracle_support/ge5_class_expressions.rs"),
        ),
        (
            "ge5_metadata_classes",
            include_str!("../../oracle_support/ge5_metadata_classes.rs"),
        ),
    ];
    fn rows(
        stem: &str,
        source: &str,
        input: &Value,
    ) -> super::super::Result<Vec<super::super::Case>> {
        let (alias, _, helper, contracts) = support_spec(stem).unwrap();
        let ignored = super::super::super::rust_facts::parse(source)?
            .ignored_tests
            .into_iter()
            .map(|n| format!("{alias}::{n}"))
            .collect::<Vec<_>>();
        support_rows(alias, helper, contracts, source, &ignored, input)
    }
    #[test]
    fn support_modules_select_the_exact_existing_frozen_case_ids_and_classifications() {
        let input = frozen();
        for &(stem, source) in SOURCES {
            let forms: &[&str] = match stem {
                "ge5_binding_maps" => &[
                    "ge1_downlevel/array-nested-binding",
                    "ge1_downlevel/object-nested-binding",
                    "ge1_downlevel/mixed-nested-default",
                    "ge1_downlevel/nested-parameter",
                    "ge1_downlevel/binding-iterator-finally",
                ],
                "ge5_iteration_maps" => &["ge2_module_helpers/iterable-bindings-forof"],
                "ge5_object_maps" => &["ge2_module_helpers/object-rest-spread"],
                "ge5_retained_decorator_maps" => &[
                    "ge3_decorator_compositions/private-super-method",
                    "ge3_decorator_compositions/private-super-accessors",
                    "ge3_decorator_compositions/private-static-super",
                    "ge3_decorator_compositions/private-super-async-generator",
                ],
                "ge5_class_maps" => &super::super::ADJACENT,
                "ge5_class_retained" => &["ge1_downlevel/super-write-receiver-order"],
                "ge5_class_expressions" => &[
                    "ge3_decorator_compositions/class-expression-named",
                    "ge3_decorator_compositions/class-expression-anonymous",
                    "ge3_decorator_compositions/class-expression-per-evaluation",
                    "ge3_decorator_compositions/class-expression-computed-heritage",
                ],
                "ge5_metadata_classes" => &[
                    "ge3_decorator_compositions/imported-class",
                    "ge3_decorator_compositions/renamed-class",
                    "ge3_decorator_compositions/namespace-class",
                    "ge3_decorator_compositions/default-class",
                ],
                _ => unreachable!(),
            };
            let adjacent = [
                "legacy-parameter-decorator-cjs-es5-i0-n0",
                "legacy-parameter-decorator-esm-es5-i0-n0",
                "legacy-parameter-decorator-amd-es5-i0-n0",
                "legacy-parameter-decorator-umd-es5-i0-n0",
                "legacy-parameter-decorator-system-es5-i0-n0",
            ];
            let expected = input["cases"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    forms.contains(&row["form"].as_str().unwrap())
                        || stem == "ge5_metadata_classes"
                            && row["profile"] == "external"
                            && row["origin"]["id"]
                                .as_str()
                                .is_some_and(|id| adjacent.contains(&id))
                })
                .map(|(i, _)| format!("/cases/{i}"))
                .collect::<BTreeSet<_>>();
            let selected = rows(stem, source, &input).unwrap_or_else(|e| panic!("{stem}: {e}"));
            assert_eq!(
                selected
                    .iter()
                    .map(|c| c.selector.clone())
                    .collect::<BTreeSet<_>>(),
                expected,
                "{stem}"
            );
            for case in selected {
                let index: usize = case
                    .selector
                    .strip_prefix("/cases/")
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(
                    case.id,
                    format!("{}#{}", super::super::INPUT, case.selector)
                );
                assert_eq!(case.label, input["cases"][index]["id"].as_str().unwrap());
                assert_eq!(case.comparison, super::super::Comparison::Compared);
            }
        }
    }
    #[test]
    fn support_adapters_reject_partial_shards_filters_changed_form_and_script() {
        let input = frozen();
        let binding = SOURCES[0].1;
        let object = SOURCES[2].1;
        for (stem, source) in [
            (
                "ge5_binding_maps",
                binding.replace(".enumerate()", ".take(1).enumerate()"),
            ),
            (
                "ge5_binding_maps",
                binding.replace(
                    "family(\"array-nested-binding\", 51)",
                    "family(\"nested-parameter\", 51)",
                ),
            ),
            (
                "ge5_binding_maps",
                binding.replace("verify_ge5_binding_roles.cjs", "verify_ge5_unknown.cjs"),
            ),
            ("ge5_object_maps", object.replace("0..2", "0..1")),
        ] {
            assert!(
                rows(stem, &source, &input).is_err(),
                "{stem}: unexpected partial selector accepted"
            );
        }
        let (alias, _, helper, contracts) = support_spec("ge5_binding_maps").unwrap();
        assert!(support_rows(alias, helper, contracts, binding, &[], &input).is_err());
    }
    #[test]
    fn metadata_adjacent_selection_keeps_original_profile_and_origin_identity() {
        let source = SOURCES.last().unwrap().1;
        let mut input = frozen();
        for row in input["cases"].as_array_mut().unwrap() {
            if row["profile"] == "external"
                && row["origin"]["id"] == "legacy-parameter-decorator-cjs-es5-i0-n0"
            {
                row["profile"] = "inline".into();
            }
        }
        let selected = rows("ge5_metadata_classes", source, &input).unwrap();
        assert!(selected.iter().all(|c| {
            let i: usize = c.selector.strip_prefix("/cases/").unwrap().parse().unwrap();
            input["cases"][i]["origin"]["id"] != "legacy-parameter-decorator-cjs-es5-i0-n0"
        }));
    }
}
