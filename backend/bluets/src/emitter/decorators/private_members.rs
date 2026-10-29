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
        let function = relocated(parameters.start, plan.shell.span.end);
        let descriptor = plan.descriptor_var();
        let prefix = if plan.is_static { "static " } else { "" };
        let replacement = match plan.kind {
            Kind::Method => format!(
                "{prefix}get {}() {{ return {descriptor}.value; }}",
                plan.name
            ),
            Kind::Getter => format!(
                "{prefix}get {}() {{ return {descriptor}.get.call(this); }}",
                plan.name
            ),
            Kind::Setter => format!(
                "{prefix}set {}(value) {{ {descriptor}.set.call(this, value); }}",
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
        let async_prefix = if tokens
            .iter()
            .take_while(|token| token.start < key_end)
            .any(|token| token.is("async"))
        {
            "async "
        } else {
            ""
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
        let label = match plan.kind {
            Kind::Getter => format!("get {}", plan.name),
            Kind::Setter => format!("set {}", plan.name),
            _ => plan.name.clone(),
        };
        // The target passes retain named callables. The public private name is
        // assigned below independently of this collision-free internal binding.
        let callable = self.names.unique("_privateCallable");
        Ok(format!("{descriptor} = {{ {key}: Object.defineProperty({async_prefix}function{generator} {callable}{function}, \"name\", {{ value: {}, configurable: true }}) }}; ", quoted(&label)))
    }
}
