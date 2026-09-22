// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST-derived script capabilities for the gatekeeper and MCP.
//!
//! This is intentionally an auditable *syntactic* summary, not a claim to
//! prove runtime behaviour. A dynamically-computed callee is recorded as an
//! `UnknownDynamicCall`; the Phase 7 gatekeeper can therefore fail closed or
//! request review instead of receiving a falsely precise capability answer.

use crate::ast::*;
use crate::parser::{ParseError, parse};

/// The exact operation families BlueJS exposes to safety policy. `DomRead`
/// and `DomWrite` are separate because the latter changes the rendered page;
/// network/storage/dynamic code are separately named even though Phase 2
/// defers those host APIs, so their use can be blocked before an API is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptCapability {
    DomRead,
    DomWrite,
    EventRegistration,
    Timer,
    Network,
    Storage,
    DynamicCode,
    UnknownDynamicCall,
}

impl ScriptCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DomRead => "dom_read",
            Self::DomWrite => "dom_write",
            Self::EventRegistration => "event_registration",
            Self::Timer => "timer",
            Self::Network => "network",
            Self::Storage => "storage",
            Self::DynamicCode => "dynamic_code",
            Self::UnknownDynamicCall => "unknown_dynamic_call",
        }
    }
}

/// One concrete AST call site. `callee` is canonical dotted text for a static
/// member chain; dynamic access uses `"<dynamic>"` rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityUse {
    pub capability: ScriptCapability,
    pub callee: String,
}

/// Stable machine-readable summary: callers can inspect the raw AST-derived
/// sites, while `capabilities` gives a deduplicated approval surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySummary {
    pub capabilities: Vec<ScriptCapability>,
    pub uses: Vec<CapabilityUse>,
}

/// Parses then summarizes a script without executing it.
pub fn analyze(source: &str) -> Result<CapabilitySummary, ParseError> {
    Ok(analyze_program(&parse(source)?))
}

pub fn analyze_program(program: &Program) -> CapabilitySummary {
    let mut uses = Vec::new();
    for statement in &program.body {
        visit_statement(statement, &mut uses);
    }
    let mut capabilities = uses.iter().map(|use_| use_.capability).collect::<Vec<_>>();
    capabilities.sort();
    capabilities.dedup();
    CapabilitySummary { capabilities, uses }
}

/// The execution-time gatekeeper contract. Core/BlueJS integration invokes
/// `before_host_call` immediately before a host binding crosses script IPC;
/// preflight uses [`CapabilitySummary`], while this hook covers data-dependent
/// call paths that static analysis cannot prove.
pub trait ScriptGatekeeperHook {
    type Error;

    fn before_host_call(
        &mut self,
        capability: ScriptCapability,
        callee: &str,
    ) -> Result<(), Self::Error>;
}

fn visit_statement(statement: &Stmt, uses: &mut Vec<CapabilityUse>) {
    match statement {
        Stmt::Empty | Stmt::Break | Stmt::Continue => {}
        Stmt::Expr(expression) | Stmt::Throw(expression) => visit_expression(expression, uses),
        Stmt::Block(statements) => statements
            .iter()
            .for_each(|statement| visit_statement(statement, uses)),
        Stmt::VarDecl(_, declarations) => declarations
            .iter()
            .filter_map(|declaration| declaration.init.as_ref())
            .for_each(|expression| visit_expression(expression, uses)),
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            visit_expression(test, uses);
            visit_statement(consequent, uses);
            if let Some(alternate) = alternate {
                visit_statement(alternate, uses);
            }
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init) = init {
                match init {
                    ForInit::VarDecl(_, declarations) => declarations
                        .iter()
                        .filter_map(|declaration| declaration.init.as_ref())
                        .for_each(|expression| visit_expression(expression, uses)),
                    ForInit::Expr(expression) => visit_expression(expression, uses),
                }
            }
            if let Some(test) = test {
                visit_expression(test, uses);
            }
            if let Some(update) = update {
                visit_expression(update, uses);
            }
            visit_statement(body, uses);
        }
        Stmt::ForIn { right, body, .. } | Stmt::ForOf { right, body, .. } => {
            visit_expression(right, uses);
            visit_statement(body, uses);
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            visit_expression(test, uses);
            visit_statement(body, uses);
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expression(discriminant, uses);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expression(test, uses);
                }
                case.consequent
                    .iter()
                    .for_each(|statement| visit_statement(statement, uses));
            }
        }
        Stmt::Return(expression) => {
            if let Some(expression) = expression {
                visit_expression(expression, uses);
            }
        }
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            block
                .iter()
                .for_each(|statement| visit_statement(statement, uses));
            if let Some(handler) = handler {
                handler
                    .body
                    .iter()
                    .for_each(|statement| visit_statement(statement, uses));
            }
            if let Some(finalizer) = finalizer {
                finalizer
                    .iter()
                    .for_each(|statement| visit_statement(statement, uses));
            }
        }
        Stmt::FunctionDecl(function) => function
            .body
            .iter()
            .for_each(|statement| visit_statement(statement, uses)),
    }
}

fn visit_expression(expression: &Expr, uses: &mut Vec<CapabilityUse>) {
    match expression {
        Expr::Call { callee, args } | Expr::New { callee, args } => {
            record_call(callee, uses);
            visit_expression(callee, uses);
            for argument in args {
                match argument {
                    Argument::Normal(expression) | Argument::Spread(expression) => {
                        visit_expression(expression, uses)
                    }
                }
            }
        }
        Expr::Array(elements) => elements.iter().flatten().for_each(|element| match element {
            ArrayElement::Normal(expression) | ArrayElement::Spread(expression) => {
                visit_expression(expression, uses)
            }
        }),
        Expr::Object(properties) => properties.iter().for_each(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                visit_property_key(key, uses);
                visit_expression(value, uses);
            }
            ObjectProp::Spread(expression) => visit_expression(expression, uses),
        }),
        Expr::Function(function) => function
            .body
            .iter()
            .for_each(|statement| visit_statement(statement, uses)),
        Expr::Arrow { body, .. } => match body {
            ArrowBody::Expr(expression) => visit_expression(expression, uses),
            ArrowBody::Block(statements) => statements
                .iter()
                .for_each(|statement| visit_statement(statement, uses)),
        },
        Expr::Unary { arg, .. } | Expr::Update { arg, .. } => visit_expression(arg, uses),
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Assign {
            target: left,
            value: right,
            ..
        } => {
            visit_expression(left, uses);
            visit_expression(right, uses);
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            visit_expression(test, uses);
            visit_expression(consequent, uses);
            visit_expression(alternate, uses);
        }
        Expr::Member {
            object, property, ..
        } => {
            visit_expression(object, uses);
            visit_expression(property, uses);
        }
        Expr::Template { expressions, .. } => expressions
            .iter()
            .for_each(|expression| visit_expression(expression, uses)),
        Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::Identifier(_) => {}
    }
}

fn visit_property_key(key: &PropertyKey, uses: &mut Vec<CapabilityUse>) {
    if let PropertyKey::Computed(expression) = key {
        visit_expression(expression, uses);
    }
}

fn record_call(callee: &Expr, uses: &mut Vec<CapabilityUse>) {
    let Some(callee) = static_callee(callee) else {
        uses.push(CapabilityUse {
            capability: ScriptCapability::UnknownDynamicCall,
            callee: "<dynamic>".to_string(),
        });
        return;
    };
    let capability = match callee.as_str() {
        "document.getElementById"
        | "document.querySelector"
        | "document.querySelectorAll"
        | "node.getAttribute"
        | "node.classList.contains" => Some(ScriptCapability::DomRead),
        "document.createElement"
        | "document.createTextNode"
        | "node.appendChild"
        | "node.removeChild"
        | "node.insertBefore"
        | "node.remove"
        | "node.setAttribute"
        | "node.removeAttribute"
        | "node.classList.add"
        | "node.classList.remove"
        | "node.classList.toggle" => Some(ScriptCapability::DomWrite),
        "node.addEventListener" | "node.removeEventListener" => {
            Some(ScriptCapability::EventRegistration)
        }
        "window.setTimeout" | "window.clearTimeout" | "setTimeout" | "clearTimeout" => {
            Some(ScriptCapability::Timer)
        }
        "fetch" | "XMLHttpRequest" | "WebSocket" => Some(ScriptCapability::Network),
        "localStorage.getItem"
        | "localStorage.setItem"
        | "sessionStorage.getItem"
        | "sessionStorage.setItem" => Some(ScriptCapability::Storage),
        "eval" | "Function" => Some(ScriptCapability::DynamicCode),
        _ => None,
    };
    if let Some(capability) = capability {
        uses.push(CapabilityUse { capability, callee });
    }
}

fn static_callee(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Identifier(name) => Some(name.clone()),
        Expr::Member {
            object,
            property,
            computed: false,
        } => {
            let object = static_callee(object)?;
            let Expr::Identifier(property) = property.as_ref() else {
                return None;
            };
            Some(format!("{object}.{property}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_distinguishes_dom_read_write_timer_and_dynamic_calls() {
        let summary = analyze("let node = document.getElementById('target'); node.setAttribute('hidden', ''); setTimeout(() => node.remove(), 1); target[method]();").unwrap();

        assert_eq!(
            summary.capabilities,
            vec![
                ScriptCapability::DomRead,
                ScriptCapability::DomWrite,
                ScriptCapability::Timer,
                ScriptCapability::UnknownDynamicCall
            ]
        );
        assert!(
            summary
                .uses
                .iter()
                .any(|use_| use_.callee == "document.getElementById")
        );
        assert!(summary.uses.iter().any(|use_| use_.callee == "<dynamic>"));
    }

    #[test]
    fn unrecognised_static_calls_do_not_claim_a_capability() {
        let summary = analyze("console.log('safe');").unwrap();

        assert!(summary.capabilities.is_empty());
        assert!(summary.uses.is_empty());
    }
}
