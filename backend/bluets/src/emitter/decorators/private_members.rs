// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorated private callables use lexical descriptors while retaining class brands.

use super::*;

impl Plan<'_> {
    pub(super) fn private_callable(&self) -> bool {
        self.private
            && self.decorated()
            && matches!(self.kind, Kind::Method | Kind::Getter | Kind::Setter)
    }

    pub(super) fn private_descriptor(&self) -> bool {
        self.private_callable() || (self.private && self.decorated() && self.kind == Kind::Accessor)
    }

    pub(super) fn private_accessor_descriptor(&self) -> String {
        let descriptor = self.descriptor_var();
        let storage = self.storage();
        format!("{descriptor} = {{ get: function () {{ return this.{storage}; }}, set: function (value) {{ this.{storage} = value; }} }}; ")
    }

    pub(super) fn descriptor_var(&self) -> String {
        format!("{}_descriptor", self.var)
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn private_callable(
        &mut self,
        class: &ClassDeclaration,
        plan: &Plan,
        home: Option<&str>,
    ) -> Result<String, Diagnostic> {
        let tokens = &class.body[plan.shell.token_start..plan.shell.token_end];
        let key_end = plan.shell.key.last().expect("a callable has a key").end;
        let parameters = tokens
            .iter()
            .find(|token| token.start >= key_end && token.is("("))
            .expect("a structured callable has parameters");
        if let Some(home) = home {
            let parameter_index = tokens
                .iter()
                .position(|token| token.start == parameters.start)
                .unwrap();
            let parameter_end =
                super_members::close(tokens, parameter_index).expect("retained parameter list");
            let return_end = plan
                .shell
                .method
                .as_ref()
                .and_then(|method| method.return_type_span.as_ref())
                .or_else(|| {
                    plan.shell
                        .accessor
                        .as_ref()
                        .and_then(|accessor| accessor.return_type_span.as_ref())
                })
                .map_or(tokens[parameter_end].end, |span| span.end);
            let body = tokens
                .iter()
                .position(|token| token.start >= return_end && token.is("{"))
                .expect("private callable body");
            if tokens[parameter_index..body]
                .iter()
                .any(|token| token.is("super"))
            {
                return Err(unsupported(&plan.shell.span, "super in a private callable parameter initializer needs its retained home scope"));
            }
            let base = if plan.is_static {
                format!("Object.getPrototypeOf({home})")
            } else {
                format!("Object.getPrototypeOf({home}.prototype)")
            };
            super_members::lower_references(
                self.module,
                &tokens[body + 1..tokens.len() - 1],
                &base,
                "this",
                false,
                &mut self.names.used,
                self.edits,
            )?;
        }
        let header = super::member_origins::logical_member_start(class, plan.shell);
        let anchor = super::super::relocations::source_anchor(header);
        let descriptor_anchor = anchor.clone();
        let no_parameters = plan
            .shell
            .method
            .as_ref()
            .map(|method| &method.parameters)
            .or_else(|| {
                plan.shell
                    .accessor
                    .as_ref()
                    .map(|accessor| &accessor.parameters)
            })
            .is_some_and(|parameters| parameters.iter().all(|parameter| parameter.name == "this"));
        let descriptor = plan.descriptor_var();
        let retained_private = self.options.target >= EcmaTarget::Es2022;
        let origin = |position| {
            if retained_private {
                super::super::relocations::source_anchor(position)
            } else {
                String::new()
            }
        };
        let prefix = if plan.is_static {
            let token = tokens
                .iter()
                .find(|token| token.start < key_end && token.is("static"))
                .expect("a static private callable has a modifier");
            format!("{}static{} ", origin(token.start), origin(token.end))
        } else {
            origin(super::member_origins::logical_member_start(
                class, plan.shell,
            ))
        };
        let key_origin = origin(plan.shell.key.first().expect("a callable has a key").start);
        let key_end_origin = origin(key_end);
        let completion = origin(plan.shell.span.end);
        let replacement = match plan.kind {
            Kind::Method => format!(
                "{prefix}get {key_origin}{}{key_end_origin}() {{ return {descriptor}.value; }}{completion}",
                plan.name
            ),
            Kind::Getter => format!(
                "{prefix}get {key_origin}{}{key_end_origin}() {{ return {descriptor}.get.call(this); }}{completion}",
                plan.name
            ),
            Kind::Setter => format!(
                "{prefix}set {key_origin}{}{key_end_origin}(value) {{ {descriptor}.set.call(this, value); }}{completion}",
                plan.name
            ),
            _ => unreachable!(),
        };
        self.push(plan.shell.span.start, plan.shell.span.end, replacement);
        let key = match plan.kind {
            Kind::Getter => "get",
            Kind::Setter => "set",
            _ => "value",
        };
        let asynchronous = plan
            .shell
            .method
            .as_ref()
            .filter(|method| method.async_function)
            .and_then(|_| {
                tokens.iter().find(|token| {
                    header <= token.start && token.start < key_end && token.is("async")
                })
            });
        let retained_async_end = asynchronous
            .filter(|_| self.options.target >= EcmaTarget::Es2017)
            .map(|token| token.end);
        let async_prefix = if let Some(token) = asynchronous {
            if retained_async_end.is_some() {
                format!(
                    "{}async{} ",
                    super::super::relocations::source_anchor(token.start),
                    super::super::relocations::source_anchor(token.end)
                )
            } else {
                "async ".to_string()
            }
        } else {
            String::new()
        };
        let generator = if tokens
            .iter()
            .take_while(|token| token.start < key_end)
            .any(|token| token.is("*"))
        {
            "*"
        } else {
            ""
        };
        let parameter_anchor = super::super::relocations::source_anchor(if generator.is_empty() {
            retained_async_end.unwrap_or(header)
        } else {
            plan.shell.key.first().expect("a callable has a key").start
        });
        let after_generator = if generator.is_empty() {
            String::new()
        } else {
            parameter_anchor.clone()
        };
        let function = if no_parameters {
            let parameter_index = tokens
                .iter()
                .position(|token| token.start == parameters.start)
                .unwrap();
            let parameter_end =
                super_members::close(tokens, parameter_index).expect("retained parameter list");
            let return_end = plan
                .shell
                .method
                .as_ref()
                .and_then(|method| method.return_type_span.as_ref())
                .or_else(|| {
                    plan.shell
                        .accessor
                        .as_ref()
                        .and_then(|accessor| accessor.return_type_span.as_ref())
                })
                .map_or(tokens[parameter_end].end, |span| span.end);
            let body = tokens
                .iter()
                .find(|token| token.start >= return_end && token.is("{"))
                .expect("private callable body");
            let header_text = &self.module.source[parameters.start..body.end];
            if header_text.contains("/*") || header_text.contains("//") {
                relocated(parameters.end, plan.shell.span.end)
            } else {
                format!(
                    "){parameter_anchor}{{ {}",
                    relocated(body.end, plan.shell.span.end)
                )
            }
        } else {
            relocated(parameters.end, plan.shell.span.end)
        };
        let label = match plan.kind {
            Kind::Getter => format!("get {}", plan.name),
            Kind::Setter => format!("set {}", plan.name),
            _ => plan.name.clone(),
        };
        // The target passes retain named callables. The public private name is
        // assigned below independently of this collision-free internal binding.
        let callable = self.names.unique("_privateCallable");
        let generated = super::super::relocations::unmapped();
        Ok(format!("{descriptor_anchor}{descriptor} = {{ {key}: {anchor}Object.defineProperty({async_prefix}function{generator}{after_generator} {generated}{callable}{parameter_anchor}({function}, \"name\", {{ value: {}, configurable: true }}) }}; ", quoted(&label)))
    }
}
