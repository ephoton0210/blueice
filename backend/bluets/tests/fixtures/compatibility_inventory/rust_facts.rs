// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rust harness facts used by the source-derived compatibility inventory.

use std::collections::{BTreeMap, BTreeSet};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, Lit, Member, Token};

#[derive(Debug, Default, serde::Serialize)]
pub struct Facts {
    pub ignored_tests: Vec<String>,
    pub includes: Vec<String>,
    pub arrays: Vec<Array>,
    pub modules: Vec<Module>,
}

#[derive(Debug, serde::Serialize)]
pub struct Module {
    pub name: String,
    pub scope: String,
    pub path: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct Array {
    pub scope: String,
    pub binding: String,
    pub labels: Vec<String>,
}

pub fn parse(source: &str) -> Result<Facts, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut collector = Collector::default();
    collector.visit_file(&file);
    collector.facts.includes = collector.includes.into_iter().collect();
    Ok(collector.facts)
}

#[derive(Default)]
struct Collector {
    facts: Facts,
    scope: Vec<String>,
    function_depth: usize,
    loops: BTreeMap<String, usize>,
    includes: BTreeSet<String>,
}

impl Collector {
    fn scope(&self) -> String {
        self.scope.join("::")
    }

    fn array(&mut self, binding: String, expression: &Expr) {
        let Some(labels) = array_labels(expression) else {
            return;
        };
        self.facts.arrays.push(Array {
            scope: self.scope(),
            binding,
            labels,
        });
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if node.content.is_none() {
            let path = node.attrs.iter().find_map(|attribute| {
                if !attribute.path().is_ident("path") {
                    return None;
                }
                match &attribute.meta {
                    syn::Meta::NameValue(value) => label(&value.value),
                    _ => None,
                }
            });
            self.facts.modules.push(Module {
                name: node.ident.to_string(),
                scope: self.scope(),
                path,
            });
        }
        self.scope.push(node.ident.to_string());
        visit::visit_item_mod(self, node);
        self.scope.pop();
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.scope.push(node.sig.ident.to_string());
        let has = |name| node.attrs.iter().any(|attr| attr.path().is_ident(name));
        if self.function_depth == 0 && has("test") && has("ignore") {
            self.facts.ignored_tests.push(self.scope());
        }
        self.function_depth += 1;
        visit::visit_item_fn(self, node);
        self.function_depth -= 1;
        self.scope.pop();
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.array(node.ident.to_string(), &node.expr);
        visit::visit_item_const(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        let pattern = match &node.pat {
            syn::Pat::Type(typed) => typed.pat.as_ref(),
            pattern => pattern,
        };
        if let (syn::Pat::Ident(binding), Some(value)) = (pattern, &node.init) {
            self.array(binding.ident.to_string(), &value.expr);
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        let next = self.loops.entry(self.scope()).or_default();
        let binding = format!("for@{next}");
        *next += 1;
        self.array(binding, &node.expr);
        visit::visit_expr_for_loop(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if node.path.is_ident("include_str") || node.path.is_ident("include_bytes") {
            if let Ok(path) = syn::parse2::<syn::LitStr>(node.tokens.clone()) {
                self.includes.insert(path.value());
            }
        }
        self.nested_includes(node.tokens.clone());
        // A macro's token stream is not traversed as Rust expressions by syn.
        if node.path.is_ident("vec") {
            if let Some(values) = vec_values(node) {
                for value in &values {
                    self.visit_expr(value);
                }
            }
        }
        visit::visit_macro(self, node);
    }
}

impl Collector {
    fn nested_includes(&mut self, tokens: proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        let tokens: Vec<_> = tokens.into_iter().collect();
        for window in tokens.windows(3) {
            if let [TokenTree::Ident(name), TokenTree::Punct(bang), TokenTree::Group(arguments)] =
                window
            {
                if matches!(name.to_string().as_str(), "include_str" | "include_bytes")
                    && bang.as_char() == '!'
                {
                    if let Ok(path) = syn::parse2::<syn::LitStr>(arguments.stream()) {
                        self.includes.insert(path.value());
                    }
                }
            }
        }
        for token in tokens {
            if let TokenTree::Group(group) = token {
                self.nested_includes(group.stream());
            }
        }
    }
}

fn unwrapped(expression: &Expr) -> &Expr {
    match expression {
        Expr::Reference(reference) => unwrapped(&reference.expr),
        Expr::Paren(parentheses) => unwrapped(&parentheses.expr),
        Expr::Group(group) => unwrapped(&group.expr),
        expression => expression,
    }
}

fn vec_values(mac: &syn::Macro) -> Option<Punctuated<Expr, Token![,]>> {
    Punctuated::<Expr, Token![,]>::parse_terminated
        .parse2(mac.tokens.clone())
        .ok()
}

fn array_labels(expression: &Expr) -> Option<Vec<String>> {
    let labels = |values: &Punctuated<Expr, Token![,]>| {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| label(value).unwrap_or_else(|| format!("#{index}")))
            .collect()
    };
    match unwrapped(expression) {
        Expr::Array(array) => Some(labels(&array.elems)),
        Expr::Macro(mac) if mac.mac.path.is_ident("vec") => {
            vec_values(&mac.mac).map(|values| labels(&values))
        }
        _ => None,
    }
}

fn label(expression: &Expr) -> Option<String> {
    match unwrapped(expression) {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(value) => Some(value.value()),
            Lit::Bool(value) => Some(value.value.to_string()),
            Lit::Int(value) => Some(value.base10_digits().to_string()),
            _ => None,
        },
        Expr::Tuple(tuple) => tuple.elems.first().and_then(label),
        Expr::Struct(record) => record.fields.iter().find_map(|field| match &field.member {
            Member::Named(name) if matches!(name.to_string().as_str(), "name" | "id" | "label") => {
                label(&field.expr)
            }
            _ => None,
        }),
        Expr::Path(path) => Some(
            path.path
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        ),
        _ => None,
    }
}
