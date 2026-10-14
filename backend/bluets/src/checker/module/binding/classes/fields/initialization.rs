// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Definite initialization along constructor branches and normal returns.

use super::*;
use crate::parser::{FunctionElseBranch, FunctionIfStatement};

#[derive(Clone)]
struct Flow {
    continuing: Option<bool>,
    incomplete_return: bool,
}

pub(super) fn analyze(body: &[FunctionBodyItem], name: &str) -> (bool, Vec<Token>) {
    let mut flow = Flow {
        continuing: Some(false),
        incomplete_return: false,
    };
    let mut reads = Vec::new();
    statements(body, name, &mut flow, &mut reads);
    (
        !flow.incomplete_return && flow.continuing.unwrap_or(true),
        reads,
    )
}

fn statements(body: &[FunctionBodyItem], name: &str, flow: &mut Flow, reads: &mut Vec<Token>) {
    for item in body {
        let Some(assigned) = flow.continuing else {
            break;
        };
        match item {
            FunctionBodyItem::Expression { tokens, .. } => {
                read(tokens, name, assigned, reads);
                if super::item_assigns_this_field(item, name) {
                    flow.continuing = Some(true);
                }
            }
            FunctionBodyItem::Variable(variable) => {
                read(&variable.initializer, name, assigned, reads)
            }
            FunctionBodyItem::Return { tokens, .. } => {
                read(tokens, name, assigned, reads);
                flow.incomplete_return |= !assigned;
                flow.continuing = None;
            }
            FunctionBodyItem::Throw { tokens, .. } => {
                read(tokens, name, assigned, reads);
                flow.continuing = None;
            }
            FunctionBodyItem::If(statement) => branch(statement, name, flow, reads),
            FunctionBodyItem::While(statement) => {
                read(&statement.test, name, assigned, reads);
                let mut iteration = flow.clone();
                statements(&statement.body, name, &mut iteration, reads);
                flow.incomplete_return |= iteration.incomplete_return;
            }
            FunctionBodyItem::Try(statement) => {
                let mut block = flow.clone();
                statements(&statement.block, name, &mut block, reads);
                let mut handler = flow.clone();
                if let Some(catch) = &statement.handler {
                    statements(&catch.body, name, &mut handler, reads);
                }
                // A catch starts from the incoming state: any expression in
                // the try may throw before its first assignment.
                if statement.handler.is_some() {
                    block = join(block, handler);
                }
                if let Some(finalizer) = &statement.finalizer {
                    let stopped = block.continuing.is_none();
                    // A finalizer can return on a throwing path. Analyze it
                    // from the incoming state when that path has no normal exit.
                    if stopped {
                        block.continuing = Some(assigned);
                    }
                    statements(finalizer, name, &mut block, reads);
                    if stopped {
                        block.continuing = None;
                    }
                }
                *flow = block;
            }
            FunctionBodyItem::Function(_) | FunctionBodyItem::Opaque(_) => {}
        }
    }
}

fn branch(statement: &FunctionIfStatement, name: &str, flow: &mut Flow, reads: &mut Vec<Token>) {
    read(
        &statement.test,
        name,
        flow.continuing.unwrap_or(true),
        reads,
    );
    let mut consequent = flow.clone();
    statements(&statement.consequent, name, &mut consequent, reads);
    let mut alternate = flow.clone();
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(body)) => statements(body, name, &mut alternate, reads),
        Some(FunctionElseBranch::ElseIf(statement)) => {
            branch(statement, name, &mut alternate, reads)
        }
        None => {}
    }
    *flow = join(consequent, alternate);
}

fn join(left: Flow, right: Flow) -> Flow {
    Flow {
        continuing: match (left.continuing, right.continuing) {
            (Some(left), Some(right)) => Some(left && right),
            (left, right) => left.or(right),
        },
        incomplete_return: left.incomplete_return || right.incomplete_return,
    }
}

fn read(tokens: &[Token], name: &str, assigned: bool, reads: &mut Vec<Token>) {
    if assigned
        || tokens
            .iter()
            .any(|token| token.is("=>") || token.is("function"))
    {
        return;
    }
    for (index, triple) in tokens.windows(3).enumerate() {
        if triple[0].is("this")
            && triple[1].is(".")
            && triple[2].text == name
            && !tokens.get(index + 3).is_some_and(|token| token.is("="))
        {
            reads.push(triple[2].clone());
        }
    }
}
