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
//! ESNext define semantics preserve proposals. Literal member names, decorators
//! on constructors/static blocks/overload signatures, super in decorated private
//! callables and static-initializer super writes retain precise refusals.

use std::collections::BTreeSet;

mod private_members;
mod scopes;
mod super_members;
pub(super) mod targets;
pub(super) use scopes::namespace_owner;

use super::{Module, TextEdit};
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassMemberKind, ClassMemberShell, Declaration, Decorator};
use crate::{Token, TokenKind};

/// The version of the emitted decorator helper text; part of the build fingerprint.
pub const DECORATOR_HELPER_V1_VERSION: &str = "bluets-decorator-helper-v1";

const RUN_INITIALIZERS: &str = "__bluetsRunInitializers";
const ES_DECORATE: &str = "__bluetsEsDecorate";

/// `__bluetsRunInitializers(thisArg, initializers[, value])`: runs each
/// initializer with `this` as `thisArg`, threading `value` through when given.
const RUN_INITIALIZERS_HELPER: &str = "var __bluetsRunInitializers = function (thisArg, initializers, value) { var useValue = arguments.length > 2; for (var i = 0; i < initializers.length; i++) { value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg); } return useValue ? value : void 0; };";

/// `__bluetsEsDecorate(ctor, descriptor, decorators, context, initializers,
/// extraInitializers)`: applies `decorators` last to first to one element.
const ES_DECORATE_HELPER: &str = "var __bluetsEsDecorate = function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) { function accept(f) { if (f !== void 0 && typeof f !== \"function\") throw new TypeError(\"Function expected\"); return f; } var kind = contextIn.kind, key = kind === \"getter\" ? \"get\" : kind === \"setter\" ? \"set\" : \"value\"; var target = !descriptorIn && ctor ? contextIn[\"static\"] ? ctor : ctor.prototype : null; var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {}); var _, done = false; for (var i = decorators.length - 1; i >= 0; i--) { var context = {}; for (var p in contextIn) context[p] = p === \"access\" ? {} : contextIn[p]; for (var p in contextIn.access) context.access[p] = contextIn.access[p]; context.addInitializer = function (f) { if (done) throw new TypeError(\"Cannot add initializers after decoration has completed\"); extraInitializers.push(accept(f || null)); }; var result = (0, decorators[i])(kind === \"accessor\" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context); if (kind === \"accessor\") { if (result === void 0) continue; if (result === null || typeof result !== \"object\") throw new TypeError(\"Object expected\"); if (_ = accept(result.get)) descriptor.get = _; if (_ = accept(result.set)) descriptor.set = _; if (_ = accept(result.init)) initializers.unshift(_); } else if (_ = accept(result)) { if (kind === \"field\") initializers.unshift(_); else descriptor[key] = _; } } if (target) Object.defineProperty(target, contextIn.name, descriptor); done = true; };";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Method,
    Getter,
    Setter,
    Field,
    Accessor,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Field => "field",
            Self::Accessor => "accessor",
        }
    }

    fn is_field(self) -> bool {
        matches!(self, Self::Field)
    }
}

/// One member of the class, as the lowering sees it.
struct Plan<'a> {
    shell: &'a ClassMemberShell,
    kind: Kind,
    is_static: bool,
    private: bool,
    /// `x` or `#p`.
    name: String,
    /// The helper variable's stem: `_static_get_x`, `_private_p`.
    var: String,
    /// Captured property key, evaluated once at class definition.
    runtime_key: Option<String>,
}

impl Plan<'_> {
    fn decorated(&self) -> bool {
        !self.shell.decorators.is_empty()
    }

    fn decorators_var(&self) -> String {
        format!("{}_decorators", self.var)
    }

    fn initializers_var(&self) -> String {
        format!("{}_initializers", self.var)
    }

    fn extra_initializers_var(&self) -> String {
        format!("{}_extraInitializers", self.var)
    }

    /// The private name of an auto-accessor's backing field.
    fn storage(&self) -> String {
        format!("#{}_accessor_storage", self.name.trim_start_matches('#'))
    }
}

struct Names {
    used: BTreeSet<String>,
}

impl Names {
    fn unique(&mut self, wanted: &str) -> String {
        let mut candidate = wanted.to_string();
        let mut count = 0;
        while self.used.contains(&candidate) {
            count += 1;
            candidate = format!("{wanted}_{count}");
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

fn unsupported(span: &SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span.clone(), message)
}

/// A relocation marker: the text of source range `start..end`, with the edits
/// inside it applied, is written here instead (see `apply_edits`).
pub(super) fn relocated(start: usize, end: usize) -> String {
    format!("\u{0}M{start},{end}\u{0}")
}

/// Whether the emitter lowers this class's decorators or auto-accessors (so the
/// other passes leave its `export` and its name to this one).
pub(super) fn lowers_class(class: &ClassDeclaration, options: &CompilerOptions) -> bool {
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
) -> Result<(), Diagnostic> {
    if preserves_proposals(options) {
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
            .map(|token| token.text.clone())
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
            names: &mut names,
            edits,
        }
        .class(class, false)?;
        let helper_start = scopes::helper_start(module, class);
        first_class_start = Some(first_class_start.map_or(helper_start, |at| at.min(helper_start)));
    }
    for expression in module.class_expressions.values() {
        if !needs_lowering(&expression.class) {
            continue;
        }
        let mut class = expression.class.clone();
        class.name = expression
            .name
            .clone()
            .unwrap_or_else(|| names.unique("_decoratedClass"));
        Lowerer {
            module,
            options,
            names: &mut names,
            edits,
        }
        .class(&class, true)?;
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
    names: &'a mut Names,
    edits: &'e mut Vec<TextEdit>,
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
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
        Ok(relocated(start, end))
    }

    fn plans<'c>(&mut self, class: &'c ClassDeclaration) -> Result<Vec<Plan<'c>>, Diagnostic> {
        let mut plans = Vec::new();
        for shell in &class.members {
            if shell
                .key
                .first()
                .is_some_and(|key| matches!(key.kind, TokenKind::String | TokenKind::Number))
            {
                return Err(unsupported(
                    &shell.span,
                    "computed or literal member names in decorator lowering are not supported yet",
                ));
            }
            let (kind, is_static, name) = match shell.kind {
                ClassMemberKind::Method => {
                    let Some(method) = &shell.method else {
                        if shell.decorators.is_empty() {
                            continue;
                        }
                        return Err(unsupported(
                            &shell.span,
                            "this decorated method shape is not lowered",
                        ));
                    };
                    if method.body.is_none() {
                        if let Some(decorator) = shell.decorators.first() {
                            return Err(unsupported(
                                &decorator.span,
                                "a decorator can only decorate a method implementation, not an overload",
                            ));
                        }
                        continue;
                    }
                    (Kind::Method, method.is_static, method.name.clone())
                }
                ClassMemberKind::Accessor => {
                    let Some(accessor) = &shell.accessor else {
                        continue;
                    };
                    (
                        if accessor.getter {
                            Kind::Getter
                        } else {
                            Kind::Setter
                        },
                        accessor.is_static,
                        accessor.name.clone(),
                    )
                }
                ClassMemberKind::Field => {
                    let Some(field) = &shell.field else {
                        continue;
                    };
                    (
                        if field.accessor {
                            Kind::Accessor
                        } else {
                            Kind::Field
                        },
                        field.is_static,
                        field.name.clone(),
                    )
                }
                ClassMemberKind::Constructor
                | ClassMemberKind::StaticBlock
                | ClassMemberKind::IndexSignature => {
                    if let Some(decorator) = shell.decorators.first() {
                        return Err(unsupported(
                            &decorator.span,
                            "decorators are not valid here",
                        ));
                    }
                    continue;
                }
                ClassMemberKind::Opaque => {
                    if let Some(decorator) = shell.decorators.first() {
                        return Err(unsupported(
                            &decorator.span,
                            "this decorated member shape (computed or literal name, or a form the class parser does not structure) is not lowered",
                        ));
                    }
                    continue;
                }
            };
            let private = name.starts_with('#');
            let mut stem = String::from("_");
            if is_static {
                stem.push_str("static_");
            }
            match kind {
                Kind::Getter => stem.push_str("get_"),
                Kind::Setter => stem.push_str("set_"),
                _ => {}
            }
            if private {
                stem.push_str("private_");
            }
            let computed = shell.key.first().is_some_and(|key| key.is("["));
            stem.push_str(if computed {
                "member"
            } else {
                name.trim_start_matches('#')
            });
            // Only decorated members get helper variables; keep their stems unique.
            let var = if shell.decorators.is_empty() {
                stem
            } else {
                self.names.unique(&stem)
            };
            let runtime_key = computed.then(|| self.names.unique("_computedKey"));
            plans.push(Plan {
                runtime_key,
                shell,
                kind,
                is_static,
                private,
                name,
                var,
            });
        }
        Ok(plans)
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
        let has = if plan.private {
            format!("obj => {key} in obj")
        } else {
            format!("obj => {} in obj", quoted(&plan.name))
        };
        let get = if plan.private {
            format!("obj => obj.{key}")
        } else {
            format!("obj => obj.{}", plan.name)
        };
        let set = if plan.private {
            format!("(obj, value) => {{ obj.{key} = value; }}")
        } else {
            format!("(obj, value) => {{ obj.{} = value; }}", plan.name)
        };
        match plan.kind {
            Kind::Method | Kind::Getter => format!("{{ has: {has}, get: {get} }}"),
            Kind::Setter => format!("{{ has: {has}, set: {set} }}"),
            Kind::Field | Kind::Accessor => format!("{{ has: {has}, get: {get}, set: {set} }}"),
        }
    }

    fn class(&mut self, class: &ClassDeclaration, expression: bool) -> Result<(), Diagnostic> {
        let plans = self.plans(class)?;
        let class_decorated = !class.decorators.is_empty();
        let extends = class.extends_span.clone();
        let class_this = if class_decorated {
            self.names.unique("_classThis")
        } else {
            "this".to_string()
        };
        let class_decorators = self.names.unique("_classDecorators");
        let class_descriptor = self.names.unique("_classDescriptor");
        let class_extra = self.names.unique("_classExtraInitializers");
        let class_super = extends.as_ref().map(|_| self.names.unique("_classSuper"));
        let metadata = self.names.unique("_metadata");
        let static_extra = plans
            .iter()
            .any(|plan| {
                plan.decorated()
                    && plan.is_static
                    && !plan.kind.is_field()
                    && plan.kind != Kind::Accessor
            })
            .then(|| self.names.unique("_staticExtraInitializers"));
        let instance_extra = plans
            .iter()
            .any(|plan| {
                plan.decorated()
                    && !plan.is_static
                    && !plan.kind.is_field()
                    && plan.kind != Kind::Accessor
            })
            .then(|| self.names.unique("_instanceExtraInitializers"));

        if let Some(base) = &class_super {
            self.rewrite_static_super(class, base, &class_this)?;
        }

        // ---- the header --------------------------------------------------
        let mut lets = String::new();
        if class_decorated {
            let decorators = class
                .decorators
                .iter()
                .map(|decorator| self.decorator_expression(decorator))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            lets.push_str(&format!(
                "let {class_decorators} = [{decorators}]; let {class_descriptor}; let {class_extra} = []; let {class_this}; "
            ));
        }
        if let (Some(name), Some(span)) = (&class_super, &extends) {
            lets.push_str(&format!(
                "let {name} = {}; ",
                relocated(span.start, span.end)
            ));
        }
        if let Some(name) = &static_extra {
            lets.push_str(&format!("let {name} = []; "));
        }
        if let Some(name) = &instance_extra {
            lets.push_str(&format!("let {name} = []; "));
        }
        for plan in &plans {
            if let Some(key) = &plan.runtime_key {
                lets.push_str(&format!("let {key}; "));
            }
        }
        for is_static in [true, false] {
            for plan in plans
                .iter()
                .filter(|plan| plan.decorated() && plan.is_static == is_static)
            {
                lets.push_str(&format!("let {}; ", plan.decorators_var()));
                if plan.private_descriptor() {
                    lets.push_str(&format!("let {}; ", plan.descriptor_var()));
                }
                if matches!(plan.kind, Kind::Field | Kind::Accessor) {
                    lets.push_str(&format!(
                        "let {} = []; let {} = []; ",
                        plan.initializers_var(),
                        plan.extra_initializers_var()
                    ));
                }
            }
        }

        // ---- the leading static block -------------------------------------
        let mut leading = String::new();
        leading.push_str(&format!(
            "const {metadata} = typeof Symbol === \"function\" && Symbol.metadata ? Object.create({}) : void 0; ",
            match &class_super {
                Some(name) => format!("{name}[Symbol.metadata] ?? null"),
                None => "null".to_string(),
            }
        ));
        for plan in plans.iter().filter(|plan| plan.decorated()) {
            let decorators = plan
                .shell
                .decorators
                .iter()
                .map(|decorator| self.decorator_expression(decorator))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            if let Some(key) = &plan.runtime_key {
                let tokens = &plan.shell.key;
                let start = tokens.first().expect("a computed key has an opener").end;
                let end = tokens.last().expect("a computed key has a closer").start;
                let value = relocated(start, end);
                self.push(tokens[0].start, tokens.last().unwrap().end,
                    format!("[({} = [{decorators}], {key} = (typeof ({key} = {value}) === \"symbol\" ? {key} : \"\".concat({key})))]", plan.decorators_var()));
            } else {
                leading.push_str(&format!("{} = [{decorators}]; ", plan.decorators_var()));
            }
            if plan.private_descriptor() {
                if plan.kind == Kind::Accessor {
                    leading.push_str(&plan.private_accessor_descriptor());
                } else {
                    leading.push_str(&self.private_callable(class, plan)?);
                }
            }
        }
        let this_for = |plan: &Plan| -> &'static str {
            if matches!(
                plan.kind,
                Kind::Method | Kind::Getter | Kind::Setter | Kind::Accessor
            ) {
                "this"
            } else {
                "null"
            }
        };
        let decorate = |plan: &Plan,
                        static_extra: &Option<String>,
                        instance_extra: &Option<String>|
         -> String {
            let name = plan
                .runtime_key
                .clone()
                .unwrap_or_else(|| quoted(&plan.name));
            let context = format!(
                "{{ kind: {}, name: {name}, static: {}, private: {}, access: {}, metadata: {metadata} }}",
                quoted(plan.kind.label()),
                plan.is_static,
                plan.private,
                Self::access(plan)
            );
            let (initializers, extra) = match plan.kind {
                Kind::Field | Kind::Accessor => {
                    (plan.initializers_var(), plan.extra_initializers_var())
                }
                _ => (
                    "null".to_string(),
                    if plan.is_static {
                        static_extra
                            .clone()
                            .expect("static extra initializers exist")
                    } else {
                        instance_extra
                            .clone()
                            .expect("instance extra initializers exist")
                    },
                ),
            };
            let (target, descriptor) = if plan.private_descriptor() {
                ("null", plan.descriptor_var())
            } else {
                (this_for(plan), "null".to_string())
            };
            format!(
                "{ES_DECORATE}({target}, {descriptor}, {}, {context}, {initializers}, {extra}); ",
                plan.decorators_var()
            )
        };
        let non_field = |plan: &&Plan| {
            matches!(
                plan.kind,
                Kind::Method | Kind::Getter | Kind::Setter | Kind::Accessor
            )
        };
        for (is_static, want_fields) in [(true, false), (false, false), (true, true), (false, true)]
        {
            for plan in plans.iter().filter(|plan| {
                plan.decorated() && plan.is_static == is_static && non_field(plan) != want_fields
            }) {
                leading.push_str(&decorate(plan, &static_extra, &instance_extra));
            }
        }
        if class_decorated {
            leading.push_str(&format!(
                "{ES_DECORATE}(null, {class_descriptor} = {{ value: {class_this} }}, {class_decorators}, {{ kind: \"class\", name: {class_this}.name, metadata: {metadata} }}, null, {class_extra}); {} = {class_this} = {class_descriptor}.value; ",
                class.name
            ));
        }
        leading.push_str(&format!(
            "if ({metadata}) Object.defineProperty({class_this}, Symbol.metadata, {{ enumerable: true, configurable: true, writable: true, value: {metadata} }}); "
        ));

        // ---- the members -------------------------------------------------
        let this_arg_static = if class_decorated {
            class_this.clone()
        } else {
            "this".to_string()
        };
        let mut pending_static: Vec<String> = Vec::new();
        let mut pending_instance: Vec<String> = Vec::new();
        if let Some(name) = &static_extra {
            pending_static.push(format!("{RUN_INITIALIZERS}({this_arg_static}, {name})"));
        }
        if let Some(name) = &instance_extra {
            pending_instance.push(format!("{RUN_INITIALIZERS}(this, {name})"));
        }
        let mut has_static_initializers = false;
        let mut constructor: Option<&ClassMemberShell> = None;
        let body_open = class.body_span.start;
        for shell in &class.members {
            // Decorators are erased where they stand; their expressions were
            // relocated into the arrays above.
            for decorator in &shell.decorators {
                self.push(decorator.span.start, decorator.span.end, String::new());
            }
            match shell.kind {
                ClassMemberKind::Constructor => constructor = Some(shell),
                ClassMemberKind::StaticBlock => {
                    has_static_initializers = true;
                    if !pending_static.is_empty() {
                        let statements = std::mem::take(&mut pending_static).join(", ");
                        self.push(
                            shell.span.start,
                            shell.span.start,
                            format!("static {{ {statements}; }} "),
                        );
                    }
                    if class_decorated {
                        self.rewrite_this(class, shell, &class_this);
                    }
                }
                ClassMemberKind::Field => {
                    let plan = plans.iter().find(|plan| std::ptr::eq(plan.shell, shell));
                    let Some(field) = &shell.field else { continue };
                    if field.is_static
                        && (field.initializer.is_some() || plan.is_some_and(Plan::decorated))
                    {
                        has_static_initializers = true;
                    }
                    let Some(plan) = plan else { continue };
                    self.field(
                        class,
                        plan,
                        class_decorated,
                        &class_this,
                        &this_arg_static,
                        if plan.is_static {
                            &mut pending_static
                        } else {
                            &mut pending_instance
                        },
                    )?;
                }
                _ => {}
            }
        }

        // ---- constructor and trailing static block ----------------------------
        let mut tail = String::new();
        if !pending_instance.is_empty() {
            let statements = pending_instance.join(", ");
            match constructor {
                Some(shell) => {
                    let insertion = shell
                        .constructor
                        .as_ref()
                        .and_then(|constructor| constructor.prologue_insertion)
                        .ok_or_else(|| {
                            unsupported(
                                &shell.span,
                                "a constructor of a decorated class needs its `super(...)` call as a top-level statement",
                            )
                        })?;
                    self.push(
                        insertion.offset,
                        insertion.offset,
                        format!(" {statements};"),
                    );
                }
                None => {
                    let super_call = if class.extends_name.is_some() {
                        "super(...arguments); "
                    } else {
                        ""
                    };
                    tail.push_str(&format!(" constructor() {{ {super_call}{statements}; }}"));
                }
            }
        }
        let mut trailing: Vec<String> = pending_static;
        if class_decorated {
            trailing.push(format!("{RUN_INITIALIZERS}({class_this}, {class_extra})"));
        }
        let mut leading_extra = String::new();
        if !trailing.is_empty() {
            let statements: String = trailing
                .iter()
                .map(|statement| format!("{statement}; "))
                .collect();
            if has_static_initializers {
                tail.push_str(&format!(" static {{ {statements}}}"));
            } else {
                leading_extra.push_str(&statements);
            }
        }

        // ---- assemble the header and the footer --------------------------------
        let blocks = if class_decorated {
            format!(
                r#"static {{ Object.defineProperty(this, "name", {{ value: {}, configurable: true }}); {class_this} = this; }} static {{ {leading}{leading_extra}}} "#,
                quoted(&class.name)
            )
        } else {
            format!("static {{ {leading}{leading_extra}}} ")
        };
        let extends_clause = class_super
            .as_ref()
            .map(|name| format!(" extends {name}"))
            .unwrap_or_default();
        let opener = if class_decorated {
            format!("var {} = class{extends_clause} {{ ", class.name)
        } else {
            format!("return class {}{extends_clause} {{ ", class.name)
        };
        let header = if expression {
            format!("(() => {{ {lets}{opener}{blocks}")
        } else {
            format!("let {} = (() => {{ {lets}{opener}{blocks}", class.name)
        };
        let header_end = body_open + 1;
        let header = format!("{header}{}", self.newlines(class.span.start, header_end));
        self.push(class.span.start, header_end, header);

        let close = class.body_span.end - 1;
        let export_suffix = if class.exported {
            if let Some((namespace, _)) = namespace_owner(self.module, class) {
                format!(" {namespace}.{0} = {0};", class.name)
            } else if self.options.module_kind == crate::compiler::ModuleKind::CommonJs {
                format!(" exports.{} = {};", class.export_name(), class.name)
            } else if class.default_export {
                format!(" export default {};", class.name)
            } else {
                format!(" export {{ {} }};", class.name)
            }
        } else {
            String::new()
        };
        let terminator = if expression { "" } else { ";" };
        let finish = if class_decorated {
            format!(
                "}}; return {} = {class_this}; }})(){terminator}{export_suffix}",
                class.name
            )
        } else {
            format!("}}; }})(){terminator}{export_suffix}")
        };
        self.push(close, close + 1, format!("{tail} {finish}"));
        Ok(())
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
                    "{RUN_INITIALIZERS}({this_arg}, {}, ",
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
                "{RUN_INITIALIZERS}({this_arg}, {})",
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
            let name = &plan.name;
            let modifier = if plan.is_static { "static " } else { "" };
            let accessors = if plan.private_descriptor() {
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
                            "{modifier}{storage} = {prefix}{}",
                            self.newlines(shell.span.start, start)
                        ),
                    );
                    self.push(
                        end,
                        shell.span.end,
                        format!(
                            "{suffix}; {accessors}{}",
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
                            "{modifier}{storage} = {value}; {accessors}{}",
                            self.newlines(shell.span.start, shell.span.end)
                        ),
                    );
                }
            }
            return Ok(());
        }
        let _ = class;
        match initializer_range {
            Some((start, end)) if wrapped => {
                self.push(start, start, prefix);
                self.push(end, end, suffix);
            }
            None if wrapped => {
                self.push(
                    field.name_span.end,
                    field.name_span.end,
                    format!(" = {prefix}void 0{suffix}"),
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
