// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete named, anonymous, per-evaluation and computed-heritage expression families.
#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 140 frozen class-expression products"]
fn all_class_expression_products_match_native_roles() {
    super::class_cli::run(
        |row| {
            matches!(
                row["form"].as_str().unwrap(),
                "ge3_decorator_compositions/class-expression-named"
                    | "ge3_decorator_compositions/class-expression-anonymous"
                    | "ge3_decorator_compositions/class-expression-per-evaluation"
                    | "ge3_decorator_compositions/class-expression-computed-heritage"
            )
        },
        140,
        "class-expressions",
        "verify_ge5_class_expressions.cjs",
    );
}

#[test]
fn preserved_class_expression_keyword_has_native_generated_role() {
    super::class_cli::run(
        |row| {
            let form = row["form"].as_str().unwrap();
            if !form.starts_with("ge3_decorator_compositions/class-expression-")
                || row["profile"] != "external"
            {
                return false;
            }
            let original = super::inputs::resolve(row);
            original["options"]["target"] == "ESNext"
                && original["options"]["useDefineForClassFields"] == true
        },
        16,
        "class-expression-boundaries",
        "verify_ge5_class_expression_boundaries.cjs",
    );
}

#[test]
fn ordinary_arrow_prefix_preserves_its_source_callable_role() {
    super::class_cli::run(
        |row| {
            row["id"] == super::inputs::input()["cases"][6390]["id"]
                || row["id"] == super::inputs::input()["cases"][6391]["id"]
        },
        2,
        "class-expression-arrow-prefix",
        "verify_ge5_class_expression_arrow_prefix.cjs",
    );
}

#[test]
fn retained_class_expression_closing_has_native_generated_role() {
    super::class_cli::run(
        |row| {
            if row["form"] != "ge3_decorator_compositions/class-expression-named"
                || row["profile"] != "external"
            {
                return false;
            }
            super::inputs::resolve(row)["options"]["target"] != "ES5"
        },
        24,
        "class-expression-closings",
        "verify_ge5_class_expression_closings.cjs",
    );
}

#[test]
fn decorated_class_expression_factory_retains_its_source_context() {
    super::class_cli::run(
        |row| {
            if !row["form"]
                .as_str()
                .unwrap()
                .starts_with("ge3_decorator_compositions/class-expression-")
                || row["profile"] != "external"
            {
                return false;
            }
            let options = super::inputs::resolve(row);
            matches!(
                options["options"]["target"].as_str(),
                Some("ES5" | "ES2015")
            ) && options["options"]["useDefineForClassFields"] == false
        },
        32,
        "class-expression-prefix",
        "verify_ge5_class_expression_prefix.cjs",
    );
}

#[test]
fn anonymous_static_class_wrapper_keeps_the_return_and_generated_roles() {
    super::class_cli::run(
        |row| {
            row["form"] == "ge3_decorator_compositions/class-expression-named"
                && row["profile"] == "external"
                && matches!(
                    super::inputs::resolve(row)["options"]["target"].as_str(),
                    Some("ES5" | "ES2015")
                )
        },
        16,
        "class-expression-plain-boundaries",
        "verify_ge5_class_expression_plain_boundaries.cjs",
    );
}

#[test]
fn es5_anonymous_class_keeps_distinct_factory_and_constructor_origins() {
    super::class_cli::run(
        |row| {
            row["form"] == "ge3_decorator_compositions/class-expression-named"
                && row["profile"] == "external"
                && super::inputs::resolve(row)["options"]["target"] == "ES5"
        },
        8,
        "class-expression-factory",
        "verify_ge5_class_expression_factory.cjs",
    );
}

#[test]
fn lowered_class_expression_initializer_phases_keep_native_source_roles() {
    super::class_cli::run(
        |row| {
            if !row["form"]
                .as_str()
                .unwrap()
                .starts_with("ge3_decorator_compositions/class-expression-")
                || row["profile"] != "external"
            {
                return false;
            }
            let original = super::inputs::resolve(row);
            original["options"]["target"] == "ES2022"
                || original["options"]["target"] == "ESNext"
                    && original["options"]["useDefineForClassFields"] == false
        },
        48,
        "class-expression-initializers",
        "verify_ge5_class_expression_initializers.cjs",
    );
}

#[test]
fn lowered_class_expression_metadata_statements_keep_native_source_roles() {
    super::class_cli::run(
        |row| {
            if !row["form"]
                .as_str()
                .unwrap()
                .starts_with("ge3_decorator_compositions/class-expression-")
                || row["profile"] != "external"
            {
                return false;
            }
            let original = super::inputs::resolve(row);
            original["options"]["target"] == "ES2022"
                || original["options"]["target"] == "ESNext"
                    && original["options"]["useDefineForClassFields"] == false
        },
        48,
        "class-expression-metadata-calls",
        "verify_ge5_class_expression_metadata_calls.cjs",
    );
}

#[test]
fn generated_class_expression_header_keeps_its_following_gap_unmapped() {
    super::class_cli::run(
        |row| {
            if !row["form"]
                .as_str()
                .unwrap()
                .starts_with("ge3_decorator_compositions/class-expression-")
                || row["profile"] != "external"
            {
                return false;
            }
            let original = super::inputs::resolve(row);
            original["options"]["target"] == "ES2022"
                || original["options"]["target"] == "ESNext"
                    && original["options"]["useDefineForClassFields"] == false
        },
        48,
        "class-expression-generated-gaps",
        "verify_ge5_class_expression_generated_gaps.cjs",
    );
}

#[test]
fn es5_define_literal_method_preserves_its_original_key_and_body_completion() {
    super::class_cli::run(
        |row| {
            matches!(
                row["form"].as_str(),
                Some(
                    "ge3_decorator_compositions/class-expression-named"
                        | "ge3_decorator_compositions/class-expression-computed-heritage"
                )
            ) && row["profile"] == "external"
                && {
                    let original = super::inputs::resolve(row);
                    original["options"]["target"] == "ES5"
                        && original["options"]["useDefineForClassFields"] == true
                }
        },
        8,
        "class-expression-literal-method",
        "verify_ge5_class_expression_literal_method.cjs",
    );
}

#[test]
fn lowered_decorated_expression_factory_retains_generated_roles() {
    super::class_cli::run(
        |row| {
            row["form"].as_str().is_some_and(|form| {
                form.starts_with("ge3_decorator_compositions/class-expression-")
            }) && super::inputs::resolve(row)["options"]["target"] == "ES5"
        },
        44,
        "class-expression-decorated-factory",
        "verify_ge5_class_expression_decorated_factory.cjs",
    );
}

#[test]
fn captured_expression_heritage_keeps_following_generated_state_unmapped() {
    super::class_cli::run(
        |row| {
            if row["form"] != "ge3_decorator_compositions/class-expression-computed-heritage" {
                return false;
            }
            let original = super::inputs::resolve(row);
            !(original["options"]["target"] == "ESNext"
                && original["options"]["useDefineForClassFields"] == true)
        },
        31,
        "class-expression-heritage-state",
        "verify_ge5_class_expression_heritage_state.cjs",
    );
}

#[test]
fn lowered_decorated_expression_continuation_keeps_its_native_endpoint() {
    super::class_cli::run(
        |row| {
            if !row["form"].as_str().is_some_and(|form| {
                form.starts_with("ge3_decorator_compositions/class-expression-")
            }) {
                return false;
            }
            let original = super::inputs::resolve(row);
            !(original["options"]["target"] == "ESNext"
                && original["options"]["useDefineForClassFields"] == true)
        },
        124,
        "class-expression-evaluation-endpoints",
        "verify_ge5_class_expression_endpoints.cjs",
    );
}

#[test]
fn decorated_expression_heritage_keeps_original_header_and_inheritance_roles() {
    super::class_cli::run(
        |row| {
            if row["form"] != "ge3_decorator_compositions/class-expression-computed-heritage" {
                return false;
            }
            let original = super::inputs::resolve(row);
            !(original["options"]["target"] == "ESNext"
                && original["options"]["useDefineForClassFields"] == true)
        },
        31,
        "class-expression-heritage-roles",
        "verify_ge5_class_expression_heritage_roles.cjs",
    );
}
