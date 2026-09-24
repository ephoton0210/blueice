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
use ring::digest::{digest, SHA256};
use std::collections::BTreeMap;
use std::fmt::Write;

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

/// A source identity contains a SHA-256 content digest but not source text.
/// The digest makes the compiler/MCP provenance record collision-resistant;
/// it is still metadata rather than source-read authority. A debugger host
/// separately decides whether a requesting principal may see any source
/// identity or digest at all.
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

/// Zero-based original-source position. Columns count UTF-16 code units, as
/// in BlueTS's emitted source maps; no source text is retained here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugSourcePosition {
    pub line: usize,
    pub column_utf16: usize,
}

/// Half-open original-source position range for one declaration span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugSourceLocation {
    pub start: DebugSourcePosition,
    pub end: DebugSourcePosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSymbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    /// The checker's exact module-export classification for this declaration.
    pub exported: bool,
    pub span: SourceSpan,
    pub location: DebugSourceLocation,
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
    pub location: DebugSourceLocation,
    pub plan: ContractPlan,
}

/// Resolves only compiler-produced declaration boundaries in one linear scan
/// of the source. The temporary index is discarded after static metadata is
/// built; the retained records contain coordinates, never source bytes.
struct SourcePositionIndex {
    positions: BTreeMap<usize, DebugSourcePosition>,
}

impl SourcePositionIndex {
    fn new<'a>(source: &str, spans: impl IntoIterator<Item = &'a SourceSpan>) -> Self {
        let mut requested = BTreeMap::<usize, Option<DebugSourcePosition>>::new();
        for span in spans {
            requested.insert(span.start, None);
            requested.insert(span.end, None);
        }
        let mut position = DebugSourcePosition {
            line: 0,
            column_utf16: 0,
        };
        let mut previous_was_cr = false;
        for (offset, character) in source.char_indices() {
            if let Some(slot) = requested.get_mut(&offset) {
                *slot = Some(position);
            }
            match character {
                '\r' => {
                    position.line += 1;
                    position.column_utf16 = 0;
                    previous_was_cr = true;
                }
                '\n' => {
                    if !previous_was_cr {
                        position.line += 1;
                    }
                    position.column_utf16 = 0;
                    previous_was_cr = false;
                }
                _ => {
                    position.column_utf16 += character.len_utf16();
                    previous_was_cr = false;
                }
            }
        }
        if let Some(slot) = requested.get_mut(&source.len()) {
            *slot = Some(position);
        }
        Self {
            positions: requested
                .into_iter()
                .map(|(offset, position)| {
                    (
                        offset,
                        position.expect("checked declaration spans must end at UTF-8 boundaries"),
                    )
                })
                .collect(),
        }
    }

    fn location(&self, span: &SourceSpan) -> DebugSourceLocation {
        DebugSourceLocation {
            start: self.positions[&span.start],
            end: self.positions[&span.end],
        }
    }
}

/// Resolves authorized UTF-8 byte spans to zero-based original-source UTF-16
/// coordinates in one scan. Invalid ranges and non-character boundaries have
/// no location; this helper never returns source text or guesses a position.
pub fn source_locations_for_spans<'a>(
    source: &str,
    spans: impl IntoIterator<Item = &'a SourceSpan>,
) -> Vec<Option<DebugSourceLocation>> {
    let spans = spans.into_iter().collect::<Vec<_>>();
    let valid = spans
        .iter()
        .map(|span| {
            span.start <= span.end
                && span.end <= source.len()
                && source.is_char_boundary(span.start)
                && source.is_char_boundary(span.end)
        })
        .collect::<Vec<_>>();
    let positions = SourcePositionIndex::new(
        source,
        spans
            .iter()
            .zip(&valid)
            .filter_map(|(span, valid)| valid.then_some(*span)),
    );
    spans
        .iter()
        .zip(valid)
        .map(|(span, valid)| valid.then(|| positions.location(span)))
        .collect()
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
        let positions = SourcePositionIndex::new(
            &checked_module.module.source,
            checked_module
                .symbols
                .iter()
                .map(|symbol| &symbol.span)
                .chain(
                    checked_module
                        .module
                        .declarations
                        .iter()
                        .filter_map(|declaration| match declaration {
                            Declaration::TypeAlias(alias) => Some(&alias.span),
                            Declaration::Interface(interface) => Some(&interface.span),
                            _ => None,
                        }),
                ),
        );
        let local_contracts = local_contracts(
            module_id,
            source,
            &checked_module.module,
            &positions,
            &mut contracts,
        );
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
                exported: symbol.exported,
                span: symbol.span.clone(),
                location: positions.location(&symbol.span),
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
    positions: &SourcePositionIndex,
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
            location: positions.location(span),
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
    let digest = digest(&SHA256, value.as_bytes());
    let mut hex = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        write!(&mut hex, "{byte:02x}").expect("writing a digest to String cannot fail");
    }
    format!("bts-sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::{source_hash, source_locations_for_spans, SourceId};
    use crate::{compile, CompilerOptions, MapLoader, ModuleSource, SourceSpan};

    #[test]
    fn authorized_spans_map_crlf_and_non_bmp_columns_without_guessing_invalid_offsets() {
        let source = "a\r\n😀b";
        let emoji = SourceSpan::new("memory:///app.ts", 3, 7);
        let eof = SourceSpan::new("memory:///app.ts", 8, 8);
        let middle_of_emoji = SourceSpan::new("memory:///app.ts", 4, 7);
        let beyond_source = SourceSpan::new("memory:///app.ts", 8, 9);
        let reversed = SourceSpan::new("memory:///app.ts", 7, 3);
        let locations = source_locations_for_spans(
            source,
            [&emoji, &eof, &middle_of_emoji, &beyond_source, &reversed],
        );
        let emoji_location = locations[0].unwrap();
        assert_eq!(
            (emoji_location.start.line, emoji_location.start.column_utf16),
            (1, 0)
        );
        assert_eq!(
            (emoji_location.end.line, emoji_location.end.column_utf16),
            (1, 2)
        );
        let eof_location = locations[1].unwrap();
        assert_eq!(
            (eof_location.start.line, eof_location.start.column_utf16),
            (1, 3)
        );
        assert_eq!(eof_location.start, eof_location.end);
        assert_eq!(&locations[2..], &[None, None, None]);
    }

    #[test]
    fn retains_static_type_and_source_hash_without_retaining_source_text() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "export const count: number = 1; const local: number = 2;",
        )]);
        let compilation = compile("memory:///app.ts", &loader, CompilerOptions::default());
        let debug = compilation.debug_info.unwrap();
        assert_eq!(debug.symbols.len(), 2);
        assert!(
            debug
                .symbols
                .iter()
                .find(|symbol| symbol.name == "count")
                .unwrap()
                .exported
        );
        assert!(
            !debug
                .symbols
                .iter()
                .find(|symbol| symbol.name == "local")
                .unwrap()
                .exported
        );
        assert_eq!(debug.types[0].display, "number");
        assert_ne!(
            debug.sources[0].content_hash,
            "export const count: number = 1;"
        );
    }

    #[test]
    fn source_provenance_uses_a_labeled_cryptographic_digest() {
        assert_eq!(
            source_hash("abc"),
            "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn declaration_locations_retain_original_utf16_columns_without_source_text() {
        let source = "const 值: number = 1; /* 🚀 */ const after: number = 2;\r\ninterface Settings { enabled: boolean; }";
        let loader = MapLoader::from([ModuleSource::new("memory:///app.ts", source)]);
        let compilation = compile("memory:///app.ts", &loader, CompilerOptions::default());
        let debug = compilation.debug_info.unwrap();
        let first = debug
            .symbols
            .iter()
            .find(|symbol| symbol.name == "值")
            .expect("first-line declaration must retain a symbol");
        assert_eq!(first.location.start.line, 0);
        assert_eq!(first.location.start.column_utf16, 0);
        let after = debug
            .symbols
            .iter()
            .find(|symbol| symbol.name == "after")
            .expect("same-line declaration must retain a symbol");
        assert_eq!(after.location.start.line, 0);
        assert_eq!(
            after.location.start.column_utf16,
            "const 值: number = 1; /* 🚀 */ ".encode_utf16().count()
        );
        let interface = debug
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Settings")
            .expect("second-line declaration must retain a symbol");
        assert_eq!(interface.location.start.line, 1);
        assert_eq!(interface.location.start.column_utf16, 0);
        let contract = debug
            .contracts
            .iter()
            .find(|contract| contract.name == "Settings")
            .expect("the interface must retain a reifiable contract");
        assert_eq!(contract.location.start, interface.location.start);
        assert_eq!(contract.location.end, interface.location.end);
        assert!(interface.location.end.column_utf16 > interface.location.start.column_utf16);
        assert_ne!(debug.sources[0].content_hash, source);
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
