// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-lines test adapter compatible with BlueJS's supervisor transport.
//!
//! BlueTS deliberately reports compile results only.  It does not evaluate
//! JavaScript, install a Test262 harness, or depend on BlueJS; runtime tests
//! belong to the future BlueTS-to-BlueJS bridge.

use blueice_bluets::{
    compile, CompilerLimits, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleSource,
    ParserLimits, RuntimePolicy, LANGUAGE_VERSION,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
struct Request {
    source: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default)]
    module_path: Option<String>,
    #[serde(default)]
    module_sources: HashMap<String, String>,
    #[serde(default)]
    parse_only: bool,
    #[serde(default)]
    source_map: bool,
    #[serde(default)]
    declaration: bool,
    #[serde(default)]
    runtime_policy: Option<String>,
    #[serde(default)]
    limits: RequestLimits,
}

#[derive(Default, Deserialize)]
struct RequestLimits {
    max_modules: Option<usize>,
    max_module_edges: Option<usize>,
    max_module_depth: Option<usize>,
    max_total_source_bytes: Option<usize>,
    max_source_bytes: Option<usize>,
    max_tokens: Option<usize>,
    max_type_depth: Option<usize>,
    max_type_expansions: Option<usize>,
    max_source_map_segments: Option<usize>,
}

fn default_mode() -> String {
    "raw".to_string()
}

fn evaluate(request: Request) -> Value {
    if !matches!(
        request.mode.as_str(),
        "raw" | "sloppy" | "strict" | "module"
    ) {
        return json!({
            "kind": "harness_error",
            "message": format!("unsupported test-interface mode `{}`", request.mode),
        });
    }
    let runtime_policy = match request.runtime_policy.as_deref() {
        None | Some("checked") => RuntimePolicy::Checked,
        Some("transpile-only") => RuntimePolicy::TranspileOnly,
        Some("strict-runtime") => RuntimePolicy::StrictRuntime,
        Some(value) => {
            return json!({
                "kind": "harness_error",
                "message": format!("unsupported runtime policy `{value}`"),
            });
        }
    };
    let entry = canonical_module_id(request.module_path.as_deref().unwrap_or("entry.ts"));
    let mut sources = request
        .module_sources
        .into_iter()
        .map(|(path, source)| ModuleSource::new(canonical_module_id(&path), source))
        .collect::<Vec<_>>();
    sources.retain(|source| source.id != entry);
    sources.push(ModuleSource::new(entry.clone(), request.source));

    let options = CompilerOptions {
        runtime_policy: if request.parse_only {
            RuntimePolicy::TranspileOnly
        } else {
            runtime_policy
        },
        source_map: request.source_map,
        declaration: request.declaration,
        limits: compiler_limits(request.limits),
        ..CompilerOptions::default()
    };
    let compilation = compile(&entry, &MapLoader::from(sources), options);
    if let Some(diagnostic) = compilation.diagnostics.first() {
        return diagnostic_reply(diagnostic);
    }
    let output = compilation
        .output
        .expect("successful BlueTS test-interface compilation has output");
    let artifacts = output
        .artifacts
        .iter()
        .map(|(module, artifact)| {
            json!({
                "module": module,
                "source_map": artifact.source_map.is_some(),
                "declaration": artifact.declaration.is_some(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "kind": "ok",
        "phase": if request.parse_only { "parse" } else { "compile" },
        "language_version": LANGUAGE_VERSION,
        "fingerprint": output.fingerprint,
        "artifacts": artifacts,
    })
}

fn compiler_limits(request: RequestLimits) -> CompilerLimits {
    let mut limits = CompilerLimits::default();
    if let Some(value) = request.max_modules {
        limits.max_modules = value;
    }
    if let Some(value) = request.max_module_edges {
        limits.max_module_edges = value;
    }
    if let Some(value) = request.max_module_depth {
        limits.max_module_depth = value;
    }
    if let Some(value) = request.max_total_source_bytes {
        limits.max_total_source_bytes = value;
    }
    limits.parser = ParserLimits {
        max_source_bytes: request
            .max_source_bytes
            .unwrap_or(limits.parser.max_source_bytes),
        max_tokens: request.max_tokens.unwrap_or(limits.parser.max_tokens),
        max_type_depth: request
            .max_type_depth
            .unwrap_or(limits.parser.max_type_depth),
    };
    if let Some(value) = request.max_type_expansions {
        limits.max_type_expansions = value;
    }
    if let Some(value) = request.max_source_map_segments {
        limits.max_source_map_segments = value;
    }
    limits
}

fn diagnostic_reply(diagnostic: &Diagnostic) -> Value {
    let (phase, kind) = match diagnostic.code {
        DiagnosticCode::ParseError => ("parse", "SyntaxError"),
        DiagnosticCode::UnsupportedSyntax => ("parse", "unsupported"),
        DiagnosticCode::ModuleNotFound | DiagnosticCode::CircularModuleDependency => {
            ("resolution", "SyntaxError")
        }
        DiagnosticCode::ResourceLimit => ("compile", "resource_error"),
        DiagnosticCode::InvalidDeclarationFile
        | DiagnosticCode::DuplicateDeclaration
        | DiagnosticCode::UnknownName
        | DiagnosticCode::UnknownType
        | DiagnosticCode::TypeMismatch
        | DiagnosticCode::ReturnTypeMismatch => ("type", "TypeError"),
        DiagnosticCode::InvalidContract => ("compile", "TypeError"),
    };
    json!({
        "kind": kind,
        "phase": phase,
        "code": diagnostic.code.to_string(),
        "message": diagnostic.message,
        "span": {
            "module": diagnostic.span.module,
            "start_byte": diagnostic.span.start,
            "end_byte": diagnostic.span.end,
        },
    })
}

fn canonical_module_id(path: &str) -> String {
    if path.contains("://") {
        return path.to_string();
    }
    let path = path.trim_start_matches("./").trim_start_matches('/');
    format!("memory:///{path}")
}

fn main() -> io::Result<()> {
    let mut output = io::stdout().lock();
    writeln!(output, "{{\"ready\":1}}")?;
    output.flush()?;
    for line in io::stdin().lock().lines() {
        let reply = match serde_json::from_str::<Request>(&line?) {
            Ok(request) => evaluate(request),
            Err(error) => json!({"kind": "harness_error", "message": error.to_string()}),
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}
