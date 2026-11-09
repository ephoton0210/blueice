// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class-level decorator evaluation, initialization and publication.

use super::*;

impl Lowerer<'_, '_> {
    pub(super) fn class(
        &mut self,
        class: &ClassDeclaration,
        expression: bool,
        label: Option<&str>,
    ) -> Result<(), Diagnostic> {
        let plans = self.plans(class)?;
        // ESNext assign mode keeps auto-accessor proposal syntax. Preserve the
        // native class AST's helper bindings through local, collision-free
        // aliases to the same versioned BlueTS helper implementations.
        let aliases = if self.options.target == EcmaTarget::EsNext
            && plans
                .iter()
                .any(|plan| plan.kind == Kind::Accessor && !plan.private)
        {
            self.run_initializers = self.names.unique("__runInitializers");
            self.es_decorate = self.names.unique("__esDecorate");
            format!(
                "const {} = {RUN_INITIALIZERS}, {} = {ES_DECORATE}; ",
                self.run_initializers, self.es_decorate
            )
        } else {
            String::new()
        };
        let run_initializers = self.run_initializers.clone();
        let es_decorate = self.es_decorate.clone();
        let class_home = plans
            .iter()
            .any(|plan| {
                plan.private_callable()
                    && class.body[plan.shell.token_start..plan.shell.token_end]
                        .iter()
                        .any(|token| token.is("super"))
            })
            .then(|| self.names.unique("_classHome"));
        let name_anchor = if expression {
            String::new()
        } else {
            super::super::relocations::source_anchor(class.name_span.start)
        };
        let name_end_anchor = if expression {
            String::new()
        } else {
            super::super::relocations::source_anchor(class.name_span.end)
        };
        let unmapped = super::super::relocations::unmapped();
        let class_decorated = !class.decorators.is_empty();
        let private_derived = !class_decorated && plans.iter().any(Plan::private_callable);
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
            self.rewrite_static_super(class, base, &class_this, class_decorated)?;
        }
        if class_decorated
            && class
                .members
                .iter()
                .all(|member| member.decorators.is_empty())
        {
            super_members::retain_static_field_values(
                self.module,
                class,
                self.options.defines_class_fields(),
                self.options.target,
                self.edits,
            );
        }

        // ---- the header --------------------------------------------------
        let mut lets = aliases;
        if let Some(home) = &class_home {
            lets.push_str(&format!("let {home}; "));
        }
        if class_decorated {
            let decorators = class
                .decorators
                .iter()
                .map(|decorator| self.decorator_expression(decorator))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            lets.push_str(&format!(
                "let {class_decorators} = [{decorators}]; {}let {class_descriptor}; let {class_extra} = []; let {class_this}; ",
                super::super::relocations::unmapped()
            ));
        }
        if let (Some(name), Some(span)) = (&class_super, &extends) {
            lets.push_str(&format!(
                "let {name} = {}; {}",
                relocated(span.start, span.end),
                super::super::relocations::unmapped()
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
        if let Some(home) = &class_home {
            leading.push_str(&format!("{home} = this; "));
        }
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
                leading.push_str(&format!(
                    "{} = [{decorators}]; {}",
                    plan.decorators_var(),
                    super::super::relocations::unmapped()
                ));
            }
            if plan.private_descriptor() {
                if plan.kind == Kind::Accessor {
                    leading.push_str(&plan.private_accessor_descriptor());
                } else {
                    leading.push_str(&self.private_callable(class, plan, class_home.as_deref())?);
                    leading.push_str(super::super::relocations::unmapped());
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
            let mut access = Self::access(plan);
            if plan.private_callable() && self.options.target >= EcmaTarget::Es2022 {
                let key_start = super::super::relocations::source_anchor(
                    plan.shell.key.first().expect("private callable key").start,
                );
                let key_end = super::super::relocations::source_anchor(
                    plan.shell.key.last().expect("private callable key").end,
                );
                access = access.replace(&plan.name, &format!("{key_start}{}{key_end}", plan.name));
            }
            if plan.kind == Kind::Method
                && !plan.private
                && plan.runtime_key.is_none()
                && !plan.literal()
            {
                let end = plan.shell.key.last().expect("method key").end;
                let closing = access
                    .strip_suffix(" }")
                    .expect("access getter and closing brace")
                    .len();
                access.insert_str(closing, &super::super::relocations::source_anchor(end));
            }
            let context = format!(
                "{{ kind: {}, name: {name}, static: {}, private: {}, access: {}, metadata: {metadata} }}",
                quoted(plan.kind.label()),
                plan.is_static,
                plan.private,
                access
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
            let origin = if plan.private_callable() {
                member_origins::logical_member_start(class, plan.shell)
            } else {
                member_origins::member_header(class, plan)
            };
            let anchor = super::super::relocations::source_anchor(origin);
            let completion = super::super::relocations::source_anchor(plan.shell.span.end);
            let descriptor_completion = if plan.private_callable() {
                completion.clone()
            } else {
                String::new()
            };
            format!(
                "{anchor}{es_decorate}({target}, {descriptor}{descriptor_completion}, {}, {context}, {initializers}, {extra});{completion} {}",
                plan.decorators_var(),
                super::super::relocations::unmapped()
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
            let anchor = super::super::relocations::source_anchor(member_origins::class_header(
                class,
                &self.module.source,
                expression,
            ));
            let completion = super::super::relocations::source_anchor(class.span.end);
            leading.push_str(&format!(
                "{anchor}{es_decorate}(null, {class_descriptor} = {{ value: {class_this} }}, {class_decorators}, {{ kind: \"class\", name: {class_this}.name, metadata: {metadata} }}, null, {class_extra});{completion} {}{} = {class_this} = {class_descriptor}.value; ",
                super::super::relocations::unmapped(),
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
            let anchor = if private_derived {
                name_anchor.clone()
            } else {
                String::new()
            };
            let completion = if private_derived {
                name_end_anchor.clone()
            } else {
                String::new()
            };
            pending_static.push(format!(
                "{anchor}{run_initializers}({this_arg_static}, {name}){completion}"
            ));
        }
        if let Some(name) = &instance_extra {
            pending_instance.push(format!("{run_initializers}(this, {name})"));
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
                    let mut entry = name_anchor.clone();
                    let mut completion = name_end_anchor.clone();
                    let field_entry = plans
                        .iter()
                        .rev()
                        .find(|plan| {
                            !plan.is_static && matches!(plan.kind, Kind::Field | Kind::Accessor)
                        })
                        .is_some_and(Plan::decorated);
                    if field_entry {
                        entry = unmapped.to_string();
                        completion.clear();
                    }
                    let methods_only = class.members.iter().all(|member| member.field.is_none());
                    let method_entry = expression && methods_only;
                    if method_entry {
                        let anonymous = self
                            .module
                            .class_expressions
                            .get(&class.span.start)
                            .is_some_and(|original| original.name.is_none());
                        entry = super::super::relocations::source_anchor(if anonymous {
                            class_keyword_start(class, &self.module.source)
                        } else {
                            class.name_span.start
                        });
                        completion = super::super::relocations::source_anchor(if anonymous {
                            class.span.end
                        } else {
                            class.name_span.end
                        });
                    }
                    let close = if methods_only {
                        format!(" {unmapped}}}")
                    } else {
                        format!("{unmapped} }}")
                    };
                    tail.push_str(&format!(
                        " constructor() {{ {super_call}{entry}{statements}{completion};{close}"
                    ));
                }
            }
        }
        let mut trailing = pending_static
            .into_iter()
            .map(|statement| (statement, None))
            .collect::<Vec<_>>();
        if class_decorated {
            let anonymous = expression
                && self
                    .module
                    .class_expressions
                    .get(&class.span.start)
                    .is_some_and(|original| original.name.is_none());
            let start = if anonymous {
                class_keyword_start(class, &self.module.source)
            } else {
                class.name_span.start
            };
            let end = if anonymous {
                class.span.end
            } else {
                class.name_span.end
            };
            trailing.push((
                format!(
                    "{}{run_initializers}({class_this}, {class_extra})",
                    super::super::relocations::source_anchor(start)
                ),
                Some(end),
            ));
        }
        let mut leading_extra = String::new();
        if !trailing.is_empty() {
            let statements: String = trailing
                .iter()
                .map(|(statement, completion)| {
                    format!(
                        "{statement};{} ",
                        completion
                            .map(super::super::relocations::source_anchor)
                            .unwrap_or_default()
                    )
                })
                .collect();
            if has_static_initializers {
                tail.push_str(&format!(" static {{ {statements}{unmapped}}}"));
            } else {
                leading_extra.push_str(&format!("{statements}{unmapped}"));
            }
        }

        // ---- assemble the header and the footer --------------------------------
        let blocks = if class_decorated {
            let lexical_binding = if expression && self.options.target < EcmaTarget::Es2022 {
                // The header's exact `var name = class` binding must already
                // hold the initial constructor when moved decorators execute.
                format!("{} = this; ", class.name)
            } else {
                String::new()
            };
            format!(
                r#"static {{ {lexical_binding}Object.defineProperty(this, "name", {{ value: {}, configurable: true }}); {class_this} = this; }} static {{ {leading}{leading_extra}}} "#,
                label.map_or_else(|| quoted(&class.name), str::to_string)
            )
        } else {
            format!("static {{ {leading}{leading_extra}}} ")
        };
        let heritage_keyword = if class_decorated || private_derived {
            extends.as_ref().and_then(|span| {
                crate::lex(&self.module.id, &self.module.source)
                    .ok()
                    .and_then(|tokens| {
                        tokens
                            .into_iter()
                            .rfind(|token| {
                                token.is("extends")
                                    && class.span.start <= token.start
                                    && token.end <= span.start
                            })
                            .map(|token| token.start)
                    })
            })
        } else {
            None
        };
        let heritage_prefix = heritage_keyword
            .map(super::super::relocations::source_anchor)
            .unwrap_or_default();
        let extends_clause = class_super
            .as_ref()
            .map(|name| {
                if let (Some(keyword), Some(span)) = (heritage_keyword, &extends) {
                    format!(
                        " {}extends {}{name}{}",
                        super::super::relocations::source_anchor(keyword),
                        super::super::relocations::source_anchor(span.start),
                        super::super::relocations::source_anchor(span.end)
                    )
                } else {
                    format!(" extends {name}")
                }
            })
            .unwrap_or_default();
        let core_name_end_anchor = if private_derived {
            heritage_keyword
                .map(super::super::relocations::source_anchor)
                .unwrap_or_else(|| name_end_anchor.clone())
        } else {
            name_end_anchor.clone()
        };
        let opener = if class_decorated {
            format!(
                "var {} = class{heritage_prefix}{extends_clause} {{ {}",
                class.name,
                if heritage_keyword.is_some() {
                    unmapped
                } else {
                    ""
                }
            )
        } else {
            format!(
                "return class {name_anchor}{}{core_name_end_anchor}{extends_clause} {{ {unmapped}",
                class.name
            )
        };
        let plain_declaration = !expression && !class_decorated;
        let header = if expression {
            let prefix_origin = self
                .module
                .class_expressions
                .get(&class.span.start)
                .and_then(|original| {
                    crate::lex(&self.module.id, &self.module.source)
                        .ok()
                        .and_then(|tokens| {
                            super::super::class_lowering::expressions::names::evaluation_prefix_origin(
                                original, &tokens,
                            )
                        })
                })
                .map(super::super::relocations::source_anchor)
                .unwrap_or_default();
            format!("{prefix_origin}(() => {{ {unmapped}{lets}{opener}{blocks}")
        } else if class_decorated && self.options.target == EcmaTarget::Es5 {
            let start = crate::lex(
                &self.module.id,
                &self.module.source[class.span.start..class.name_span.start],
            )
            .ok()
            .and_then(|tokens| {
                tokens.into_iter().find(|token| {
                    !class.decorators.iter().any(|decorator| {
                        decorator.span.start <= class.span.start + token.start
                            && class.span.start + token.end <= decorator.span.end
                    })
                })
            })
            .map_or(class.span.start, |token| class.span.start + token.start);
            format!(
                "{}let {} = (() => {{ {unmapped}{lets}{opener}{blocks}",
                super::super::relocations::source_anchor(start),
                class.name
            )
        } else if plain_declaration && self.options.target == EcmaTarget::Es5 {
            format!(
                "{}let {} = (() => {{ {unmapped}{lets}{opener}{blocks}",
                if self.wrapper == crate::ModuleKind::System {
                    unmapped.to_string()
                } else {
                    super::super::relocations::source_anchor(class.span.start)
                },
                class.name
            )
        } else {
            format!(
                "let {name_anchor}{}{name_end_anchor} = (() => {{ {unmapped}{lets}{opener}{blocks}",
                class.name
            )
        };
        let header_end = body_open + 1;
        let header = format!(
            "{}{header}{}{}",
            super::super::relocations::unmapped(),
            self.newlines(class.span.start, header_end),
            if expression {
                super::super::relocations::unmapped()
            } else {
                ""
            }
        );
        let header = if !expression {
            format!("{header}{unmapped}")
        } else {
            header
        };
        self.push(class.span.start, header_end, header);

        let close = class.body_span.end - 1;
        let export_suffix = if class.exported {
            if let Some((namespace, _)) = namespace_owner(self.module, class) {
                format!(" {namespace}.{0} = {0};", class.name)
            } else if self.options.module_kind == crate::compiler::ModuleKind::CommonJs {
                if !expression && self.wrapper != crate::ModuleKind::System {
                    format!(
                        " {name_anchor}exports.{} = {};{name_end_anchor}",
                        class.export_name(),
                        class.name
                    )
                } else {
                    format!(
                        " {unmapped}exports.{} = {};{unmapped}",
                        class.export_name(),
                        class.name
                    )
                }
            } else if class.default_export {
                format!(" export default {};", class.name)
            } else if !expression {
                format!(
                    " {unmapped}export {{ {name_anchor}{}{name_end_anchor} }};{}",
                    class.name,
                    if class_decorated {
                        format!("{name_end_anchor} {unmapped}")
                    } else {
                        String::new()
                    }
                )
            } else {
                format!(" export {{ {} }};", class.name)
            }
        } else {
            String::new()
        };
        let terminator = if expression { "" } else { ";" };
        let binding_completion = if (plain_declaration || class_decorated && !expression)
            && self.options.target == EcmaTarget::Es5
            && self.wrapper != crate::ModuleKind::System
        {
            super::super::relocations::source_anchor(class.span.end)
        } else {
            String::new()
        };
        let lowered_class_completion = plain_declaration
            && self.options.target == EcmaTarget::Es5
            && class.members.iter().any(|member| member.field.is_some());
        let class_completion = if lowered_class_completion {
            super::super::relocations::source_anchor(class.span.end)
        } else {
            String::new()
        };
        let class_gap_end = if lowered_class_completion {
            unmapped
        } else {
            ""
        };
        let method_value_completion = plain_declaration
            && self.options.target == EcmaTarget::Es5
            && class.members.iter().all(|member| member.field.is_none());
        let (value_completion, factory_gap) = if method_value_completion {
            (
                format!(
                    "{} {unmapped}",
                    super::super::relocations::source_anchor(class.span.end)
                ),
                "",
            )
        } else {
            (String::new(), " ")
        };
        let binding_carrier = if class_decorated && !expression {
            if self.options.target == EcmaTarget::Es5 {
                format!(" {unmapped}")
            } else if self.options.target >= EcmaTarget::Es2022
                && !class.members.is_empty()
                && class.members.iter().all(|member| member.method.is_some())
            {
                unmapped.to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let finish = if class_decorated {
            format!(
                "}}; return {} = {class_this}; }})(){terminator}{binding_completion}{binding_carrier}{export_suffix}",
                class.name
            )
        } else {
            format!("}}{value_completion};{class_completion}{factory_gap}{class_gap_end}}})(){terminator}{binding_completion}{export_suffix}")
        };
        let mut finish_end = close + 1;
        if plain_declaration {
            while self
                .module
                .source
                .as_bytes()
                .get(finish_end)
                .is_some_and(u8::is_ascii_whitespace)
            {
                finish_end += 1;
            }
        }
        self.push(
            close,
            finish_end,
            format!(
                "{}{tail} {finish}{}",
                super::super::relocations::unmapped(),
                if expression
                    && expressions::unmapped_continuation(self.module, class, self.options)
                {
                    super::super::relocations::unmapped()
                } else {
                    &self.module.source[close + 1..finish_end]
                }
            ),
        );
        Ok(())
    }
}
