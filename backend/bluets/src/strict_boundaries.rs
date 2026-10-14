// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admission of owner-selected emitted runtime crossing descriptors.

use crate::compiler::{is_declaration_module, CompilerOptions, Project, RuntimePolicy};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::emitter::{EmittedRuntimeBoundary, EmittedRuntimeSite, EmittedStrictModule};
use crate::parser::{Declaration, FunctionBodyItem, FunctionDeclaration, Module, TextEdit, Type};
use crate::syntax::{Token, TokenKind};
use std::collections::BTreeSet;

pub(crate) const HELPER_V1_VERSION: &str = "bluets-runtime-helper-v1";
pub(crate) const HELPER_ALIAS: &str = "__bluetsValidateStringV1";
pub(crate) const HELPER_V1_FILE: &str = "bluets.runtime-helper.v1.mjs";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_EMITTED_STRING_BYTES: usize = 1_048_576;

pub(crate) fn validate_descriptors(
    project: &Project,
    options: &CompilerOptions,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut contract_ids = BTreeSet::new();
    let mut selected_functions = BTreeSet::new();
    for boundary in &options.strict_runtime_boundaries {
        let reject = |message: &str| {
            Diagnostic::error(
                DiagnosticCode::InvalidContract,
                boundary.span.clone(),
                message,
            )
        };
        if options.runtime_policy != RuntimePolicy::StrictRuntime {
            diagnostics.push(reject("emitted boundary requires strict-runtime policy"));
        }
        if boundary.helper_version != HELPER_V1_VERSION {
            diagnostics.push(reject("unsupported emitted runtime helper version"));
        }
        if u64::try_from(boundary.max_string_bytes).map_or(true, |limit| limit > MAX_SAFE_INTEGER) {
            diagnostics.push(reject(
                "string-byte limit exceeds helper v1's safe integer range",
            ));
        }
        if boundary.max_string_bytes > MAX_EMITTED_STRING_BYTES {
            diagnostics.push(reject(
                "string-byte limit exceeds the first emitted profile",
            ));
        }
        if boundary.contract_id.is_empty()
            || boundary.contract_id.len() > 128
            || !boundary
                .contract_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            diagnostics.push(reject("invalid emitted contract ID"));
        }
        if !contract_ids.insert(boundary.contract_id.as_str()) {
            diagnostics.push(reject("duplicate emitted contract ID"));
        }
        if !selected_functions.insert((&boundary.span.module, &boundary.function)) {
            diagnostics.push(reject("duplicate emitted function boundary"));
        }
        if !canonical_emitted_module_id(&boundary.span.module) {
            diagnostics.push(reject(
                "emitted boundary module must have a canonical root-relative .ts identity",
            ));
        }
        let matched = !is_declaration_module(&boundary.span.module)
            && project
                .modules
                .get(&boundary.span.module)
                .is_some_and(|module| {
                    module.declarations.iter().any(|declaration| {
                        matches!(declaration, Declaration::Function(function)
                            if function.name == boundary.function
                                && function.exported
                                && !function.default_export
                                && !function.declared
                                && !function.overload
                                && function.span == boundary.span)
                    })
                });
        if !matched {
            diagnostics.push(reject(
                "emitted boundary does not match an exported function at the exact source span",
            ));
        }
    }
    if options.runtime_policy == RuntimePolicy::StrictRuntime
        && !options.strict_runtime_boundaries.is_empty()
    {
        diagnostics.extend(validate_profile(project, options));
    }
    diagnostics
}

fn validate_profile(project: &Project, options: &CompilerOptions) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for ambient in &options.ambient_declaration_modules {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidContract,
            SourceSpan::new(&ambient.id, 0, 0),
            "emitted strict profile cannot use ambient declarations",
        ));
    }
    for (module_id, module) in &project.modules {
        if is_declaration_module(module_id) {
            continue;
        }
        if !canonical_emitted_module_id(module_id) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidContract,
                SourceSpan::new(module_id, 0, 0),
                "emitted strict module must have a canonical root-relative .ts identity",
            ));
        }
        if module.source.contains(HELPER_ALIAS) {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidContract,
                SourceSpan::new(module_id, 0, 0),
                "emitted strict source uses the reserved helper alias",
            ));
        }
        let mut selected_count = 0usize;
        for declaration in &module.declarations {
            match declaration {
                Declaration::Import(import) if import.type_only => {}
                Declaration::TypeExport(_)
                | Declaration::TypeAlias(_)
                | Declaration::Interface(_) => {}
                Declaration::Function(function) => {
                    let selected = options.strict_runtime_boundaries.iter().any(|boundary| {
                        boundary.span == function.span && boundary.function == function.name
                    });
                    if selected {
                        selected_count += 1;
                        if !supported_function(function) {
                            diagnostics.push(Diagnostic::error(
                                DiagnosticCode::InvalidContract,
                                function.span.clone(),
                                "unsupported exported string function in emitted strict profile",
                            ));
                        }
                    } else {
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InvalidContract,
                            function.span.clone(),
                            "runtime function has no owner-selected emitted boundary",
                        ));
                    }
                }
                other => diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidContract,
                    other.span().clone(),
                    "unsupported runtime declaration or import in emitted strict profile",
                )),
            }
        }
        if selected_count == 0 {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidContract,
                SourceSpan::new(module_id, 0, 0),
                "emitted strict module has no owner-selected boundary",
            ));
        }
    }
    diagnostics
}

fn canonical_emitted_module_id(module_id: &str) -> bool {
    module_id.ends_with(".ts")
        && !module_id.ends_with(".d.ts")
        && module_id.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(':')
                && !part.contains('\\')
        })
}

fn supported_function(function: &FunctionDeclaration) -> bool {
    if !function.exported
        || function.default_export
        || function.declared
        || function.overload
        || function.async_function
        || !function.type_parameters.is_empty()
        || function.parameters.is_empty()
        || function.parameters.len() > 16
        || function.return_type != Some(Type::String)
    {
        return false;
    }
    let mut parameter_names = BTreeSet::new();
    for parameter in &function.parameters {
        if parameter.rest
            || parameter.optional
            || parameter.default.is_some()
            || parameter.annotation != Some(Type::String)
            || !parameter_names.insert(parameter.name.as_str())
        {
            return false;
        }
    }
    let [FunctionBodyItem::Return { tokens, .. }] = function.body.as_slice() else {
        return false;
    };
    supported_string_expression(tokens, &parameter_names)
}

fn supported_string_expression(tokens: &[Token], parameters: &BTreeSet<&str>) -> bool {
    if tokens.len() > 128 {
        return false;
    }
    let mut needs_operand = true;
    let mut depth = 0usize;
    for token in tokens {
        if needs_operand {
            match token.text.as_str() {
                "(" => depth += 1,
                _ if token.kind == TokenKind::Identifier
                    && parameters.contains(token.text.as_str()) =>
                {
                    needs_operand = false;
                }
                _ if token.kind == TokenKind::String => needs_operand = false,
                _ => return false,
            }
        } else {
            match token.text.as_str() {
                "+" => needs_operand = true,
                ")" if depth > 0 => depth -= 1,
                _ => return false,
            }
        }
    }
    !needs_operand && depth == 0
}

/// Produces only edits for an already-admitted strict module. A missing source
/// location is still an error here, so a later parser change cannot silently
/// publish a strict artifact without all required runtime calls.
pub(crate) fn plan_emission(
    module: &Module,
    options: &CompilerOptions,
) -> Result<Option<(Vec<TextEdit>, EmittedStrictModule)>, Diagnostic> {
    if options.runtime_policy != RuntimePolicy::StrictRuntime
        || options.strict_runtime_boundaries.is_empty()
    {
        return Ok(None);
    }
    let mut selected = options
        .strict_runtime_boundaries
        .iter()
        .filter(|boundary| boundary.span.module == module.id)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(emission_error(
            module,
            "strict module has no emitted boundary",
        ));
    }
    selected.sort_by_key(|boundary| boundary.span.start);
    let helper_path = format!(
        "{}{}",
        if module.id.contains('/') {
            "../".repeat(module.id.split('/').count() - 1)
        } else {
            "./".to_string()
        },
        HELPER_V1_FILE
    );
    let import_text =
        format!("import {{ validateStringV1 as {HELPER_ALIAS} }} from '{helper_path}';\n");
    let mut edits = vec![TextEdit {
        start: 0,
        end: 0,
        replacement: import_text.clone(),
    }];
    let mut record = EmittedStrictModule {
        helper_version: HELPER_V1_VERSION.to_string(),
        helper_import: EmittedRuntimeSite {
            source_span: SourceSpan::new(&module.id, 0, 0),
            generated_start: 0,
            expected_text: import_text,
        },
        boundaries: Vec::new(),
    };
    for boundary in selected {
        let function = module
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function)
                    if function.span == boundary.span && function.name == boundary.function =>
                {
                    Some(function)
                }
                _ => None,
            });
        let Some(function) = function else {
            return Err(emission_error(
                module,
                "strict function disappeared before emission",
            ));
        };
        let Some(body_open) = function.body_open else {
            return Err(emission_error(
                module,
                "strict function has no body opening brace",
            ));
        };
        if module.source.get(body_open..body_open + 1) != Some("{") {
            return Err(emission_error(module, "strict function body opening moved"));
        }
        let mut ingress_text = String::new();
        let mut ingress = Vec::new();
        for parameter in &function.parameters {
            let call = format!(
                "{HELPER_ALIAS}({}, {})",
                parameter.name, boundary.max_string_bytes
            );
            ingress_text.push_str("\n  ");
            ingress_text.push_str(&call);
            ingress_text.push(';');
            ingress.push(EmittedRuntimeSite {
                source_span: parameter.span.clone(),
                generated_start: 0,
                expected_text: call,
            });
        }
        edits.push(TextEdit {
            start: body_open + 1,
            end: body_open + 1,
            replacement: ingress_text,
        });
        let [FunctionBodyItem::Return { tokens, .. }] = function.body.as_slice() else {
            return Err(emission_error(
                module,
                "strict return disappeared before emission",
            ));
        };
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return Err(emission_error(module, "strict return has no expression"));
        };
        let Some(expression) = module.source.get(first.start..last.end) else {
            return Err(emission_error(
                module,
                "strict return source span is invalid",
            ));
        };
        let egress_text = format!(
            "{HELPER_ALIAS}({expression}, {})",
            boundary.max_string_bytes
        );
        edits.push(TextEdit {
            start: first.start,
            end: first.start,
            replacement: format!("{HELPER_ALIAS}("),
        });
        edits.push(TextEdit {
            start: last.end,
            end: last.end,
            replacement: format!(", {})", boundary.max_string_bytes),
        });
        record.boundaries.push(EmittedRuntimeBoundary {
            contract_id: boundary.contract_id.clone(),
            function: function.name.clone(),
            source_span: function.span.clone(),
            max_string_bytes: boundary.max_string_bytes,
            ingress,
            egress: EmittedRuntimeSite {
                source_span: SourceSpan::new(&module.id, first.start, last.end),
                generated_start: 0,
                expected_text: egress_text,
            },
        });
    }
    Ok(Some((edits, record)))
}

pub(crate) fn locate_emitted_calls(
    module: &Module,
    javascript: &str,
    record: &mut EmittedStrictModule,
) -> Result<(), Diagnostic> {
    if !javascript.starts_with(&record.helper_import.expected_text) {
        return Err(emission_error(
            module,
            "strict helper import was lost during emission",
        ));
    }
    let marker = format!("{HELPER_ALIAS}(");
    let mut offsets = javascript.match_indices(&marker).map(|(offset, _)| offset);
    for boundary in &mut record.boundaries {
        for site in boundary
            .ingress
            .iter_mut()
            .chain(std::iter::once(&mut boundary.egress))
        {
            let Some(start) = offsets.next() else {
                return Err(emission_error(
                    module,
                    "strict helper call was lost during emission",
                ));
            };
            if !javascript[start..].starts_with(&site.expected_text) {
                return Err(emission_error(
                    module,
                    "strict helper call changed during emission",
                ));
            }
            site.generated_start = start;
        }
    }
    if offsets.next().is_some() {
        return Err(emission_error(
            module,
            "unexpected strict helper call in output",
        ));
    }
    Ok(())
}

fn emission_error(module: &Module, message: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::InvalidContract,
        SourceSpan::new(&module.id, 0, 0),
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{compile, MapLoader, ModuleSource, StrictRuntimeBoundary};
    use crate::diagnostic::SourceSpan;

    const MODULE: &str = "src/main.ts";
    const SOURCE: &str = "export function echo(value: string): string { return value; }\n";

    fn boundary() -> StrictRuntimeBoundary {
        StrictRuntimeBoundary {
            contract_id: "echo-string-v1".to_string(),
            function: "echo".to_string(),
            span: SourceSpan::new(MODULE, 0, SOURCE.find('}').unwrap() + 1),
            max_string_bytes: 64,
            helper_version: HELPER_V1_VERSION.to_string(),
        }
    }

    fn compile_with(
        boundaries: Vec<StrictRuntimeBoundary>,
        policy: RuntimePolicy,
    ) -> crate::Compilation {
        let loader = MapLoader::from([ModuleSource::new(MODULE, SOURCE)]);
        compile(
            MODULE,
            &loader,
            CompilerOptions {
                runtime_policy: policy,
                strict_runtime_boundaries: boundaries,
                ..CompilerOptions::default()
            },
        )
    }

    #[test]
    fn exact_owner_descriptor_is_fingerprinted_and_stale_or_duplicate_descriptors_refuse() {
        let valid = compile_with(vec![boundary()], RuntimePolicy::StrictRuntime);
        assert!(!valid.has_errors(), "{:?}", valid.diagnostics);
        let first_fingerprint = valid.output.unwrap().fingerprint;
        let mut changed_budget = boundary();
        changed_budget.max_string_bytes = 63;
        let changed = compile_with(vec![changed_budget], RuntimePolicy::StrictRuntime);
        assert!(!changed.has_errors(), "{:?}", changed.diagnostics);
        assert_ne!(first_fingerprint, changed.output.unwrap().fingerprint);

        let mut stale = boundary();
        stale.span.end -= 1;
        let rejected = compile_with(vec![stale], RuntimePolicy::StrictRuntime);
        assert!(rejected.has_errors());
        assert!(rejected.output.is_none());
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidContract
                && diagnostic.message.contains("exact source span")
        }));

        let duplicate = compile_with(vec![boundary(), boundary()], RuntimePolicy::StrictRuntime);
        assert!(duplicate.output.is_none());
        assert!(duplicate
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("duplicate emitted contract ID") }));
    }

    #[test]
    fn wrong_policy_helper_and_limit_refuse_before_emission() {
        let wrong_policy = compile_with(vec![boundary()], RuntimePolicy::Checked);
        assert!(wrong_policy.output.is_none());
        assert!(wrong_policy.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("requires strict-runtime policy")
        }));

        let mut wrong_helper = boundary();
        wrong_helper.helper_version = "bluets-runtime-helper-v2".to_string();
        let refused = compile_with(vec![wrong_helper], RuntimePolicy::StrictRuntime);
        assert!(refused.output.is_none());

        let mut invalid_limit = boundary();
        invalid_limit.max_string_bytes = 1_048_577;
        let refused = compile_with(vec![invalid_limit], RuntimePolicy::StrictRuntime);
        assert!(refused.output.is_none());
    }

    #[test]
    fn admits_only_explicit_string_parameters_and_pure_string_returns() {
        let source = "export function join(left: string, right: string): string { return (left + ':') + right; }\n";
        let boundary = StrictRuntimeBoundary {
            contract_id: "join-string-v1".to_string(),
            function: "join".to_string(),
            span: SourceSpan::new(MODULE, 0, source.find('}').unwrap() + 1),
            max_string_bytes: 64,
            helper_version: HELPER_V1_VERSION.to_string(),
        };
        let loader = MapLoader::from([ModuleSource::new(MODULE, source)]);
        let result = compile(
            MODULE,
            &loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![boundary],
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.as_ref().unwrap().artifacts[MODULE];
        let record = artifact.strict_runtime.as_ref().unwrap();
        assert_eq!(record.helper_version, HELPER_V1_VERSION);
        assert!(artifact.javascript.starts_with(&format!(
            "import {{ validateStringV1 as {HELPER_ALIAS} }} from '../{HELPER_V1_FILE}';\n"
        )));
        assert_eq!(record.boundaries.len(), 1);
        assert_eq!(record.boundaries[0].ingress.len(), 2);
        assert_eq!(record.boundaries[0].contract_id, "join-string-v1");
        for site in record.boundaries[0]
            .ingress
            .iter()
            .chain(std::iter::once(&record.boundaries[0].egress))
        {
            assert!(artifact.javascript[site.generated_start..].starts_with(&site.expected_text));
        }
        assert!(artifact
            .javascript
            .contains(&format!("return {HELPER_ALIAS}((left + ':') + right, 64);")));
    }

    #[test]
    fn refuses_ambient_or_unsupported_runtime_shapes_even_with_a_matching_descriptor() {
        let cases = [
            "export async function echo(value: string): string { return value; }",
            "export function echo(value?: string): string { return value; }",
            "export function echo(value: string): string { return value.toString(); }",
            "export function echo(value: string): string { return globalText(); }",
            "export function echo(value: string): string { const copy = value; return copy; }",
            "export function echo(value: string): string { if (value) { return value; } return ''; }",
            "export function echo(value: string): string { return value; } export const extra: string = 'x';",
            "export function echo(value: string): string { return value; } declare function ambient(): string;",
        ];
        for source in cases {
            let mut boundary = boundary();
            boundary.span.end = source.find('}').unwrap() + 1;
            let loader = MapLoader::from([ModuleSource::new(MODULE, source)]);
            let result = compile(
                MODULE,
                &loader,
                CompilerOptions {
                    runtime_policy: RuntimePolicy::StrictRuntime,
                    strict_runtime_boundaries: vec![boundary],
                    ..CompilerOptions::default()
                },
            );
            assert!(result.has_errors(), "unexpected admission for {source}");
            assert!(result.output.is_none(), "unexpected output for {source}");
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.code == DiagnosticCode::InvalidContract }),
                "missing strict profile refusal for {source}: {:?}",
                result.diagnostics
            );
        }

        let loader = MapLoader::from([ModuleSource::new(MODULE, SOURCE)]);
        let result = compile(
            MODULE,
            &loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![boundary()],
                ambient_declaration_modules: vec![ModuleSource::new(
                    "src/ambient.d.ts",
                    "declare function globalText(): string;",
                )],
                ..CompilerOptions::default()
            },
        );
        assert!(result.output.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("cannot use ambient declarations")
        }));
    }

    #[test]
    fn permits_erased_type_imports_but_refuses_runtime_imports_and_uncovered_modules() {
        let type_source =
            "import type { Shape } from './types.d.ts';\nexport function echo(value: string): string { return value; }";
        let mut selected = boundary();
        selected.span = SourceSpan::new(
            MODULE,
            type_source.find("export function").unwrap(),
            type_source.rfind('}').unwrap() + 1,
        );
        let type_loader = MapLoader::from([
            ModuleSource::new(MODULE, type_source),
            ModuleSource::new("src/types.d.ts", "export interface Shape { name: string }"),
        ]);
        let allowed = compile(
            MODULE,
            &type_loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![selected.clone()],
                ..CompilerOptions::default()
            },
        );
        assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);

        let runtime_source =
            "import { other } from './other.ts';\nexport function echo(value: string): string { return value; }";
        let runtime_loader = MapLoader::from([
            ModuleSource::new(MODULE, runtime_source),
            ModuleSource::new("src/other.ts", "export const other: string = 'outside';"),
        ]);
        selected.span = SourceSpan::new(
            MODULE,
            runtime_source.find("export function").unwrap(),
            runtime_source.rfind('}').unwrap() + 1,
        );
        let refused = compile(
            MODULE,
            &runtime_loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![selected],
                ..CompilerOptions::default()
            },
        );
        assert!(refused.output.is_none());
        assert!(refused.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("unsupported runtime declaration or import")
        }));
        assert!(refused
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("no owner-selected boundary") }));
    }

    #[test]
    fn reserves_the_generated_alias_and_requires_root_relative_module_ids() {
        let source = format!("// {HELPER_ALIAS}\n{SOURCE}");
        let loader = MapLoader::from([ModuleSource::new(MODULE, &source)]);
        let mut selected = boundary();
        selected.span = SourceSpan::new(
            MODULE,
            source.find("export function").unwrap(),
            source.rfind('}').unwrap() + 1,
        );
        let refused = compile(
            MODULE,
            &loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![selected],
                ..CompilerOptions::default()
            },
        );
        assert!(refused.output.is_none());
        assert!(refused
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("reserved helper alias") }));

        let foreign_id = "memory:///main.ts";
        let loader = MapLoader::from([ModuleSource::new(foreign_id, SOURCE)]);
        let mut selected = boundary();
        selected.span.module = foreign_id.to_string();
        let refused = compile(
            foreign_id,
            &loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                strict_runtime_boundaries: vec![selected],
                ..CompilerOptions::default()
            },
        );
        assert!(refused.output.is_none());
        assert!(refused
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("canonical root-relative") }));
    }
}
