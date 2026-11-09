// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standard (TC39) decorators with target-aware class lowering (K.7.4).
//!
//! A decorated class becomes the shape TypeScript 5.9.3 emits: an immediately
//! invoked arrow function holding the helper state, whose class has two leading
//! `static` blocks that evaluate every decorator expression in source order and
//! apply them (static methods and accessors, instance ones, static fields, instance
//! fields, then the class itself), define `Symbol.metadata`, and whose fields run
//! the initializers the decorators returned. The helper text is written here from
//! the specified semantics (TC39 decorators, `ApplyDecoratorsToElementDefinition`
//! and `CreateDecoratorAccessObject`), carries a version, and is emitted once per
//! module ahead of the first class that needs it.
//!
//! Only the class syntax is rewritten: members keep their source text, a
//! decorator expression is moved into its array by a relocation (so edits inside
//! it, such as CommonJS name rewriting, follow it), and a field's initializer stays
//! where it is, wrapped in place. Auto-accessors (`accessor x`) are lowered to a
//! private backing field with a getter and a setter, decorated or not.
//!
//! Computed keys, namespace and class-expression scopes, private descriptors and
//! static super receivers are lowered before the existing target transforms.
//! ESNext define semantics preserve proposals. Literal/private callable keys and
//! receiver-aware super references retain their own evaluation scope. Decorators
//! on constructors/static blocks/overload signatures retain precise refusals.

use std::collections::BTreeSet;

pub(in crate::emitter) mod auto_accessors;
mod class;
pub(in crate::emitter) mod expressions;
pub(in crate::emitter) mod member_origins;
mod members;
use members::{Kind, Names, Plan};
mod private_members;
mod scopes;
pub(super) mod super_members;
pub(super) mod targets;
pub(super) use scopes::namespace_owner;

use super::{Module, TextEdit};
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassMemberKind, ClassMemberShell, Declaration, Decorator};
use crate::syntax::lex;
use crate::{Token, TokenKind};

/// The version of the emitted decorator helper text; part of the build fingerprint.
pub const DECORATOR_HELPER_V1_VERSION: &str = "bluets-decorator-helper-v2";

const RUN_INITIALIZERS: &str = "__bluetsRunInitializers";
const ES_DECORATE: &str = "__bluetsEsDecorate";

/// `__bluetsRunInitializers(thisArg, initializers[, value])`: runs each
/// initializer with `this` as `thisArg`, threading `value` through when given.
const RUN_INITIALIZERS_HELPER: &str = "var __bluetsRunInitializers = function (thisArg, initializers, value) { var useValue = arguments.length > 2; for (var i = 0; i < initializers.length; i++) { value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg); } return useValue ? value : void 0; };";

/// `__bluetsEsDecorate(ctor, descriptor, decorators, context, initializers,
/// extraInitializers)`: applies `decorators` last to first to one element.
const ES_DECORATE_HELPER: &str = "var __bluetsEsDecorate = function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) { function accept(f) { if (f !== void 0 && typeof f !== \"function\") throw new TypeError(\"Function expected\"); return f; } var kind = contextIn.kind, key = kind === \"getter\" ? \"get\" : kind === \"setter\" ? \"set\" : \"value\"; var target = !descriptorIn && ctor ? contextIn[\"static\"] ? ctor : ctor.prototype : null; var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {}); var _, done = false; for (var i = decorators.length - 1; i >= 0; i--) { var context = {}; for (var p in contextIn) context[p] = p === \"access\" ? {} : contextIn[p]; for (var p in contextIn.access) context.access[p] = contextIn.access[p]; context.addInitializer = function (f) { if (done) throw new TypeError(\"Cannot add initializers after decoration has completed\"); extraInitializers.push(accept(f || null)); }; var result = (0, decorators[i])(kind === \"accessor\" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context); if (kind === \"accessor\") { if (result === void 0) continue; if (result === null || typeof result !== \"object\") throw new TypeError(\"Object expected\"); if (_ = accept(result.get)) descriptor.get = _; if (_ = accept(result.set)) descriptor.set = _; if (_ = accept(result.init)) initializers.unshift(_); } else if (_ = accept(result)) { if (kind === \"field\") initializers.unshift(_); else descriptor[key] = _; } } if (target) Object.defineProperty(target, contextIn.name, descriptor); done = true; };";

pub(super) fn is_helper_insertion(edit: &TextEdit) -> bool {
    edit.start == edit.end
        && edit.replacement.starts_with(RUN_INITIALIZERS_HELPER)
        && edit.replacement.contains(ES_DECORATE_HELPER)
}

fn unsupported(span: &SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span.clone(), message)
}

/// A relocation marker: the text of source range `start..end`, with the edits
/// inside it applied, is written here instead (see `apply_edits`).
pub(super) fn relocated(start: usize, end: usize) -> String {
    format!("\u{0}M{start},{end}\u{0}")
}

pub(super) fn class_keyword_start(class: &ClassDeclaration, source: &str) -> usize {
    // The declaration span can begin at an export or decorator. The class
    // keyword is the last such token before the checked class name; lexing
    // the header avoids mistaking the word `class` in trivia for that token.
    source
        .get(class.span.start..class.name_span.start)
        .and_then(|header| lex("<class-header>", header).ok())
        .and_then(|tokens| tokens.into_iter().rfind(|token| token.is("class")))
        .map_or(class.span.start, |token| class.span.start + token.start)
}

/// Whether the emitter lowers this class's decorators or auto-accessors (so the
/// other passes leave its `export` and its name to this one).
pub(super) fn lowers_class(class: &ClassDeclaration, options: &CompilerOptions) -> bool {
    if options.experimental_decorators {
        return super::legacy_decorators::lowers_class(class);
    }
    needs_lowering(class) && !preserves_proposals(options)
}

fn preserves_proposals(options: &CompilerOptions) -> bool {
    options.target == EcmaTarget::EsNext && options.defines_class_fields()
}

fn needs_lowering(class: &ClassDeclaration) -> bool {
    !class.decorators.is_empty()
        || class.members.iter().any(|member| {
            !member.decorators.is_empty()
                || member.field.as_ref().is_some_and(|field| field.accessor)
        })
}

pub(super) fn has_standard_lowering(module: &Module) -> bool {
    super::runtime_declarations(&module.declarations).into_iter().any(
        |(declaration, _)| matches!(declaration, Declaration::Class(class) if needs_lowering(class)),
    ) || module.class_expressions.values().any(|expression| needs_lowering(&expression.class))
}

pub(super) fn lower_decorators(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    wrapper: crate::ModuleKind,
) -> Result<(), Diagnostic> {
    if preserves_proposals(options) {
        for (declaration, _) in super::runtime_declarations(&module.declarations) {
            let Declaration::Class(class) = declaration else {
                continue;
            };
            if class.exported || class.decorators.is_empty() {
                continue;
            }
            let keyword = class_keyword_start(class, &module.source);
            edits.push(TextEdit {
                start: keyword,
                end: keyword + "class".len(),
                replacement: format!(
                    "{}class{}",
                    super::relocations::unmapped(),
                    super::relocations::unmapped()
                ),
            });
        }
        for expression in module
            .class_expressions
            .values()
            .filter(|expression| !expression.statement && !expression.class.decorators.is_empty())
        {
            let keyword = class_keyword_start(&expression.class, &module.source);
            edits.push(TextEdit {
                start: keyword,
                end: keyword + "class".len(),
                replacement: format!(
                    "{}class{}",
                    super::relocations::unmapped(),
                    super::relocations::unmapped()
                ),
            });
        }
        return Ok(());
    }
    if !has_standard_lowering(module) {
        return Ok(());
    }
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return Ok(());
    };
    let mut names = Names {
        used: tokens
            .iter()
            .filter(|token| matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword))
            .flat_map(|token| {
                [
                    token.text.clone(),
                    token.text.trim_start_matches('#').to_string(),
                ]
            })
            .collect(),
    };
    let mut first_class_start: Option<usize> = None;
    for (declaration, _) in super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if !needs_lowering(class) {
            continue;
        }
        Lowerer {
            module,
            options,
            wrapper,
            names: &mut names,
            edits,
            run_initializers: RUN_INITIALIZERS.into(),
            es_decorate: ES_DECORATE.into(),
        }
        .class(class, false, None)?;
        let helper_start = scopes::helper_start(module, class);
        first_class_start = Some(first_class_start.map_or(helper_start, |at| at.min(helper_start)));
    }
    for expression in module.class_expressions.values() {
        if !needs_lowering(&expression.class) {
            continue;
        }
        let label = expression.name.as_ref().map_or_else(
            || {
                super::class_lowering::expressions::names::contextual_name(
                    module, expression, &tokens,
                )
            },
            |name| quoted(name),
        );
        let mut class = expression.class.clone();
        class.name = expression
            .name
            .clone()
            .unwrap_or_else(|| names.unique("_decoratedClass"));
        Lowerer {
            module,
            options,
            wrapper,
            names: &mut names,
            edits,
            run_initializers: RUN_INITIALIZERS.into(),
            es_decorate: ES_DECORATE.into(),
        }
        .class(&class, !expression.statement, Some(&label))?;
        let at = scopes::helper_start(module, &class);
        first_class_start = Some(first_class_start.map_or(at, |first| first.min(at)));
    }
    if let Some(at) = first_class_start {
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: format!("{RUN_INITIALIZERS_HELPER} {ES_DECORATE_HELPER} "),
        });
    }
    Ok(())
}

struct Lowerer<'a, 'e> {
    module: &'a Module,
    options: &'a CompilerOptions,
    wrapper: crate::ModuleKind,
    names: &'a mut Names,
    edits: &'e mut Vec<TextEdit>,
    run_initializers: String,
    es_decorate: String,
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text)
        .expect("property text is a string")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

impl Lowerer<'_, '_> {
    fn push(&mut self, start: usize, end: usize, replacement: String) {
        self.edits.push(TextEdit {
            start,
            end,
            replacement,
        });
    }

    fn newlines(&self, start: usize, end: usize) -> String {
        "\n".repeat(self.module.source[start..end].matches('\n').count())
    }

    /// A decorator's expression as it is called: a property access (`a.b`) is
    /// bound to its receiver, as TypeScript does, since the helper calls it
    /// without one.
    fn decorator_expression(&self, decorator: &Decorator) -> Result<String, Diagnostic> {
        let tokens = &decorator.tokens;
        let (start, end) = (
            tokens.first().expect("a decorator has tokens").start,
            tokens.last().expect("a decorator has tokens").end,
        );
        let inner: &[Token] = if tokens.first().is_some_and(|token| token.is("("))
            && tokens.last().is_some_and(|token| token.is(")"))
        {
            &tokens[1..tokens.len() - 1]
        } else {
            tokens
        };
        let is_chain = !inner.is_empty()
            && inner.len() % 2 == 1
            && inner.iter().enumerate().all(|(index, token)| {
                if index % 2 == 0 {
                    matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
                } else {
                    token.is(".")
                }
            });
        let dotted = inner.iter().any(|token| token.is("."))
            || inner.iter().any(|token| token.text.contains('.'));
        let single_dotted_token = inner.len() == 1 && inner[0].text.contains('.');
        if (is_chain && dotted) || single_dotted_token {
            // `a.b.c` -> `a.b.c.bind(a.b)`.
            let text = relocated(start, end);
            let receiver = if single_dotted_token {
                let full = &inner[0].text;
                full.rsplit_once('.').map(|(head, _)| head.to_string())
            } else {
                let last_dot = inner
                    .iter()
                    .rposition(|token| token.is("."))
                    .expect("dotted");
                Some(
                    inner[..last_dot]
                        .iter()
                        .map(|token| token.text.as_str())
                        .collect::<String>(),
                )
            };
            let receiver = receiver.expect("a dotted chain has a receiver");
            return Ok(format!("{text}.bind({receiver})"));
        }
        if self.module.source[start..end].contains("//") {
            return Err(unsupported(
                &decorator.span,
                "a decorator expression with a line comment is not lowered",
            ));
        }
        // TypeScript removes the grouping parentheses around a simple
        // type-asserted decorator name. Relocate the surviving name through
        // the erasure, then retain the original decorator end for the array
        // boundary rather than mapping synthetic parentheses to source.
        if inner.len() == 3
            && tokens.len() == inner.len() + 2
            && matches!(inner[0].kind, TokenKind::Identifier | TokenKind::Keyword)
            && inner[1].is("as")
            && matches!(inner[2].kind, TokenKind::Identifier | TokenKind::Keyword)
        {
            return Ok(format!(
                "{}{}",
                relocated(inner[0].start, inner[0].end),
                super::relocations::source_anchor(end)
            ));
        }
        if inner.len() == 1
            && tokens.len() == 3
            && matches!(inner[0].kind, TokenKind::Identifier | TokenKind::Keyword)
        {
            let leading = &self.module.source[tokens[0].end..inner[0].start];
            let trailing = &self.module.source[inner[0].end..tokens[2].start];
            if leading.chars().all(char::is_whitespace) && trailing.chars().all(char::is_whitespace)
            {
                let space = if leading.is_empty() { "" } else { " " };
                return Ok(format!(
                    "{}({}{space}{}){}",
                    super::relocations::source_anchor(start),
                    super::relocations::source_anchor(inner[0].start),
                    relocated(inner[0].start, inner[0].end),
                    super::relocations::source_anchor(end)
                ));
            }
        }
        Ok(relocated(start, end))
    }

    /// The `{ has, get, set }` access object of a decorated member.
    fn access(plan: &Plan) -> String {
        if let Some(key) = &plan.runtime_key {
            let has = format!("obj => {key} in obj");
            let get = format!("obj => obj[{key}]");
            let set = format!("(obj, value) => {{ obj[{key}] = value; }}");
            return match plan.kind {
                Kind::Method | Kind::Getter => format!("{{ has: {has}, get: {get} }}"),
                Kind::Setter => format!("{{ has: {has}, set: {set} }}"),
                Kind::Field | Kind::Accessor => format!("{{ has: {has}, get: {get}, set: {set} }}"),
            };
        }
        let key = plan.name.clone();
        let field_name = if matches!(plan.kind, Kind::Field | Kind::Getter)
            && !plan.private
            && !plan.literal()
        {
            Some((
                super::relocations::source_anchor(plan.shell.key.first().expect("field key").start),
                super::relocations::source_anchor(plan.shell.key.last().expect("field key").end),
            ))
        } else {
            None
        };
        let has = if plan.private {
            format!("obj => {key} in obj")
        } else {
            format!("obj => {} in obj", quoted(&plan.name))
        };
        let get = if plan.private {
            format!("obj => obj.{key}")
        } else if plan.literal() {
            format!("obj => obj[{}]", quoted(&plan.name))
        } else {
            format!(
                "obj => obj.{}{}{}",
                if plan.kind == Kind::Getter {
                    field_name.as_ref().map_or("", |(start, _)| start.as_str())
                } else {
                    ""
                },
                plan.name,
                field_name.as_ref().map_or("", |(_, end)| end.as_str())
            )
        };
        let set = if plan.private {
            format!("(obj, value) => {{ obj.{key} = value; }}")
        } else if plan.literal() {
            format!("(obj, value) => {{ obj[{}] = value; }}", quoted(&plan.name))
        } else {
            format!(
                "(obj, value) => {{ obj.{}{}{} = value; }}",
                field_name.as_ref().map_or("", |(start, _)| start.as_str()),
                plan.name,
                field_name.as_ref().map_or("", |(_, end)| end.as_str())
            )
        };
        match plan.kind {
            Kind::Method | Kind::Getter => format!("{{ has: {has}, get: {get} }}"),
            Kind::Setter => format!("{{ has: {has}, set: {set} }}"),
            Kind::Field | Kind::Accessor => format!("{{ has: {has}, get: {get}, set: {set} }}"),
        }
    }

    /// A field or auto-accessor: its initializer is wrapped in place, and what it
    /// leaves for the next field (its extra initializers) is queued.
    fn field(
        &mut self,
        class: &ClassDeclaration,
        plan: &Plan,
        class_decorated: bool,
        class_this: &str,
        this_arg_static: &str,
        pending: &mut Vec<String>,
    ) -> Result<(), Diagnostic> {
        let field = plan.shell.field.as_ref().expect("a field plan has a field");
        let run_initializers = self.run_initializers.clone();
        let this_arg = if plan.is_static {
            this_arg_static
        } else {
            "this"
        };
        // The wrapped initializer: decorators' returned initializers, then the
        // initializers queued by earlier members.
        let (prefix, suffix) = {
            let mut prefix = String::new();
            let mut suffix = String::new();
            let inject = !pending.is_empty();
            if inject {
                prefix.push('(');
                prefix.push_str(&pending.join(", "));
                prefix.push_str(", ");
            }
            if plan.decorated() {
                prefix.push_str(&format!(
                    "{run_initializers}({this_arg}, {}, ",
                    plan.initializers_var()
                ));
                suffix.push(')');
            }
            if inject {
                suffix.push(')');
            }
            (prefix, suffix)
        };
        pending.clear();
        if plan.decorated() {
            pending.push(format!(
                "{run_initializers}({this_arg}, {})",
                plan.extra_initializers_var()
            ));
        }
        let wrapped = !prefix.is_empty();
        let shell = plan.shell;
        let initializer_range = field.initializer.as_ref().map(|tokens| {
            (
                tokens.first().expect("an initializer has tokens").start,
                tokens.last().expect("an initializer has tokens").end,
            )
        });
        if class_decorated && plan.is_static {
            if let Some(tokens) = &field.initializer {
                self.rewrite_this_tokens(tokens, class_this);
            }
        }
        if plan.kind == Kind::Accessor
            && (self.options.target != EcmaTarget::EsNext || plan.private)
        {
            let storage = plan.storage();
            let name = if plan.literal() {
                quoted(&plan.name)
            } else {
                plan.name.clone()
            };
            let modifier = if plan.is_static { "static " } else { "" };
            let original_named = self
                .module
                .source
                .get(class.name_span.start..class.name_span.end)
                .is_some_and(|name| name == class.name);
            let direct =
                !plan.private && plan.runtime_key.is_none() && !class_decorated && original_named;
            let storage_prefix = if direct {
                auto_accessors::storage_prefix(
                    class,
                    shell,
                    field,
                    &storage,
                    false,
                    self.options.target < EcmaTarget::Es2022,
                )
            } else {
                format!("{modifier}{storage} = ")
            };
            let accessors = if direct {
                auto_accessors::callable_pair(
                    class,
                    shell,
                    field,
                    &storage,
                    false,
                    self.options.target < EcmaTarget::Es2022,
                )
            } else if plan.private_descriptor() {
                let descriptor = plan.descriptor_var();
                format!("{modifier}get {name}() {{ return {descriptor}.get.call(this); }} {modifier}set {name}(value) {{ {descriptor}.set.call(this, value); }}")
            } else {
                format!("{modifier}get {name}() {{ return this.{storage}; }} {modifier}set {name}(value) {{ this.{storage} = value; }}")
            };
            match initializer_range {
                Some((start, end)) => {
                    self.push(
                        shell.span.start,
                        start,
                        format!(
                            "{storage_prefix}{prefix}{}",
                            self.newlines(shell.span.start, start)
                        ),
                    );
                    self.push(
                        end,
                        shell.span.end,
                        format!(
                            "{}{suffix};{} {accessors}{}",
                            super::relocations::source_anchor(end),
                            super::relocations::source_anchor(shell.span.end),
                            self.newlines(end, shell.span.end)
                        ),
                    );
                }
                None => {
                    let value = if wrapped {
                        format!("{prefix}void 0{suffix}")
                    } else {
                        "void 0".to_string()
                    };
                    self.push(
                        shell.span.start,
                        shell.span.end,
                        format!(
                            "{storage_prefix}{value};{} {accessors}{}",
                            super::relocations::source_anchor(shell.span.end),
                            self.newlines(shell.span.start, shell.span.end)
                        ),
                    );
                }
            }
            return Ok(());
        }
        let _ = class;
        let prefix_origin =
            if self.options.defines_class_fields() && self.options.target < EcmaTarget::Es2022 {
                super::relocations::unmapped().to_string()
            } else {
                super::relocations::source_anchor(field.name_span.end)
            };
        match initializer_range {
            Some((start, end)) if wrapped => {
                self.push(start, start, format!("{prefix_origin}{prefix}"));
                self.push(
                    end,
                    end,
                    format!("{}{suffix}", super::relocations::source_anchor(end)),
                );
            }
            None if wrapped => {
                self.push(
                    field.name_span.end,
                    field.name_span.end,
                    format!("{prefix_origin} = {prefix}void 0{suffix}"),
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// `this` in a static initializer or block of a class whose binding is
    /// replaced stands for the replaced class.
    fn rewrite_this(
        &mut self,
        class: &ClassDeclaration,
        shell: &ClassMemberShell,
        class_this: &str,
    ) {
        let tokens = &class.body[shell.token_start..shell.token_end];
        self.rewrite_this_tokens(tokens, class_this);
    }

    fn rewrite_this_tokens(&mut self, tokens: &[Token], class_this: &str) {
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            // A nested function or class has its own `this`.
            if matches!(token.text.as_str(), "function" | "class") {
                if let Some(open) = tokens[index..].iter().position(|token| token.is("{")) {
                    if let Some(close) = matching_brace(tokens, index + open) {
                        index = close + 1;
                        continue;
                    }
                }
            }
            if token.is("this") && token.end > token.start {
                if let Some(dot) = tokens.get(index + 1).filter(|token| token.is(".")) {
                    let trivia = &self.module.source[token.end..dot.start];
                    if trivia.chars().all(char::is_whitespace) {
                        let anchor = super::relocations::source_anchor(token.start);
                        self.push(
                            token.start,
                            dot.end,
                            format!(
                                "{anchor}{class_this}{anchor}{trivia}.{}",
                                super::relocations::source_anchor(dot.end)
                            ),
                        );
                        index += 2;
                        continue;
                    }
                }
                self.push(token.start, token.end, class_this.to_string());
            }
            index += 1;
        }
    }
}

/// The index of the `}` closing the `{` at `open`.
fn matching_brace(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.text.as_str() {
            "{" => depth += 1,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
