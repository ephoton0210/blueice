// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generator functions and `yield` (J.3.7.7.4).
//!
//! The iteration protocol types a generator is annotated with (`Generator<T, R,
//! N>`, `Iterator`, `IterableIterator`, `IteratorResult`) are parsed from
//! declarations of their own, unless a local or host declaration defines the
//! name. Inside a generator the checker knows what may be yielded (`T`), what
//! may be returned (`R`) and what `yield` evaluates to (`N`), reads each
//! `yield` operand against `T`, and refuses a `yield` anywhere else.

use super::*;

/// What a generator body may yield, return and receive.
#[derive(Debug, Clone)]
pub(in crate::checker::module) struct GeneratorContext {
    /// `None` for an unannotated generator, whose yields are not checked.
    pub(in crate::checker::module) yield_type: Option<Type>,
    pub(in crate::checker::module) return_type: Option<Type>,
    /// The type of a `yield` expression: what `next(value)` passes in.
    pub(in crate::checker::module) next_type: Type,
}

const ITERATION_TYPES: &str = "\
interface IteratorYieldResult<T> { done?: false; value: T }
interface IteratorReturnResult<R> { done: true; value: R }
type IteratorResult<T, R = any> = IteratorYieldResult<T> | IteratorReturnResult<R>;
interface Iterator<T, R = any, N = any> {
    next(value?: N): IteratorResult<T, R>;
    return(value?: R): IteratorResult<T, R>;
    throw(error?: any): IteratorResult<T, R>;
}
interface Generator<T = unknown, R = any, N = any> {
    next(value?: N): IteratorResult<T, R>;
    return(value: R): IteratorResult<T, R>;
    throw(error: any): IteratorResult<T, R>;
}
interface IterableIterator<T, R = any, N = any> {
    next(value?: N): IteratorResult<T, R>;
    return(value?: R): IteratorResult<T, R>;
    throw(error?: any): IteratorResult<T, R>;
}
interface Iterable<T> { }
";

impl ModuleChecker<'_> {
    /// The iteration protocol types, unless a declaration already defines them.
    pub(super) fn bind_builtin_iteration_types(&mut self) {
        let Ok(module) = crate::parser::parse_module("<builtin>", ITERATION_TYPES) else {
            return;
        };
        for declaration in &module.declarations {
            match declaration {
                Declaration::Interface(interface) if !self.types.contains_key(&interface.name) => {
                    self.types.insert(
                        interface.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Interface,
                            parameters: interface.type_parameters.clone(),
                            value: interface_value(interface),
                        },
                    );
                }
                Declaration::TypeAlias(alias) if !self.types.contains_key(&alias.name) => {
                    self.types.insert(
                        alias.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Alias,
                            parameters: alias.type_parameters.clone(),
                            value: alias.value.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
    }

    /// The context of a generator function: read from its return annotation,
    /// which must be a generator-like type, or open when there is none.
    pub(in crate::checker::module) fn generator_context_for(
        &mut self,
        function: &FunctionDeclaration,
    ) -> GeneratorContext {
        let open = GeneratorContext {
            yield_type: None,
            return_type: None,
            next_type: Type::Any,
        };
        let Some(annotation) = &function.return_type else {
            return open;
        };
        let Type::Named { name, arguments } = annotation else {
            self.refuse_generator_annotation(annotation, &function.span);
            return open;
        };
        match name.as_str() {
            "Generator" | "Iterator" | "IterableIterator" | "Iterable" => {
                let argument =
                    |index: usize, default: Type| arguments.get(index).cloned().unwrap_or(default);
                GeneratorContext {
                    yield_type: Some(argument(0, Type::Unknown)),
                    return_type: if name == "Iterable" {
                        None
                    } else {
                        Some(argument(1, Type::Any))
                    },
                    next_type: argument(2, Type::Any),
                }
            }
            _ => {
                self.refuse_generator_annotation(annotation, &function.span);
                open
            }
        }
    }

    fn refuse_generator_annotation(&mut self, annotation: &Type, span: &SourceSpan) {
        self.type_error(
            span,
            format!(
                "the return type of a generator must be `Generator`, `Iterator`, \
                 `IterableIterator` or `Iterable`, not `{}`",
                type_label(annotation)
            ),
            DiagnosticCode::TypeMismatch,
        );
    }

    /// Every `yield` in `tokens` outside a nested function is in a generator,
    /// yields something assignable to the generator's yield type, and is at the
    /// start of an expression or of an assignment's right-hand side.
    pub(in crate::checker::module) fn check_yield_expressions(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if !tokens.iter().any(|token| token.is("yield")) {
            return;
        }
        let module = self.module;
        let range_start = tokens.first().map_or(0, |token| token.start);
        let inside_nested = |offset: usize| {
            module
                .nested_functions
                .range(range_start..=offset)
                .next_back()
                .is_some_and(|(_, nested)| offset < nested.span.end)
        };
        let positions: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.is("yield") && !inside_nested(token.start))
            .map(|(position, _)| position)
            .collect();
        for position in positions {
            self.check_one_yield(tokens, position, scope, span);
        }
    }

    fn check_one_yield(
        &mut self,
        tokens: &[Token],
        position: usize,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let at = SourceSpan::new(&span.module, tokens[position].start, tokens[position].end);
        let Some(context) = self.generator_context.clone() else {
            self.type_error(
                &at,
                "`yield` is only valid inside a generator function".to_string(),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        // `yield` extends as far right as the expression does, so it must start
        // an expression: the beginning of the tokens, a statement, or the right
        // side of an assignment, a `return` or a declaration.
        let starts_expression = position == 0
            || matches!(
                tokens[position - 1].text.as_str(),
                ";" | "{" | "}" | ")" | "=" | "return" | "else"
            );
        if !starts_expression {
            self.type_error(
                &at,
                "a `yield` inside a larger expression is not supported yet".to_string(),
                DiagnosticCode::UnsupportedSyntax,
            );
            return;
        }
        let delegate = tokens.get(position + 1).is_some_and(|token| token.is("*"));
        let start = position + 1 + usize::from(delegate);
        // The operand ends at the statement's `;` or the closing `}` of its block.
        let mut depth = 0usize;
        let mut end = tokens.len();
        for (index, token) in tokens.iter().enumerate().skip(start) {
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" => depth = depth.saturating_sub(1),
                "}" => {
                    if depth == 0 {
                        end = index;
                        break;
                    }
                    depth -= 1;
                }
                ";" if depth == 0 => {
                    end = index;
                    break;
                }
                _ => {}
            }
        }
        let operand = &tokens[start..end];
        if operand.is_empty() {
            if delegate {
                self.type_error(
                    &at,
                    "expected an operand after `yield*`".to_string(),
                    DiagnosticCode::ParseError,
                );
            }
            return;
        }
        let yielded = if delegate {
            let operand_type = self.infer_expression(operand, scope);
            match self.iterated_type(&operand_type) {
                Some(item) => item,
                None => {
                    if !matches!(operand_type, Type::Unknown | Type::Any) {
                        self.type_error(
                            &at,
                            format!("type `{}` is not iterable", type_label(&operand_type)),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                    return;
                }
            }
        } else {
            match &context.yield_type {
                Some(expected) => self.infer_in_context(operand, scope, expected),
                None => return,
            }
        };
        if let Some(expected) = &context.yield_type {
            if !self.is_assignable_bounded(&yielded, expected, &at) {
                self.type_error(
                    &at,
                    format!(
                        "yielded value of type `{}` is not assignable to the generator's yield type `{}`",
                        type_label(&yielded),
                        type_label(expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// The type of the values iterating `value` produces: an array's items, a
    /// tuple's elements, a string's characters, or a generator's yield type.
    pub(in crate::checker::module) fn iterated_type(&self, value: &Type) -> Option<Type> {
        let mut resolved = value.clone();
        if let Type::Named { name, arguments } = &resolved {
            if matches!(
                name.as_str(),
                "Generator" | "Iterator" | "IterableIterator" | "Iterable"
            ) {
                return Some(arguments.first().cloned().unwrap_or(Type::Unknown));
            }
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(expanded) = instantiate_named(
            &resolved,
            &self.types,
            &mut visited,
            &mut budget,
            "iteration",
        ) {
            resolved = expanded;
        }
        match resolved {
            Type::Array(item) => Some(*item),
            Type::Tuple(elements) => {
                let mut items: Vec<Type> = Vec::new();
                for element in elements {
                    let item = match (element.rest, element.annotation) {
                        (true, Type::Array(item)) => *item,
                        (_, annotation) => annotation,
                    };
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
                Some(match items.len() {
                    0 => Type::Never,
                    1 => items.remove(0),
                    _ => Type::Union(items),
                })
            }
            Type::String | Type::Literal(_) => Some(Type::String),
            _ => None,
        }
    }
}
