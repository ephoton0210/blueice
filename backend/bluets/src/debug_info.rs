// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static TypeScript debugger metadata independent of a JavaScript VM.

use crate::checker::{type_label, CheckedProject, SymbolKind, Type};
use crate::compiler::CompilerOptions;
use crate::diagnostic::SourceSpan;
use crate::LANGUAGE_VERSION;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId(pub u32);

/// A source identity contains a content hash but not source text.  A debugger
/// host decides separately whether a requesting principal can read the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSource {
    pub module: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugType {
    pub id: TypeId,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSymbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub span: SourceSpan,
    pub static_type: Option<TypeId>,
}

/// The host-neutral part of the Phase 18 debugger product.  BlueJS bytecode
/// safe points and runtime-value correspondence are intentionally absent until
/// the public hand-off ABI exists; this object is nevertheless stable enough
/// for `bluetsc`, source navigation, diagnostic UI, and a future MCP adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueTsDebugInfo {
    pub language_version: String,
    pub compiler_options_hash: String,
    pub sources: Vec<DebugSource>,
    pub types: Vec<DebugType>,
    pub symbols: Vec<DebugSymbol>,
}

pub(crate) fn build(checked: &CheckedProject, options: &CompilerOptions) -> BlueTsDebugInfo {
    let mut sources = Vec::new();
    let mut interned_types = BTreeMap::<String, TypeId>::new();
    let mut types = Vec::new();
    let mut symbols = Vec::new();

    for (module_id, checked_module) in &checked.modules {
        sources.push(DebugSource {
            module: module_id.clone(),
            content_hash: source_hash(&checked_module.module.source),
        });
        for symbol in &checked_module.symbols {
            let static_type = symbol
                .value_type
                .as_ref()
                .map(|value| intern_type(value, &mut interned_types, &mut types));
            let id = SymbolId(symbols.len() as u32);
            symbols.push(DebugSymbol {
                id,
                name: symbol.name.clone(),
                kind: symbol.kind,
                span: symbol.span.clone(),
                static_type,
            });
        }
    }
    BlueTsDebugInfo {
        language_version: LANGUAGE_VERSION.to_string(),
        compiler_options_hash: options_hash(options),
        sources,
        types,
        symbols,
    }
}

fn intern_type(
    value: &Type,
    interned_types: &mut BTreeMap<String, TypeId>,
    types: &mut Vec<DebugType>,
) -> TypeId {
    let display = type_label(value);
    if let Some(id) = interned_types.get(&display) {
        return *id;
    }
    let id = TypeId(types.len() as u32);
    interned_types.insert(display.clone(), id);
    types.push(DebugType { id, display });
    id
}

fn options_hash(options: &CompilerOptions) -> String {
    let source_map = if options.source_map { "map" } else { "no-map" };
    let declaration = if options.declaration { "dts" } else { "no-dts" };
    hash(&format!(
        "{}|{}|{}|{source_map}|{declaration}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        options.target.as_str(),
        options.runtime_policy.as_str(),
        options.resolver_fingerprint,
        options.require_declared_global_calls,
        options.limits.max_modules,
        options.limits.max_module_edges,
        options.limits.max_module_depth,
        options.limits.max_total_source_bytes,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
        options.limits.parser.max_type_depth,
        options.limits.max_type_expansions,
        options.limits.max_source_map_segments,
    ))
}

fn source_hash(source: &str) -> String {
    hash(source)
}

fn hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("bts-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use crate::{compile, CompilerOptions, MapLoader, ModuleSource};

    #[test]
    fn retains_static_type_and_source_hash_without_retaining_source_text() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "export const count: number = 1;",
        )]);
        let compilation = compile("memory:///app.ts", &loader, CompilerOptions::default());
        let debug = compilation.debug_info.unwrap();
        assert_eq!(debug.symbols.len(), 1);
        assert_eq!(debug.types[0].display, "number");
        assert_ne!(
            debug.sources[0].content_hash,
            "export const count: number = 1;"
        );
    }

    #[test]
    fn debug_info_hash_binds_declared_global_call_policy() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "export const count: number = 1;",
        )]);
        let default = compile("memory:///app.ts", &loader, CompilerOptions::default())
            .debug_info
            .unwrap()
            .compiler_options_hash;
        let page_profile = compile(
            "memory:///app.ts",
            &loader,
            CompilerOptions {
                require_declared_global_calls: true,
                ..CompilerOptions::default()
            },
        )
        .debug_info
        .unwrap()
        .compiler_options_hash;

        assert_ne!(default, page_profile);
    }
}
