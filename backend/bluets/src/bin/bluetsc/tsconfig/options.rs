// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Validate supported option shapes and expose pinned TypeScript defaults.

use super::*;

pub(super) const STRICT: &[&str] = &[
    "noImplicitAny",
    "noImplicitThis",
    "strictNullChecks",
    "strictFunctionTypes",
    "strictBindCallApply",
    "strictPropertyInitialization",
    "strictBuiltinIteratorReturn",
    "alwaysStrict",
    "useUnknownInCatchVariables",
];

const BOOLEAN: &[&str] = &[
    "removeComments",
    "emitBOM",
    "inlineSources",
    "stripInternal",
    "preserveValueImports",
    "downlevelIteration",
    "importHelpers",
    "noEmitHelpers",
    "verbatimModuleSyntax",
    "sourceMap",
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "isolatedDeclarations",
    "useDefineForClassFields",
    "preserveConstEnums",
    "isolatedModules",
    "esModuleInterop",
    "experimentalDecorators",
    "emitDecoratorMetadata",
    "strict",
    "allowSyntheticDefaultImports",
    "resolvePackageJsonExports",
    "resolvePackageJsonImports",
    "resolveJsonModule",
    "noUnusedLocals",
    "noUnusedParameters",
    "noImplicitReturns",
    "noImplicitOverride",
    "noFallthroughCasesInSwitch",
    "exactOptionalPropertyTypes",
    "noUncheckedIndexedAccess",
    "allowJs",
    "checkJs",
    "noEmit",
    "noEmitOnError",
    "diagnostics",
    "pretty",
    "listFiles",
    "listEmittedFiles",
    "skipLibCheck",
    "forceConsistentCasingInFileNames",
    "allowImportingTsExtensions",
    "rewriteRelativeImportExtensions",
];

pub(super) fn validate(
    value: &Value,
    directory: &Path,
    reader: &Reader,
) -> Result<Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "compilerOptions must be an object".to_string())?;
    let mut result = Map::new();
    for (name, value) in object {
        if !BOOLEAN.contains(&name.as_str())
            && !STRICT.contains(&name.as_str())
            && !matches!(
                name.as_str(),
                "target"
                    | "newLine"
                    | "sourceRoot"
                    | "mapRoot"
                    | "importsNotUsedAsValues"
                    | "lib"
                    | "module"
                    | "moduleResolution"
                    | "jsx"
                    | "jsxFactory"
                    | "jsxFragmentFactory"
                    | "jsxImportSource"
                    | "outDir"
                    | "declarationDir"
                    | "rootDir"
                    | "baseUrl"
                    | "paths"
                    | "rootDirs"
                    | "types"
                    | "typeRoots"
            )
        {
            return Err(format!("unknown or unsupported compiler option `{name}`"));
        }
        if value.is_null() {
            result.insert(name.clone(), Value::Null);
            continue;
        }
        if let Some(checked) = resolution_options::validate(name, value, directory, reader)? {
            result.insert(name.clone(), checked);
            continue;
        }
        let checked = if name == "lib" {
            let libraries = value
                .as_array()
                .ok_or_else(|| "compiler option `lib` requires an array".to_string())?;
            let mut selected = Vec::new();
            for library in libraries {
                let name = library
                    .as_str()
                    .ok_or_else(|| "compiler option `lib` requires strings".to_string())?;
                let target = EcmaTarget::parse(name)
                    .ok_or_else(|| format!("unsupported compiler option `lib` value `{name}`"))?;
                selected.push(json!(if target == EcmaTarget::Es2015 {
                    "es6"
                } else {
                    target.as_str()
                }));
            }
            Value::Array(selected)
        } else if BOOLEAN.contains(&name.as_str()) || STRICT.contains(&name.as_str()) {
            Value::Bool(
                value
                    .as_bool()
                    .ok_or_else(|| format!("compiler option `{name}` requires a boolean"))?,
            )
        } else {
            let string = value
                .as_str()
                .ok_or_else(|| format!("compiler option `{name}` requires a string"))?;
            match name.as_str() {
                "newLine" => enumeration(name, string, &["lf", "crlf"])?,
                "sourceRoot" | "mapRoot" => Value::String(string.to_string()),
                "importsNotUsedAsValues" => {
                    enumeration(name, string, &["remove", "preserve", "error"])?
                }
                "target" => {
                    let target = EcmaTarget::parse(string).ok_or_else(|| {
                        format!("unsupported compiler option `{name}` value `{string}`")
                    })?;
                    json!(if target == EcmaTarget::Es2015 {
                        "es6"
                    } else {
                        target.as_str()
                    })
                }
                "module" => {
                    let module = enumeration(
                        name,
                        string,
                        &[
                            "esnext", "es2020", "es2022", "commonjs", "es2015", "es6", "amd",
                            "umd", "system", "node16", "nodenext",
                        ],
                    )?;
                    if module == "es2015" {
                        json!("es6")
                    } else {
                        module
                    }
                }
                "moduleResolution" => {
                    let value = enumeration(
                        name,
                        string,
                        &["classic", "node", "node10", "node16", "nodenext", "bundler"],
                    )?;
                    if value == "node" {
                        Value::String("node10".to_string())
                    } else {
                        value
                    }
                }
                "jsx" => enumeration(
                    name,
                    string,
                    &[
                        "preserve",
                        "react-native",
                        "react",
                        "react-jsx",
                        "react-jsxdev",
                    ],
                )?,
                "jsxFactory" | "jsxFragmentFactory" | "jsxImportSource" => {
                    Value::String(string.to_string())
                }
                "outDir" | "rootDir" | "declarationDir" => {
                    let path = clean_path(&directory.join(string));
                    reader.authorize_future(&path, name, false)?;
                    Value::String(path.to_string_lossy().into_owned())
                }
                _ => return Err(format!("unknown or unsupported compiler option `{name}`")),
            }
        };
        if name == "preserveValueImports" && checked == true
            || name == "importsNotUsedAsValues" && checked != "remove"
        {
            return Err(format!("removed compiler option `{name}`"));
        }
        result.insert(name.clone(), checked);
    }
    Ok(result)
}

fn enumeration(name: &str, value: &str, allowed: &[&str]) -> Result<Value, String> {
    let value = value.to_ascii_lowercase();
    if allowed.contains(&value.as_str()) {
        Ok(Value::String(value))
    } else {
        Err(format!(
            "unsupported compiler option `{name}` value `{value}`"
        ))
    }
}

pub(super) fn checking(
    raw: &Map<String, Value>,
) -> Result<blueice_bluets::CheckingOptions, String> {
    let strict = raw.get("strict").and_then(Value::as_bool).unwrap_or(false);
    let flag = |name: &str| {
        raw.get(name)
            .and_then(Value::as_bool)
            .unwrap_or(STRICT.contains(&name) && strict)
    };
    let options = blueice_bluets::CheckingOptions {
        no_implicit_any: flag("noImplicitAny"),
        no_implicit_this: flag("noImplicitThis"),
        strict_null_checks: flag("strictNullChecks"),
        strict_function_types: flag("strictFunctionTypes"),
        strict_bind_call_apply: flag("strictBindCallApply"),
        strict_property_initialization: flag("strictPropertyInitialization"),
        strict_builtin_iterator_return: flag("strictBuiltinIteratorReturn"),
        always_strict: flag("alwaysStrict"),
        use_unknown_in_catch_variables: flag("useUnknownInCatchVariables"),
        no_unused_locals: flag("noUnusedLocals"),
        no_unused_parameters: flag("noUnusedParameters"),
        no_implicit_returns: flag("noImplicitReturns"),
        no_implicit_override: flag("noImplicitOverride"),
        no_fallthrough_cases_in_switch: flag("noFallthroughCasesInSwitch"),
        exact_optional_property_types: flag("exactOptionalPropertyTypes"),
        no_unchecked_indexed_access: flag("noUncheckedIndexedAccess"),
    };
    if !options.strict_null_checks
        && (options.strict_property_initialization || options.exact_optional_property_types)
    {
        return Err(
            "strictPropertyInitialization and exactOptionalPropertyTypes require strictNullChecks"
                .to_string(),
        );
    }
    Ok(options)
}

pub(super) fn effective(raw: &Map<String, Value>, directory: &Path) -> Map<String, Value> {
    let mut values = raw
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Map<_, _>>();
    let target = raw.get("target").and_then(Value::as_str).unwrap_or("es5");
    let module = raw
        .get("module")
        .and_then(Value::as_str)
        .unwrap_or(if target == "es5" { "commonjs" } else { "es6" });
    if !raw.contains_key("module") && target != "es5" {
        values.insert("module".to_string(), json!(module));
    }
    let resolution = raw
        .get("moduleResolution")
        .and_then(Value::as_str)
        .unwrap_or(match module {
            "commonjs" => "node10",
            "node16" => "node16",
            "nodenext" => "nodenext",
            _ => "classic",
        });
    if !raw.contains_key("moduleResolution")
        && resolution != "node10"
        && (raw.contains_key("module") || raw.contains_key("target"))
    {
        values.insert("moduleResolution".to_string(), json!(resolution));
    }
    if raw.get("strict").and_then(Value::as_bool) == Some(true) {
        for name in STRICT {
            if !raw.contains_key(*name) {
                values.insert((*name).to_string(), json!(true));
            }
        }
    }
    let bundler = resolution == "bundler";
    let modern_resolution = matches!(resolution, "node16" | "nodenext" | "bundler");
    let interop = raw
        .get("esModuleInterop")
        .and_then(Value::as_bool)
        .unwrap_or(matches!(module, "node16" | "nodenext"));
    let implied = [
        ("esModuleInterop", interop),
        (
            "isolatedModules",
            raw.get("verbatimModuleSyntax").and_then(Value::as_bool) == Some(true),
        ),
        ("allowSyntheticDefaultImports", interop || bundler),
        ("resolvePackageJsonExports", modern_resolution),
        ("resolvePackageJsonImports", modern_resolution),
        ("resolveJsonModule", bundler),
        (
            "preserveConstEnums",
            raw.get("isolatedModules").and_then(Value::as_bool) == Some(true),
        ),
        (
            "useDefineForClassFields",
            EcmaTarget::parse(target).is_some_and(|target| target >= EcmaTarget::Es2022),
        ),
        (
            "allowJs",
            raw.get("checkJs").and_then(Value::as_bool) == Some(true),
        ),
        (
            "allowImportingTsExtensions",
            raw.get("rewriteRelativeImportExtensions")
                .and_then(Value::as_bool)
                == Some(true),
        ),
    ];
    for (name, yes) in implied {
        if yes && !raw.contains_key(name) {
            values.entry(name.to_string()).or_insert(json!(true));
        }
    }
    for name in ["outDir", "rootDir", "declarationDir"] {
        if let Some(Value::String(path)) = values.get_mut(name) {
            *path = relative_text(directory, Path::new(path));
        }
    }
    resolution_options::show(&mut values, directory);
    values
}

pub(super) fn owner_options(
    raw: &Map<String, Value>,
    root: &Path,
) -> Result<Map<String, Value>, String> {
    let mut owner = Map::new();
    let computed = effective(raw, root);
    owner.insert("target".to_string(), json!("es5"));
    owner.insert("inlineSources".to_string(), json!(false));
    for name in [
        "target",
        "lib",
        "downlevelIteration",
        "importHelpers",
        "noEmitHelpers",
        "verbatimModuleSyntax",
        "module",
        "resolveJsonModule",
        "sourceMap",
        "declaration",
        "declarationMap",
        "emitDeclarationOnly",
        "isolatedDeclarations",
        "removeComments",
        "newLine",
        "emitBOM",
        "inlineSources",
        "stripInternal",
        "sourceRoot",
        "mapRoot",
        "useDefineForClassFields",
        "preserveConstEnums",
        "isolatedModules",
        "esModuleInterop",
        "experimentalDecorators",
        "emitDecoratorMetadata",
        "jsx",
        "jsxFactory",
        "jsxFragmentFactory",
        "jsxImportSource",
    ] {
        if let Some(value) = computed.get(name) {
            owner.insert(name.to_string(), value.clone());
        }
    }
    for name in ["outDir", "declarationDir"] {
        if let Some(Value::String(path)) = raw.get(name) {
            let relative = Path::new(path)
                .strip_prefix(root)
                .map_err(|_| format!("{name} {path} is outside project root"))?;
            owner.insert(name.to_string(), json!(output_path(relative)));
        }
    }
    if let Some(value) = computed.get("moduleResolution").and_then(Value::as_str) {
        if matches!(value, "node16" | "nodenext" | "bundler")
            || raw.contains_key("moduleResolution")
            || value == "node10" && raw.get("importHelpers").and_then(Value::as_bool) == Some(true)
        {
            owner.insert("moduleResolution".to_string(), json!(value));
        }
    }
    Ok(owner)
}
