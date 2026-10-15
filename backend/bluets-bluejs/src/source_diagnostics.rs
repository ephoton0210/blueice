// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source-only evidence from checked BlueTS nodes and the final direct AST.
//! Addresses exist only during this immutable traversal; retained evidence has
//! ordinals and coordinates, with no original source text or debugger handles.

use super::*;
use bluejs::diagnostics::{self, DiagnosticSourceMap, DiagnosticSourceNode};
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Clone)]
pub(crate) struct DirectDiagnosticOrigins {
    source: bluejs::BlueJsSourceIdentity,
    positions: Vec<Option<(u32, u32)>>,
    ast_fingerprint: [u8; 32],
}

fn invalid() -> BridgeError {
    BridgeError::ProvenanceAttachment("direct diagnostic source evidence is inconsistent".into())
}

impl DirectDiagnosticOrigins {
    pub(crate) fn build(
        module: &Module,
        program: &bluejs::BlueJsProgramV1,
        provenance: &[LoweringProvenance],
        sources: &[BridgeSource],
        define_fields: bool,
    ) -> Result<Self, BridgeError> {
        let source = source_identity(sources)?;
        if source.canonical_module_id() != module.id {
            return Err(invalid());
        }
        let count = diagnostics::visit_diagnostic_source_nodes(program, &mut |_, _| {});
        // Bound the temporary node/offset/position vectors before allocation.
        let vector_bytes = std::mem::size_of::<(u8, usize, usize)>()
            + std::mem::size_of::<Option<usize>>()
            + std::mem::size_of::<usize>()
            + 2 * std::mem::size_of::<Option<(u32, u32)>>();
        if count
            .checked_mul(vector_bytes)
            .is_none_or(|bytes| bytes > diagnostics::MAX_DIAGNOSTIC_SOURCE_MAP_BYTES)
        {
            return Err(BridgeError::ProvenanceAttachment(
                "direct diagnostic inventory exceeds its byte budget".into(),
            ));
        }
        let body = match program {
            bluejs::BlueJsProgramV1::Script(program) => &program.body,
            bluejs::BlueJsProgramV1::Module(module) => &module.body,
        };
        if body.len() != provenance.len() {
            return Err(invalid());
        }
        let declarations = module
            .declarations
            .iter()
            .map(|declaration| {
                let span = declaration.span();
                ((span.start, span.end), declaration)
            })
            .collect::<BTreeMap<_, _>>();
        let mut erased = module
            .erased_assertion_spans()
            .chain(module.erased_call_type_argument_spans())
            .map(|span| (span.start, span.end))
            .collect::<Vec<_>>();
        erased.sort_unstable();
        let mut intervals = Vec::<(usize, usize)>::new();
        for (start, end) in erased {
            if let Some(previous) = intervals.last_mut().filter(|previous| start <= previous.1) {
                previous.1 = previous.1.max(end);
            } else {
                intervals.push((start, end));
            }
        }
        let mut evidence = Evidence {
            module,
            define_fields,
            nodes: Vec::with_capacity(count),
            erased: intervals,
            error: None,
        };
        for (statement, origin) in body.iter().zip(provenance) {
            if origin.source.module != module.id {
                return Err(invalid());
            }
            if let Some(original) = declarations.get(&(origin.source.start, origin.source.end)) {
                evidence.declaration(original, statement);
            }
        }
        if let Some(error) = evidence.error {
            return Err(error);
        }
        evidence
            .nodes
            .sort_unstable_by_key(|(kind, address, _)| (*kind, *address));
        if evidence
            .nodes
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 && pair[0].2 != pair[1].2)
        {
            return Err(invalid());
        }
        let mut offsets = vec![None; count];
        diagnostics::visit_diagnostic_source_nodes(program, &mut |node, ordinal| {
            let key = key(node);
            if let Ok(index) = evidence
                .nodes
                .binary_search_by_key(&key, |(kind, address, _)| (*kind, *address))
            {
                offsets[ordinal] = Some(evidence.nodes[index].2);
            }
        });
        let positions = diagnostics::source_positions(
            &module.source,
            &offsets
                .iter()
                .map(|offset| offset.unwrap_or(0))
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .zip(offsets)
        .map(|(position, offset)| offset.and(position))
        .collect();
        Ok(Self {
            source,
            positions,
            ast_fingerprint: fingerprint(program)?,
        })
    }

    pub(crate) fn map(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
        program: &bluejs::BlueJsProgramV1,
    ) -> Result<DiagnosticSourceMap, bluejs::BlueJsPageRuntimeError> {
        let compiled = registry
            .get(handle)
            .map_err(bluejs::BlueJsPageRuntimeError::ProgramRegistry)?;
        if compiled.source() != &self.source
            || fingerprint(program).ok() != Some(self.ast_fingerprint)
        {
            return Err(bluejs::BlueJsPageRuntimeError::DiagnosticSourceMap(
                diagnostics::DiagnosticSourceMapError::SourceMismatch,
            ));
        }
        DiagnosticSourceMap::from_node_positions(
            self.source.clone(),
            compiled.bytecode(),
            self.source.canonical_module_id(),
            &self.positions,
        )
        .map_err(bluejs::BlueJsPageRuntimeError::DiagnosticSourceMap)
    }
}

/// Stream a bounded AST certificate without retaining a second AST or source.
/// The certificate is private to this artifact, not a serialized ABI.
fn fingerprint(program: &bluejs::BlueJsProgramV1) -> Result<[u8; 32], BridgeError> {
    struct Output {
        hash: Sha256,
        bytes: usize,
    }
    impl Write for Output {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.bytes = self
                .bytes
                .checked_add(text.len())
                .filter(|count| *count <= diagnostics::MAX_DIAGNOSTIC_SOURCE_MAP_BYTES)
                .ok_or(std::fmt::Error)?;
            self.hash.update(text.as_bytes());
            Ok(())
        }
    }
    let mut output = Output {
        hash: Sha256::new(),
        bytes: 0,
    };
    write!(&mut output, "{program:?}").map_err(|_| {
        BridgeError::ProvenanceAttachment(
            "direct diagnostic AST fingerprint exceeds its byte budget".into(),
        )
    })?;
    // Debug deliberately displays all NaNs alike; include actual numeric bits.
    diagnostics::visit_diagnostic_source_nodes(program, &mut |node, _| {
        if let DiagnosticSourceNode::Expression(bluejs::Expr::Number(number)) = node {
            output.hash.update(number.to_bits().to_le_bytes());
        }
    });
    Ok(output.hash.finalize().into())
}

fn key(node: DiagnosticSourceNode<'_>) -> (u8, usize) {
    match node {
        DiagnosticSourceNode::Statement(node) => (0, node as *const bluejs::Stmt as usize),
        DiagnosticSourceNode::Expression(node) => (1, node as *const bluejs::Expr as usize),
        DiagnosticSourceNode::Pattern(node) => (2, node as *const bluejs::Pattern as usize),
        DiagnosticSourceNode::AssignmentPattern(node) => {
            (3, node as *const bluejs::AssignmentPattern as usize)
        }
    }
}

struct Evidence<'a> {
    module: &'a Module,
    define_fields: bool,
    nodes: Vec<(u8, usize, usize)>,
    erased: Vec<(usize, usize)>,
    error: Option<BridgeError>,
}

impl Evidence<'_> {
    fn bind(&mut self, node: DiagnosticSourceNode<'_>, span: &SourceSpan) {
        if span.module == self.module.id
            && span.start <= span.end
            && span.end <= self.module.source.len()
            && self.module.source.is_char_boundary(span.start)
            && self.module.source.is_char_boundary(span.end)
        {
            let (kind, address) = key(node);
            self.nodes.push((kind, address, span.start));
        }
    }

    fn declaration(&mut self, original: &Declaration, statement: &bluejs::Stmt) {
        match (original, statement) {
            (Declaration::Function(original), bluejs::Stmt::FunctionDecl(function)) => {
                self.bind(DiagnosticSourceNode::Statement(statement), &original.span);
                self.function(&original.parameters, &original.body, function);
            }
            (Declaration::Variable(original), bluejs::Stmt::VarDecl(_, declarations))
                if declarations.len() == 1 =>
            {
                self.bind(DiagnosticSourceNode::Statement(statement), &original.span);
                if let Some(value) = &declarations[0].init {
                    self.expression(value, &original.initializer);
                }
            }
            (Declaration::Raw(original), bluejs::Stmt::Expr(expression)) => {
                self.bind(DiagnosticSourceNode::Statement(statement), &original.span);
                self.expression(expression, &original.tokens);
            }
            (Declaration::Class(original), bluejs::Stmt::ClassDecl(class))
                if self.define_fields =>
            {
                self.bind(DiagnosticSourceNode::Statement(statement), &original.span);
                self.class(original, class);
            }
            // Generated namespace/enum scaffolding requires separate evidence;
            // an enclosing declaration is never a substitute for its nodes.
            _ => {}
        }
    }

    fn parameters(&mut self, parameters: &[Parameter], function: &bluejs::Function) {
        for (original, parameter) in parameters
            .iter()
            .filter(|parameter| parameter.name != "this")
            .zip(&function.params)
        {
            if let (Some(tokens), Some(value)) = (&original.default, &parameter.default) {
                self.expression(value, tokens);
            }
        }
    }
    fn function(
        &mut self,
        parameters: &[Parameter],
        items: &[FunctionBodyItem],
        function: &bluejs::Function,
    ) {
        self.parameters(parameters, function);
        self.body(items, &function.body);
    }

    fn body(&mut self, items: &[FunctionBodyItem], body: &[bluejs::Stmt]) {
        for (original, statement) in items.iter().zip(body) {
            self.statement(original, statement);
        }
    }

    fn statement(&mut self, original: &FunctionBodyItem, statement: &bluejs::Stmt) {
        use FunctionBodyItem as Item;
        let span = match (original, statement) {
            (Item::Variable(original), bluejs::Stmt::VarDecl(_, declarations))
                if declarations.len() == 1 =>
            {
                if let Some(value) = &declarations[0].init {
                    self.expression(value, &original.initializer);
                }
                &original.span
            }
            (Item::Expression { span, tokens }, bluejs::Stmt::Expr(expression))
            | (Item::Throw { span, tokens }, bluejs::Stmt::Throw(expression)) => {
                self.expression(expression, tokens);
                span
            }
            (Item::Return { span, tokens }, bluejs::Stmt::Return(expression)) => {
                if let Some(expression) = expression {
                    self.expression(expression, tokens);
                }
                span
            }
            (Item::Function(original), bluejs::Stmt::FunctionDecl(function)) => {
                self.function(&original.parameters, &original.body, function);
                &original.span
            }
            (Item::If(original), bluejs::Stmt::If { .. }) => {
                self.conditional(original, statement);
                &original.span
            }
            (Item::While(original), bluejs::Stmt::While { test, body }) => {
                self.expression(test, &original.test);
                if let bluejs::Stmt::Block(body) = body.as_ref() {
                    self.body(&original.body, body);
                }
                &original.span
            }
            (
                Item::Try(original),
                bluejs::Stmt::Try {
                    block,
                    handler,
                    finalizer,
                },
            ) => {
                self.body(&original.block, block);
                if let (Some(original), Some(handler)) = (&original.handler, handler) {
                    self.body(&original.body, &handler.body);
                }
                if let (Some(original), Some(finalizer)) = (&original.finalizer, finalizer) {
                    self.body(original, finalizer);
                }
                &original.span
            }
            _ => return,
        };
        self.bind(DiagnosticSourceNode::Statement(statement), span);
    }

    fn conditional(&mut self, original: &FunctionIfStatement, statement: &bluejs::Stmt) {
        let bluejs::Stmt::If {
            test,
            consequent,
            alternate,
        } = statement
        else {
            return;
        };
        self.expression(test, &original.test);
        if let bluejs::Stmt::Block(body) = consequent.as_ref() {
            self.body(&original.consequent, body);
        }
        match (&original.alternate, alternate.as_deref()) {
            (Some(FunctionElseBranch::Braced(items)), Some(bluejs::Stmt::Block(body))) => {
                self.body(items, body)
            }
            (
                Some(FunctionElseBranch::ElseIf(original)),
                Some(statement @ bluejs::Stmt::If { .. }),
            ) => {
                self.conditional(original, statement);
                self.bind(DiagnosticSourceNode::Statement(statement), &original.span);
            }
            _ => {}
        }
    }

    fn expression(&mut self, expression: &bluejs::Expr, tokens: &[Token]) {
        if self.error.is_some() {
            return;
        }
        match ExpressionLowerer::for_module(self.module, tokens).parse_with_origins() {
            Ok((replayed, origins)) if &replayed == expression => {
                let result = origins.bind(expression, &mut |node, offset| {
                    self.bind(
                        DiagnosticSourceNode::Expression(node),
                        &SourceSpan::new(&self.module.id, offset, offset),
                    );
                });
                self.error = result.err();
                return;
            }
            Err(error @ BridgeError::ProvenanceAttachment(_)) => {
                self.error = Some(error);
                return;
            }
            // Enum/namespace rewriting and generated expression helpers need
            // separate evidence. Never join a merely similar expression.
            _ => {}
        }
        let Some(first) = tokens.iter().find(|token| {
            let index = self
                .erased
                .partition_point(|(start, _)| *start <= token.start);
            index == 0 || self.erased[index - 1].1 < token.end
        }) else {
            return;
        };
        let Some(last) = tokens.last() else { return };
        if !self.prefix(expression, first) {
            self.bind(
                DiagnosticSourceNode::Expression(expression),
                &SourceSpan::new(&self.module.id, first.start, last.end),
            );
        }
    }

    /// Only reference/call prefixes proven to start at this original identifier
    /// share its start. Arguments, computed keys and conditional branches keep
    /// independent gaps until their own lowering evidence is available.
    fn prefix(&mut self, expression: &bluejs::Expr, first: &Token) -> bool {
        let certified = match expression {
            bluejs::Expr::Identifier(name) => name == &first.text,
            bluejs::Expr::Member { object, .. } => self.prefix(object, first),
            bluejs::Expr::Call { callee, .. } => self.prefix(callee, first),
            _ => false,
        };
        if certified {
            self.bind(
                DiagnosticSourceNode::Expression(expression),
                &first.span(&self.module.id),
            );
        }
        certified
    }

    fn class(&mut self, original: &ClassDeclaration, class: &bluejs::Class) {
        // The lowering emits synthetic parameter-property fields first.
        let mut index = original.parameter_property_fields().len();
        for member in &original.members {
            if member.index.is_some()
                || member.abstract_modifier.is_some()
                || member.field.as_ref().is_some_and(|field| field.declared)
            {
                continue;
            }
            let has_body = member
                .constructor
                .as_ref()
                .is_none_or(|value| value.body.is_some())
                && member
                    .method
                    .as_ref()
                    .is_none_or(|value| value.body.is_some());
            if !has_body {
                continue;
            }
            let Some(element) = class.elements.get(index) else {
                return;
            };
            index += 1;
            match element {
                bluejs::ClassElement::StaticBlock(body) => {
                    if let Some(block) = &member.static_block {
                        self.body(&block.body, body);
                    }
                }
                bluejs::ClassElement::Accessor { function, .. } => {
                    if let Some(accessor) = &member.accessor {
                        self.function(&accessor.parameters, &accessor.body, function);
                    }
                }
                bluejs::ClassElement::Method { function, .. } => {
                    if let Some(method) = &member.method {
                        if let Some(body) = &method.body {
                            self.function(&method.parameters, body, function);
                        }
                    } else if let Some(constructor) = &member.constructor {
                        if let Some(body) = &constructor.body {
                            self.parameters(&constructor.parameters, function);
                            let count = constructor.parameter_properties.len();
                            if count == 0 {
                                self.body(body, &function.body);
                            } else if let Some(insertion) = &constructor.prologue_insertion {
                                let at = insertion.item_index;
                                if at <= body.len() && function.body.len() == body.len() + count {
                                    self.body(&body[..at], &function.body[..at]);
                                    self.body(&body[at..], &function.body[at + count..]);
                                }
                            }
                        }
                    }
                }
                bluejs::ClassElement::Field {
                    initializer: Some(value),
                    ..
                } => {
                    if let Some(tokens) = member
                        .field
                        .as_ref()
                        .and_then(|field| field.initializer.as_deref())
                    {
                        self.expression(value, tokens);
                    }
                }
                _ => {}
            }
        }
    }
}
