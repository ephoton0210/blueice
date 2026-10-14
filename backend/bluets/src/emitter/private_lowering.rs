// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMAScript private names below ES2022.
//!
//! ES2022 emits `#x` natively. Below it a private name becomes state kept
//! outside the object, as TypeScript does: a `WeakMap` per private field
//! (`_C_x`), one `WeakSet` per class for the brand of its private methods and
//! accessors (`_C_instances`), a function per method or accessor, and a
//! `{ value }` holder per static field, with every access rewritten to a call
//! of one of three small helpers. The helper text is written here from the
//! specified semantics, carries a version, and is emitted once per module ahead
//! of the first class that needs it.
//!
//! The rewrite is deliberately narrow, because it works on tokens and not on
//! an expression tree. A receiver must be `this` or a plain identifier, so
//! duplicating it changes nothing; an assignment, compound assignment or
//! increment must be a whole statement; `#x in o` needs a plain `o`. Anything
//! else is refused rather than guessed.

use std::collections::BTreeSet;

use super::{apply_edits, Module, TextEdit};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassMemberShell};
use crate::Token;

/// The version of the emitted private-name helper text. It is part of the
/// build fingerprint, so a change to the helpers is a change to the artifact.
pub const CLASS_HELPER_V1_VERSION: &str = "bluets-class-helper-v1";

const GET_HELPER: &str = "__bluetsClassPrivateGet";
const SET_HELPER: &str = "__bluetsClassPrivateSet";
const IN_HELPER: &str = "__bluetsClassPrivateIn";

/// A helper a module needs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Helper {
    Get,
    Set,
    In,
}

/// The helper definitions for a module, or nothing when none is used. Each is a
/// `var` so it is available to every class in the module once its statement has
/// run, which is before any class that uses it is defined.
pub(super) fn helper_definitions(needed: &BTreeSet<Helper>) -> String {
    if needed.is_empty() {
        return String::new();
    }
    let mut text = format!("/* {CLASS_HELPER_V1_VERSION} */ ");
    for helper in needed {
        text.push_str(match helper {
            Helper::Get => concat!(
                "var __bluetsClassPrivateGet = function (receiver, state, kind, holder) { ",
                "if (kind === \"a\" && !holder) throw new TypeError(\"Private accessor was defined without a getter\"); ",
                "if (typeof state === \"function\" ? receiver !== state || !holder : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\"); ",
                "return kind === \"m\" ? holder : kind === \"a\" ? holder.call(receiver) : holder ? holder.value : state.get(receiver); }; "
            ),
            Helper::Set => concat!(
                "var __bluetsClassPrivateSet = function (receiver, state, value, kind, holder) { ",
                "if (kind === \"m\") throw new TypeError(\"Private method is not writable\"); ",
                "if (kind === \"a\" && !holder) throw new TypeError(\"Private accessor was defined without a setter\"); ",
                "if (typeof state === \"function\" ? receiver !== state || !holder : !state.has(receiver)) throw new TypeError(\"Cannot write private member to an object whose class did not declare it\"); ",
                "return (kind === \"a\" ? holder.call(receiver, value) : holder ? holder.value = value : state.set(receiver, value)), value; }; "
            ),
            Helper::In => concat!(
                "var __bluetsClassPrivateIn = function (state, receiver) { ",
                "if (receiver === null || (typeof receiver !== \"object\" && typeof receiver !== \"function\")) throw new TypeError(\"Cannot use 'in' operator on non-object\"); ",
                "return typeof state === \"function\" ? receiver === state : state.has(receiver); }; "
            ),
        });
    }
    text
}

/// What a private name declares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Field,
    Method,
    Accessor,
}

/// One private name of a class: how it is stored and read.
struct PrivateName {
    /// The name with its `#`.
    name: String,
    kind: Kind,
    is_static: bool,
    /// The WeakMap (instance field) or `{ value }` holder (static field), or
    /// the method's function; for an accessor, unused.
    variable: String,
    getter_variable: Option<String>,
    setter_variable: Option<String>,
}

pub(super) struct PrivateNames<'a> {
    class: &'a ClassDeclaration,
    names: Vec<PrivateName>,
    /// `_C_instances`, when the class has an instance method or accessor.
    instances: Option<String>,
}

impl<'a> PrivateNames<'a> {
    /// The private names a class declares, or `None` for a class with none.
    pub(super) fn collect(
        module: &Module,
        class: &'a ClassDeclaration,
    ) -> Result<Option<Self>, Diagnostic> {
        let class_name = &class.name;
        let mut names: Vec<PrivateName> = Vec::new();
        let mut has_instance_brand = false;
        for member in &class.members {
            let (name, kind, is_static, accessor_getter) = if let Some(field) = &member.field {
                (&field.name, Kind::Field, field.is_static, None)
            } else if let Some(method) = &member.method {
                (&method.name, Kind::Method, method.is_static, None)
            } else if let Some(accessor) = &member.accessor {
                (
                    &accessor.name,
                    Kind::Accessor,
                    accessor.is_static,
                    Some(accessor.getter),
                )
            } else {
                continue;
            };
            if !name.starts_with('#') {
                continue;
            }
            let stem = format!("_{class_name}_{}", &name[1..]);
            if let Some(existing) = names.iter_mut().find(|entry| entry.name == *name) {
                // The second half of a getter/setter pair.
                match accessor_getter {
                    Some(true) => existing.getter_variable = Some(format!("{stem}_get")),
                    Some(false) => existing.setter_variable = Some(format!("{stem}_set")),
                    None => {}
                }
                continue;
            }
            if !is_static && kind != Kind::Field {
                has_instance_brand = true;
            }
            names.push(PrivateName {
                name: name.clone(),
                kind,
                is_static,
                variable: stem.clone(),
                getter_variable: (accessor_getter == Some(true)).then(|| format!("{stem}_get")),
                setter_variable: (accessor_getter == Some(false)).then(|| format!("{stem}_set")),
            });
        }
        if names.is_empty() {
            return Ok(None);
        }
        let instances = has_instance_brand.then(|| format!("_{class_name}_instances"));
        let this = Self {
            class,
            names,
            instances,
        };
        for variable in this.variables() {
            if mentions_identifier(&module.source, &variable) {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    class.name_span.clone(),
                    format!("the identifier `{variable}` is needed to lower this class's private names but is already used"),
                ));
            }
        }
        Ok(Some(this))
    }

    /// Every variable the lowering declares for the class.
    pub(super) fn variables(&self) -> Vec<String> {
        let mut variables: Vec<String> = self.instances.iter().cloned().collect();
        for entry in &self.names {
            match entry.kind {
                Kind::Field | Kind::Method => variables.push(entry.variable.clone()),
                Kind::Accessor => {
                    variables.extend(entry.getter_variable.iter().cloned());
                    variables.extend(entry.setter_variable.iter().cloned());
                }
            }
        }
        variables
    }

    fn find(&self, name: &str) -> Option<&PrivateName> {
        self.names.iter().find(|entry| entry.name == name)
    }

    /// The first argument to the helpers: the brand or store the name lives in.
    fn state(&self, entry: &PrivateName) -> String {
        if entry.is_static {
            self.class.name.clone()
        } else if entry.kind == Kind::Field {
            entry.variable.clone()
        } else {
            self.instances
                .clone()
                .expect("an instance method or accessor has a brand")
        }
    }

    /// The trailing `"kind", holder` arguments of a read.
    fn read_tail(&self, entry: &PrivateName) -> String {
        match entry.kind {
            Kind::Field if entry.is_static => format!("\"f\", {}", entry.variable),
            Kind::Field => "\"f\"".to_string(),
            Kind::Method => format!("\"m\", {}", entry.variable),
            Kind::Accessor => format!(
                "\"a\", {}",
                entry.getter_variable.as_deref().unwrap_or("void 0")
            ),
        }
    }

    /// The trailing `"kind", holder` arguments of a write.
    fn write_tail(&self, entry: &PrivateName) -> String {
        match entry.kind {
            Kind::Field if entry.is_static => format!("\"f\", {}", entry.variable),
            Kind::Field => "\"f\"".to_string(),
            Kind::Method => format!("\"m\", {}", entry.variable),
            Kind::Accessor => format!(
                "\"a\", {}",
                entry.setter_variable.as_deref().unwrap_or("void 0")
            ),
        }
    }

    fn read(&self, entry: &PrivateName, receiver: &str) -> String {
        format!(
            "{GET_HELPER}({receiver}, {}, {})",
            self.state(entry),
            self.read_tail(entry)
        )
    }

    fn write(&self, entry: &PrivateName, receiver: &str, value: &str) -> String {
        format!(
            "{SET_HELPER}({receiver}, {}, {value}, {})",
            self.state(entry),
            self.write_tail(entry)
        )
    }

    /// `_C_instances.add(this);` for a class with private methods or accessors.
    pub(super) fn brand_statement(&self) -> String {
        self.instances
            .as_ref()
            .map(|instances| format!(" {instances}.add(this);"))
            .unwrap_or_default()
    }

    /// `_C_x.set(this, init);` for an instance private field.
    pub(super) fn field_store(&self, name: &str, value: Option<String>) -> String {
        let entry = self.find(name).expect("a private field was collected");
        format!(
            " {}.set(this, {});",
            entry.variable,
            value.unwrap_or_else(|| "void 0".to_string())
        )
    }

    /// The statement that sets up a static private field.
    pub(super) fn static_field_store(&self, name: &str, value: Option<String>) -> String {
        let entry = self.find(name).expect("a private field was collected");
        format!(
            " {} = {{ value: {} }};",
            entry.variable,
            value.unwrap_or_else(|| "void 0".to_string())
        )
    }

    /// `_C_x = new WeakMap();` for each instance private field, and
    /// `_C_instances = new WeakSet();`.
    pub(super) fn instance_setup(&self) -> String {
        let mut text = String::new();
        for entry in &self.names {
            if entry.kind == Kind::Field && !entry.is_static {
                text.push_str(&format!(" {} = new WeakMap();", entry.variable));
            }
        }
        if let Some(instances) = &self.instances {
            text.push_str(&format!(" {instances} = new WeakSet();"));
        }
        text
    }

    /// The variable a method or accessor member is assigned to, and the text of
    /// its function: `_C_m = function _C_m(params) { .. };`.
    pub(super) fn function_definition(
        &self,
        member: &ClassMemberShell,
        rendered: &str,
    ) -> Result<String, Diagnostic> {
        let (name, variable) = if let Some(method) = &member.method {
            let entry = self
                .find(&method.name)
                .expect("a private method was collected");
            (&method.name, entry.variable.clone())
        } else if let Some(accessor) = &member.accessor {
            let entry = self
                .find(&accessor.name)
                .expect("a private accessor was collected");
            let variable = if accessor.getter {
                entry.getter_variable.clone()
            } else {
                entry.setter_variable.clone()
            }
            .expect("the accessor half was collected");
            (&accessor.name, variable)
        } else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                member.span.clone(),
                "this private member has no lowering",
            ));
        };
        // Cut the modifiers and the name; what is left is `(params) { body }`.
        let Some(at) = rendered.find(name.as_str()) else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                member.span.clone(),
                "this private member could not be located in its own text",
            ));
        };
        let rest = &rendered[at + name.len()..];
        Ok(format!(" {variable} = function {variable}{rest};"))
    }

    /// Rewrites every access to a private name in the class body, adding the
    /// helpers it needs to `needed`. Accesses are handled last to first, so the
    /// right-hand side of an assignment is already rewritten when the
    /// assignment is.
    pub(super) fn rewrite_accesses(
        &self,
        module: &Module,
        edits: &mut Vec<TextEdit>,
        needed: &mut BTreeSet<Helper>,
    ) -> Result<(), Diagnostic> {
        let tokens = &self.class.body;
        let mut indices: Vec<usize> = (0..tokens.len())
            .filter(|&index| {
                tokens[index].kind == crate::TokenKind::Identifier
                    && tokens[index].text.starts_with('#')
                    && self.find(&tokens[index].text).is_some()
            })
            .collect();
        indices.reverse();
        for index in indices {
            let after_dot = index >= 1 && tokens[index - 1].is(".");
            if after_dot {
                self.rewrite_member_access(module, tokens, index, edits, needed)?;
            } else if tokens.get(index + 1).is_some_and(|next| next.is("in")) {
                self.rewrite_brand_check(module, tokens, index, edits, needed)?;
            }
        }
        Ok(())
    }

    fn rewrite_member_access(
        &self,
        module: &Module,
        tokens: &[Token],
        index: usize,
        edits: &mut Vec<TextEdit>,
        needed: &mut BTreeSet<Helper>,
    ) -> Result<(), Diagnostic> {
        let name = &tokens[index];
        let entry = self.find(&name.text).expect("collected above");
        let span = name.span(&module.id);
        // A receiver is `this` or one plain identifier.
        let receiver_index = index.checked_sub(2);
        let receiver = receiver_index
            .and_then(|at| tokens.get(at))
            .filter(|token| {
                (token.is("this") || token.kind == crate::TokenKind::Identifier)
                    && !token.text.starts_with('#')
            });
        let (Some(receiver_index), Some(receiver)) = (receiver_index, receiver) else {
            return Err(complex(&span, &name.text));
        };
        if receiver_index >= 1
            && (tokens[receiver_index - 1].is(".") || tokens[receiver_index - 1].is("?."))
        {
            return Err(complex(&span, &name.text));
        }
        let receiver_text = receiver.text.clone();
        let statement_start = is_statement_start(tokens, receiver_index);
        let next = tokens.get(index + 1);

        // `++RECV.#x;` / `--RECV.#x;`
        let prefix = receiver_index
            .checked_sub(1)
            .and_then(|at| tokens.get(at))
            .filter(|token| token.is("++") || token.is("--"));
        if let Some(prefix) = prefix {
            let start_index = receiver_index - 1;
            if !is_statement_start(tokens, start_index) || !next.is_some_and(|next| next.is(";")) {
                return Err(unsupported_form(
                    &span,
                    "an increment or decrement used as a value",
                ));
            }
            let delta = if prefix.is("++") { "+" } else { "-" };
            needed.extend([Helper::Get, Helper::Set]);
            push_replacement(
                edits,
                prefix.start,
                name.end,
                self.write(
                    entry,
                    &receiver_text,
                    &format!("{} {delta} 1", self.read(entry, &receiver_text)),
                ),
            );
            return Ok(());
        }
        // `RECV.#x++;` / `RECV.#x--;`
        if let Some(operator) = next.filter(|token| token.is("++") || token.is("--")) {
            if !statement_start || !tokens.get(index + 2).is_some_and(|after| after.is(";")) {
                return Err(unsupported_form(
                    &span,
                    "an increment or decrement used as a value",
                ));
            }
            let delta = if operator.is("++") { "+" } else { "-" };
            needed.extend([Helper::Get, Helper::Set]);
            push_replacement(
                edits,
                receiver.start,
                operator.end,
                self.write(
                    entry,
                    &receiver_text,
                    &format!("{} {delta} 1", self.read(entry, &receiver_text)),
                ),
            );
            return Ok(());
        }
        // Assignment and compound assignment, as a whole statement.
        if let Some(operator) = next.filter(|token| is_assignment(&token.text)) {
            if !statement_start {
                return Err(unsupported_form(&span, "an assignment used as a value"));
            }
            let Some(end) = statement_end(tokens, index + 2) else {
                return Err(unsupported_form(&span, "an assignment with no closing `;`"));
            };
            let (Some(first), Some(last)) =
                (tokens.get(index + 2), tokens.get(end.wrapping_sub(1)))
            else {
                return Err(unsupported_form(&span, "an assignment with no value"));
            };
            if end <= index + 2 {
                return Err(unsupported_form(&span, "an assignment with no value"));
            }
            let value = render(module, edits, first.start, last.end);
            needed.insert(Helper::Set);
            let replacement = if operator.is("=") {
                self.write(entry, &receiver_text, &value)
            } else {
                let Some(binary) = operator
                    .text
                    .strip_suffix('=')
                    .filter(|binary| !matches!(*binary, "&&" | "||" | "??"))
                else {
                    return Err(unsupported_form(&span, "a logical assignment"));
                };
                needed.insert(Helper::Get);
                self.write(
                    entry,
                    &receiver_text,
                    &format!("{} {binary} ({value})", self.read(entry, &receiver_text)),
                )
            };
            push_replacement(edits, receiver.start, last.end, replacement);
            return Ok(());
        }
        // A call: `RECV.#m(args)` keeps `this` for the function.
        if next.is_some_and(|next| next.is("(")) {
            needed.insert(Helper::Get);
            let opening = &tokens[index + 1];
            let empty = tokens.get(index + 2).is_some_and(|token| token.is(")"));
            push_replacement(
                edits,
                receiver.start,
                opening.end,
                format!(
                    "{}.call({receiver_text}{}",
                    self.read(entry, &receiver_text),
                    if empty { "" } else { ", " }
                ),
            );
            return Ok(());
        }
        // A read.
        needed.insert(Helper::Get);
        push_replacement(
            edits,
            receiver.start,
            name.end,
            self.read(entry, &receiver_text),
        );
        Ok(())
    }

    fn rewrite_brand_check(
        &self,
        module: &Module,
        tokens: &[Token],
        index: usize,
        edits: &mut Vec<TextEdit>,
        needed: &mut BTreeSet<Helper>,
    ) -> Result<(), Diagnostic> {
        let name = &tokens[index];
        let entry = self.find(&name.text).expect("collected above");
        let span = name.span(&module.id);
        let Some(operand) = tokens.get(index + 2).filter(|token| {
            (token.is("this") || token.kind == crate::TokenKind::Identifier)
                && !token.text.starts_with('#')
        }) else {
            return Err(unsupported_form(
                &span,
                "a brand check on a complex operand",
            ));
        };
        // The operand must end here: `#x in o.y` or `#x in o(...)` is more than
        // the plain identifier.
        if tokens
            .get(index + 3)
            .is_some_and(|after| after.is(".") || after.is("?.") || after.is("(") || after.is("["))
        {
            return Err(unsupported_form(
                &span,
                "a brand check on a complex operand",
            ));
        }
        needed.insert(Helper::In);
        push_replacement(
            edits,
            name.start,
            operand.end,
            format!("{IN_HELPER}({}, {})", self.state(entry), operand.text),
        );
        Ok(())
    }
}

fn complex(span: &SourceSpan, name: &str) -> Diagnostic {
    unsupported_form(
        span,
        &format!(
            "private name `{name}` accessed through anything but `this` or a plain identifier"
        ),
    )
}

fn unsupported_form(span: &SourceSpan, what: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        span.clone(),
        format!("{what} cannot be lowered for a target below ES2022"),
    )
}

fn is_assignment(text: &str) -> bool {
    matches!(
        text,
        "=" | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "**="
            | "<<="
            | ">>="
            | ">>>="
            | "&="
            | "|="
            | "^="
            | "&&="
            | "||="
            | "??="
    )
}

/// Whether the token at `index` begins a statement: it follows `;`, `{` or `}`,
/// or is the first token of the class body.
fn is_statement_start(tokens: &[Token], index: usize) -> bool {
    match index.checked_sub(1).and_then(|before| tokens.get(before)) {
        None => true,
        Some(before) => before.is(";") || before.is("{") || before.is("}"),
    }
}

/// The index of the `;` that ends the expression statement whose value starts
/// at `start`, skipping nested brackets.
fn statement_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            ";" if depth == 0 => return Some(offset),
            _ => {}
        }
    }
    None
}

fn push_replacement(edits: &mut Vec<TextEdit>, start: usize, end: usize, replacement: String) {
    edits.retain(|edit| !(edit.start >= start && edit.end <= end));
    edits.push(TextEdit {
        start,
        end,
        replacement,
    });
}

/// The source text of a range with the recorded edits inside it applied.
fn render(module: &Module, edits: &[TextEdit], start: usize, end: usize) -> String {
    let inner: Vec<TextEdit> = edits
        .iter()
        .filter(|edit| edit.start >= start && edit.end <= end)
        .map(|edit| TextEdit {
            start: edit.start - start,
            end: edit.end - start,
            replacement: edit.replacement.clone(),
        })
        .collect();
    apply_edits(&module.source[start..end], inner).javascript
}

/// Whether `identifier` appears in `source` as a whole word.
fn mentions_identifier(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(index, _)| {
        let before = source[..index].chars().next_back();
        let after = source[index + identifier.len()..].chars().next();
        let part = |character: Option<char>| {
            character.is_some_and(|character| {
                character.is_alphanumeric() || matches!(character, '_' | '$')
            })
        };
        !part(before) && !part(after)
    })
}

/// Helpers already spelled in the source would collide with the emitted ones.
pub(super) fn refuse_helper_name_collisions(module: &Module) -> Result<(), Diagnostic> {
    for helper in [GET_HELPER, SET_HELPER, IN_HELPER] {
        if mentions_identifier(&module.source, helper) {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                format!("the identifier `{helper}` is reserved for the private-name helpers"),
            ));
        }
    }
    Ok(())
}
