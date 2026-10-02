// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct execution of classic-mode JSX (J.5.2).
//!
//! A `.tsx` element lowers to a call of the module's own factory, built as BlueJS
//! AST from the same tree and the same rules as the emitted JavaScript (the text
//! rules and entity decoding are shared, the call shape is TypeScript's classic
//! transform). The factory is whatever name the program (or `jsxFactory`/`@jsx`)
//! gives, resolved like any other identifier in the program: JSX syntax imports
//! nothing and grants no host API. The automatic runtime needs an imported
//! runtime module and `preserve` keeps JSX, so neither runs directly.

use std::cell::RefCell;

use blueice_bluets::jsx::{self, JsxAttribute, JsxChild, JsxElement, JsxValue, Pragmas};
use blueice_bluets::{CompilerOptions, JsxMode, Module};

use super::*;

/// What the direct bridge needs of the JSX options for one module.
#[derive(Debug, Clone)]
pub(super) struct JsxContext {
    mode: Option<JsxMode>,
    factory: String,
    fragment: String,
    automatic: bool,
}

impl JsxContext {
    pub(super) fn of(options: &CompilerOptions, module: &Module) -> Self {
        let pragmas = Pragmas::of(&module.source);
        let automatic = match pragmas.runtime.as_deref() {
            Some("classic") => false,
            Some("automatic") => true,
            _ => {
                matches!(options.jsx, Some(JsxMode::ReactJsx | JsxMode::ReactJsxDev))
                    || pragmas.import_source.is_some()
            }
        };
        Self {
            mode: options.jsx,
            factory: pragmas
                .factory
                .or_else(|| options.jsx_factory.clone())
                .unwrap_or_else(|| "React.createElement".to_string()),
            fragment: pragmas
                .fragment
                .or_else(|| options.jsx_fragment_factory.clone())
                .unwrap_or_else(|| "React.Fragment".to_string()),
            automatic,
        }
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<JsxContext>> = const { RefCell::new(None) };
}

/// Makes `context` the JSX settings of the module being lowered on this thread.
pub(super) struct JsxScope(Option<JsxContext>);

impl JsxScope {
    pub(super) fn enter(context: JsxContext) -> Self {
        Self(CONTEXT.with(|slot| slot.replace(Some(context))))
    }
}

impl Drop for JsxScope {
    fn drop(&mut self) {
        CONTEXT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

fn current() -> Option<JsxContext> {
    CONTEXT.with(|slot| slot.borrow().clone())
}

fn string_expression(text: &str) -> bluejs::Expr {
    bluejs::Expr::String(text.to_string().into())
}

/// TypeScript writes an attribute name bare when it matches `[A-Z_]\w*`.
fn property_key(name: &str) -> bluejs::PropertyKey {
    let mut characters = name.chars();
    let bare = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if bare {
        bluejs::PropertyKey::Identifier(name.to_string())
    } else {
        bluejs::PropertyKey::String(name.to_string().into())
    }
}

impl ExpressionLowerer<'_> {
    /// At a `JsxElement` token: lowers it and the argument group that follows it.
    pub(super) fn lower_jsx_element(&mut self, token: &Token) -> Result<bluejs::Expr, BridgeError> {
        let span = self.token_span(token);
        let Some(context) = current() else {
            return Err(unsupported(span, "JSX needs the `jsx` option"));
        };
        match context.mode {
            None => return Err(unsupported(span, "cannot use JSX unless the `jsx` option is provided")),
            Some(JsxMode::Preserve | JsxMode::ReactNative) => {
                return Err(unsupported(
                    span,
                    "preserved JSX is not executable; choose `jsx: react` for direct execution",
                ))
            }
            Some(_) if context.automatic => {
                return Err(unsupported(
                    span,
                    "the automatic JSX runtime imports a runtime module, which the direct bridge does not link; use the classic mode with an in-program factory",
                ))
            }
            Some(_) => {}
        }
        // The embedded expressions follow as `( e1 , e2 , ... )`, the commas being
        // zero-width.
        let group = self.take_group(token)?;
        let element = blueice_bluets::parse_jsx(self.module, &token.text, 0)
            .map_err(|error| unsupported(span.clone(), error.message))?;
        let mut expressions = group.into_iter();
        self.lower_element(&element, &token.text, &context, &mut expressions)
    }

    fn take_group(&mut self, token: &Token) -> Result<Vec<Vec<Token>>, BridgeError> {
        let open = self.tokens.get(self.index);
        if !open.is_some_and(|open| open.text == "(" && open.start == open.end) {
            return Err(unsupported(
                self.token_span(token),
                "a JSX element is missing its expression group",
            ));
        }
        self.index += 1;
        let mut depth = 0usize;
        let mut groups: Vec<Vec<Token>> = vec![Vec::new()];
        loop {
            let Some(next) = self.tokens.get(self.index).cloned() else {
                return Err(unsupported(
                    self.token_span(token),
                    "a JSX element's expression group is unterminated",
                ));
            };
            self.index += 1;
            match next.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" if depth > 0 => depth -= 1,
                ")" if next.start == next.end => break,
                "," if depth == 0 && next.start == next.end => {
                    groups.push(Vec::new());
                    continue;
                }
                _ => {}
            }
            groups.last_mut().expect("a group exists").push(next);
        }
        if groups.len() == 1 && groups[0].is_empty() {
            groups.clear();
        }
        Ok(groups)
    }

    fn lower_expression_tokens(&self, tokens: &[Token]) -> Result<bluejs::Expr, BridgeError> {
        ExpressionLowerer::new(self.module, tokens).parse()
    }

    fn entity_expression(&self, entity: &str) -> Result<bluejs::Expr, BridgeError> {
        let mut tokens = blueice_bluets::lex(self.module, entity).map_err(|_| {
            unsupported(
                SourceSpan::new(self.module, 0, 0),
                "invalid JSX factory name",
            )
        })?;
        tokens.pop();
        self.lower_expression_tokens(&tokens)
    }

    fn lower_element(
        &self,
        element: &JsxElement,
        source: &str,
        context: &JsxContext,
        expressions: &mut std::vec::IntoIter<Vec<Token>>,
    ) -> Result<bluejs::Expr, BridgeError> {
        let callee = self.entity_expression(&context.factory)?;
        let tag = match &element.name {
            None => self.entity_expression(&context.fragment)?,
            Some(name) if jsx::is_intrinsic_name(&name.text) => string_expression(&name.text),
            Some(name) => self.entity_expression(&name.text)?,
        };
        let mut args = vec![bluejs::Argument::Normal(tag)];
        if element.attributes.is_empty() {
            args.push(bluejs::Argument::Normal(bluejs::Expr::Null));
        } else {
            let mut properties = Vec::new();
            for attribute in &element.attributes {
                match attribute {
                    JsxAttribute::Spread { .. } => {
                        let tokens = self.next_group(expressions)?;
                        properties.push(bluejs::ObjectProp::Spread(
                            self.lower_expression_tokens(&tokens)?,
                        ));
                    }
                    JsxAttribute::Named { name, value, .. } => {
                        let lowered = match value {
                            None => bluejs::Expr::Bool(true),
                            Some(JsxValue::String { start, end }) => {
                                let raw = &source[*start..*end];
                                string_expression(&jsx::decode_entities(&raw[1..raw.len() - 1]))
                            }
                            Some(JsxValue::Expression { .. }) => {
                                let tokens = self.next_group(expressions)?;
                                self.lower_expression_tokens(&tokens)?
                            }
                            Some(JsxValue::Element(inner)) => {
                                self.lower_element(inner, source, context, expressions)?
                            }
                        };
                        properties.push(bluejs::ObjectProp::KeyValue {
                            key: property_key(&name.text),
                            value: lowered,
                            shorthand: false,
                        });
                    }
                }
            }
            args.push(bluejs::Argument::Normal(bluejs::Expr::Object(properties)));
        }
        for child in &element.children {
            match child {
                JsxChild::Text { start, end } => {
                    if let Some(text) = jsx::text_value(&source[*start..*end]) {
                        args.push(bluejs::Argument::Normal(string_expression(&text)));
                    }
                }
                JsxChild::Expression { tokens, spread, .. } => {
                    if tokens.is_empty() && !*spread {
                        continue;
                    }
                    let group = self.next_group(expressions)?;
                    let lowered = self.lower_expression_tokens(&group)?;
                    args.push(if *spread {
                        bluejs::Argument::Spread(lowered)
                    } else {
                        bluejs::Argument::Normal(lowered)
                    });
                }
                JsxChild::Element(inner) => args.push(bluejs::Argument::Normal(
                    self.lower_element(inner, source, context, expressions)?,
                )),
            }
        }
        Ok(bluejs::Expr::Call {
            callee: Box::new(callee),
            args,
        })
    }

    fn next_group(
        &self,
        expressions: &mut std::vec::IntoIter<Vec<Token>>,
    ) -> Result<Vec<Token>, BridgeError> {
        expressions.next().ok_or_else(|| {
            unsupported(
                SourceSpan::new(self.module, 0, 0),
                "a JSX element has fewer embedded expressions than its syntax",
            )
        })
    }
}
