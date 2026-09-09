// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS: BlueIce's own JavaScript engine
//! (`development/browser_core/phase-13-bluejs-engine/PLAN.md`).
//!
//! This crate currently holds only the front end -- tokenizer and
//! parser producing an AST -- per that plan's checklist item of the
//! same name. The object model, bytecode compiler/interpreter, GC, and
//! event loop are separate, not-yet-implemented checklist items; see
//! the plan doc for the full picture. Scoped throughout to exactly
//! `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope (decided)" section, not
//! the full ECMAScript grammar.

mod ast;
mod parser;
mod token;

pub use ast::*;
pub use parser::{parse, ParseError};
