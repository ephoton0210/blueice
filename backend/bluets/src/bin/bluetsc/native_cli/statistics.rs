// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Report actual BlueTS work; memory counts retained authorized source bytes.
use super::*;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Statistics {
    modules: BTreeSet<String>,
    symbols: BTreeSet<String>,
    types: BTreeSet<String>,
    lines: usize,
    identifiers: usize,
    bytes: usize,
    instantiations: usize,
    read: Duration,
    parse: Duration,
    bind: Duration,
    check: Duration,
    emit: Duration,
    write: Duration,
}
impl Statistics {
    pub(super) fn add(&mut self, result: &blueice_bluets::Compilation) {
        let stats = &result.performance;
        self.read += stats.load_time;
        self.parse += stats.parse_time;
        self.bind += stats.bind_time;
        self.check += stats.check_time;
        self.emit += stats.emit_time;
        self.instantiations += stats.instantiations;
        for (id, module) in &result.project.modules {
            if self.modules.insert(id.clone()) {
                self.bytes += module.source.len();
                self.lines +=
                    module.source.lines().count() + usize::from(module.source.ends_with('\n'));
                if let Ok(tokens) = blueice_bluets::lex(id, &module.source) {
                    self.identifiers += tokens
                        .iter()
                        .filter(|token| token.kind == blueice_bluets::TokenKind::Identifier)
                        .count();
                }
            }
        }
        if let Some(checked) = &result.checked {
            for (id, module) in &checked.modules {
                for symbol in &module.symbols {
                    self.symbols
                        .insert(format!("{id}:{}:{}", symbol.span.start, symbol.name));
                    if let Some(value) = &symbol.value_type {
                        self.types.insert(format!("{value:?}"));
                    }
                }
            }
        }
    }
    pub(super) fn extra_emit(&mut self, duration: Duration) {
        self.emit += duration;
    }
    pub(super) fn written(&mut self, duration: Duration) {
        self.write += duration;
    }
    pub(super) fn display(&self, total: Duration) -> String {
        let mut text = String::new();
        for (name, value) in [
            ("Files", self.modules.len()),
            ("Lines", self.lines),
            ("Identifiers", self.identifiers),
            ("Symbols", self.symbols.len()),
            ("Types", self.types.len()),
            ("Instantiations", self.instantiations),
        ] {
            text.push_str(&format!("{:<15}{:>7}\n", format!("{name}:"), value));
        }
        text.push_str(&format!(
            "{:<15}{:>6}K\n",
            "Memory used:",
            self.bytes.div_ceil(1024)
        ));
        for (name, duration) in [
            ("I/O read", self.read),
            ("I/O write", self.write),
            ("Parse time", self.parse),
            ("Bind time", self.bind),
            ("Check time", self.check),
            ("Emit time", self.emit),
            ("Total time", total),
        ] {
            text.push_str(&format!(
                "{:<15}{:>6.2}s\n",
                format!("{name}:"),
                duration.as_secs_f64()
            ));
        }
        text
    }
}
