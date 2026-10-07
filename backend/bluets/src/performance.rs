// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Invocation-local measurements; nested and concurrent compiles remain isolated.
use crate::{CheckedProject, Project};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::Rc,
    time::{Duration, Instant},
};

/// BlueTS work and retained-source measurements, not TypeScript process costs.
#[derive(Debug, Clone, Default)]
pub struct CompilerPerformance {
    pub files: usize,
    pub lines: usize,
    pub identifiers: usize,
    pub symbols: usize,
    pub types: usize,
    pub instantiations: usize,
    /// Actual retained UTF-8 source bytes (not whole-process resident memory).
    pub retained_source_bytes: usize,
    pub load_time: Duration,
    pub parse_time: Duration,
    pub bind_time: Duration,
    pub check_time: Duration,
    pub emit_time: Duration,
    pub total_time: Duration,
}
#[derive(Default)]
struct Counters {
    load: Duration,
    bind: Duration,
    instantiations: usize,
}
thread_local! {static SESSIONS:RefCell<Vec<Rc<RefCell<Counters>>>>=const {RefCell::new(Vec::new())};}
pub(crate) struct Session {
    start: Instant,
    counters: Rc<RefCell<Counters>>,
}
impl Session {
    pub(crate) fn new() -> Self {
        let counters = Rc::new(RefCell::new(Counters::default()));
        SESSIONS.with_borrow_mut(|sessions| sessions.push(counters.clone()));
        Self {
            start: Instant::now(),
            counters,
        }
    }
    pub(crate) fn finish(
        &self,
        project: &Project,
        checked: Option<&CheckedProject>,
        graph: Duration,
        check: Duration,
        emit: Duration,
    ) -> CompilerPerformance {
        let counters = self.counters.borrow();
        let mut stats = CompilerPerformance {
            files: project.modules.len(),
            load_time: counters.load,
            parse_time: graph.saturating_sub(counters.load),
            bind_time: counters.bind,
            check_time: check.saturating_sub(counters.bind),
            emit_time: emit,
            total_time: self.start.elapsed(),
            instantiations: counters.instantiations,
            ..CompilerPerformance::default()
        };
        let mut types = BTreeSet::new();
        for module in project.modules.values() {
            stats.retained_source_bytes += module.source.len();
            stats.lines +=
                module.source.lines().count() + usize::from(module.source.ends_with('\n'));
            if let Ok(tokens) = crate::syntax::lex(&module.id, &module.source) {
                stats.identifiers += tokens
                    .iter()
                    .filter(|token| token.kind == crate::syntax::TokenKind::Identifier)
                    .count();
            }
        }
        if let Some(checked) = checked {
            for module in checked.modules.values() {
                stats.symbols += module.symbols.len();
                for symbol in &module.symbols {
                    if let Some(value) = &symbol.value_type {
                        types.insert(crate::diagnostic::type_text::render(value));
                    }
                }
            }
        }
        stats.types = types.len();
        stats
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        SESSIONS.with_borrow_mut(|sessions| {
            sessions.pop();
        });
    }
}
pub(crate) enum Stage {
    Load,
    Bind,
}
pub(crate) struct Timer {
    start: Instant,
    stage: Stage,
    counters: Option<Rc<RefCell<Counters>>>,
}
pub(crate) fn timer(stage: Stage) -> Timer {
    Timer {
        start: Instant::now(),
        stage,
        counters: SESSIONS.with_borrow(|sessions| sessions.last().cloned()),
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(counters) = &self.counters {
            let mut counters = counters.borrow_mut();
            match self.stage {
                Stage::Load => counters.load += self.start.elapsed(),
                Stage::Bind => counters.bind += self.start.elapsed(),
            }
        }
    }
}
pub(crate) fn instantiated() {
    SESSIONS.with_borrow(|sessions| {
        if let Some(counters) = sessions.last() {
            counters.borrow_mut().instantiations += 1;
        }
    });
}
