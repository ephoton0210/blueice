// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static TypeScript debugger metadata independent of a JavaScript VM.

use crate::checker::{type_label, CheckedProject, SymbolKind, Type};
use crate::compiler::CompilerOptions;
use crate::contracts::ContractPlan;
use crate::diagnostic::SourceSpan;
use crate::parser::{Declaration, InterfaceDeclaration, Module};
use crate::LANGUAGE_VERSION;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId(pub u32);

/// An opaque source identity minted with one static debug-info record. The
/// corresponding [`DebugSource`] deliberately retains a hash rather than
/// source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub u32);

/// An opaque, compiler-minted identifier for one reifiable static contract.
/// It is valid only for the exact `BlueTsDebugInfo` generation that produced
/// it; hosts must not substitute a later compilation's plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractId(pub u32);

/// A source identity contains a content hash but not source text.  A debugger
/// host decides separately whether a requesting principal can read the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSource {
    pub id: SourceId,
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
    /// The source record that owns this source span. It is a source-free
    /// provenance handle, not a source-read capability.
    pub source: SourceId,
    /// A reifiable local declaration plan, when the declaration has one.
    /// Generic, imported, erased, and otherwise non-reifiable declarations
    /// intentionally have no contract handle.
    pub contract: Option<ContractId>,
}

/// A pure, source-text-free runtime contract retained as static debug
/// metadata. The plan contains no callbacks, JavaScript values, host objects,
/// source text, resolver authority, or output capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugContract {
    pub id: ContractId,
    pub source: SourceId,
    pub name: String,
    pub span: SourceSpan,
    pub plan: ContractPlan,
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
    pub contracts: Vec<DebugContract>,
}

pub(crate) fn build(checked: &CheckedProject, options: &CompilerOptions) -> BlueTsDebugInfo {
    let mut sources = Vec::new();
    let mut source_ids = BTreeMap::new();
    for module_id in checked.modules.keys() {
        let id = SourceId(u32::try_from(sources.len()).expect("BlueTS source IDs must fit u32"));
        source_ids.insert(module_id.clone(), id);
        let checked_module = &checked.modules[module_id];
        sources.push(DebugSource {
            id,
            module: module_id.clone(),
            content_hash: source_hash(&checked_module.module.source),
        });
    }

    let mut interned_types = BTreeMap::<String, TypeId>::new();
    let mut types = Vec::new();
    let mut symbols = Vec::new();
    let mut contracts = Vec::new();
    let mut contracts_by_declaration = BTreeMap::new();

    for (module_id, checked_module) in &checked.modules {
        let source = source_ids[module_id];
        let local_contracts =
            local_contracts(module_id, source, &checked_module.module, &mut contracts);
        contracts_by_declaration.extend(
            local_contracts
                .into_iter()
                .map(|(name, id)| ((module_id.clone(), name), id)),
        );
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
                source,
                contract: matches!(symbol.kind, SymbolKind::TypeAlias | SymbolKind::Interface)
                    .then(|| {
                        contracts_by_declaration.get(&(module_id.clone(), symbol.name.clone()))
                    })
                    .flatten()
                    .copied(),
            });
        }
    }
    BlueTsDebugInfo {
        language_version: LANGUAGE_VERSION.to_string(),
        compiler_options_hash: options_hash(options),
        sources,
        types,
        symbols,
        contracts,
    }
}

/// Retains only declaration-local, non-generic plans whose complete named
/// definition table is present in this exact module. This is intentionally
/// narrower than BlueTS's full checker: imported names, generic instantiations
/// and erased types are not reconstructed from display strings or guessed
/// across module boundaries.
fn local_contracts(
    module_id: &str,
    source: SourceId,
    module: &Module,
    contracts: &mut Vec<DebugContract>,
) -> BTreeMap<String, ContractId> {
    let definitions = local_contract_definitions(module);
    let mut retained = BTreeMap::new();
    for name in definitions.keys() {
        let root = Type::Named {
            name: name.clone(),
            arguments: Vec::new(),
        };
        let Ok(plan) = ContractPlan::from_type(format!("{module_id}#{name}"), &root, &definitions)
        else {
            continue;
        };
        let Some(span) = local_contract_span(module, name) else {
            continue;
        };
        let id =
            ContractId(u32::try_from(contracts.len()).expect("BlueTS contract IDs must fit u32"));
        contracts.push(DebugContract {
            id,
            source,
            name: name.clone(),
            span: span.clone(),
            plan,
        });
        retained.insert(name.clone(), id);
    }
    retained
}

fn local_contract_definitions(module: &Module) -> BTreeMap<String, Type> {
    module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::TypeAlias(alias) if alias.type_parameters.is_empty() => {
                Some((alias.name.clone(), alias.value.clone()))
            }
            Declaration::Interface(interface) if interface.type_parameters.is_empty() => {
                Some((interface.name.clone(), interface_contract_type(interface)))
            }
            _ => None,
        })
        .collect()
}

fn interface_contract_type(interface: &InterfaceDeclaration) -> Type {
    let record = Type::Record(interface.fields.clone());
    if interface.heritage.is_empty() {
        record
    } else {
        let mut parts = interface.heritage.clone();
        parts.push(record);
        Type::Intersection(parts)
    }
}

fn local_contract_span<'a>(module: &'a Module, name: &str) -> Option<&'a SourceSpan> {
    module
        .declarations
        .iter()
        .find_map(|declaration| match declaration {
            Declaration::TypeAlias(alias)
                if alias.name == name && alias.type_parameters.is_empty() =>
            {
                Some(&alias.span)
            }
            Declaration::Interface(interface)
                if interface.name == name && interface.type_parameters.is_empty() =>
            {
                Some(&interface.span)
            }
            _ => None,
        })
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
    use super::SourceId;
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

    #[test]
    fn retains_only_reifiable_local_contracts_with_source_provenance_handles() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///contracts.ts",
            "interface Settings { enabled: boolean; name?: string; } \
             type Pair = [number, string]; \
             type Generic<T> = { value: T }; \
             export const settings: Settings = { enabled: true };",
        )]);
        let compilation = compile(
            "memory:///contracts.ts",
            &loader,
            CompilerOptions::default(),
        );
        let debug = compilation
            .debug_info
            .expect("successful compile has debug metadata");
        assert_eq!(debug.sources.len(), 1);
        assert_eq!(debug.sources[0].id, SourceId(0));
        assert_eq!(debug.contracts.len(), 2);
        assert!(debug
            .contracts
            .iter()
            .any(|contract| contract.name == "Settings"));
        assert!(debug
            .contracts
            .iter()
            .any(|contract| contract.name == "Pair"));
        assert!(!debug
            .contracts
            .iter()
            .any(|contract| contract.name == "Generic"));
        let settings = debug
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Settings")
            .expect("interface must retain a debugger symbol");
        assert_eq!(settings.source, SourceId(0));
        assert!(settings.contract.is_some());
        assert_ne!(debug.sources[0].content_hash, "Settings");
    }
}
