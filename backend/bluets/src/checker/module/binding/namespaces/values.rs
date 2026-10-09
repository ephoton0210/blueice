// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked module values reuse the namespace surface's private-type identities.

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ExportedValue {
    surface: NamespaceExport,
    /// An inferred variable in a module that has not completed checking.
    pending: bool,
    readonly: bool,
}

impl ExportedValue {
    pub(in crate::checker) fn value_type(&self) -> Type {
        let name = &self.surface.source_name;
        if let Some(signatures) = self.surface.functions.get(name) {
            let mut types: Vec<_> = signatures
                .iter()
                .map(|signature| Type::Function {
                    parameters: signature.parameters.clone(),
                    result: Box::new(signature.return_type.clone()),
                })
                .collect();
            return if types.len() == 1 {
                types.pop().expect("one function signature")
            } else {
                Type::Intersection(types)
            };
        }
        self.surface
            .values
            .get(name)
            .cloned()
            .unwrap_or(Type::Unknown)
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker) fn exported_values(
        &self,
        seed: bool,
    ) -> BTreeMap<String, ExportedValue> {
        if let Some(schema) = self.project.json_modules.get(&self.module.id) {
            let mut values = BTreeMap::new();
            if let Type::Record(fields) = schema {
                values.extend(
                    fields
                        .iter()
                        .map(|field| (field.name.clone(), field.value.clone())),
                );
            }
            values.insert("default".to_string(), schema.clone());
            return values
                .into_iter()
                .map(|(name, value)| {
                    let mut surface = self.namespace_export_of(&name);
                    surface.values.insert(name.clone(), value);
                    surface.has_values = true;
                    (
                        name,
                        ExportedValue {
                            surface,
                            pending: false,
                            readonly: false,
                        },
                    )
                })
                .collect();
        }
        let mut names = BTreeMap::new();
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Variable(item) if item.exported => {
                    names.insert(item.name.clone(), item.name.clone());
                }
                Declaration::Function(item) if item.exported => {
                    names.insert(
                        if item.default_export {
                            "default"
                        } else {
                            &item.name
                        }
                        .to_string(),
                        item.name.clone(),
                    );
                }
                Declaration::Class(item) if item.exported => {
                    names.insert(item.export_name().to_string(), item.name.clone());
                }
                Declaration::Enum(item) if item.exported => {
                    names.insert(item.name.clone(), item.name.clone());
                }
                Declaration::Namespace(item) if item.exported => {
                    names.insert(item.name.clone(), item.name.clone());
                }
                Declaration::DefaultExport(item) => {
                    names.insert("default".to_string(), item.name.clone());
                }
                Declaration::ValueExport(item) if item.specifier.is_none() => {
                    for binding in &item.bindings {
                        names.insert(binding.exported.clone(), binding.local.clone());
                    }
                }
                _ => {}
            }
        }
        names
            .into_iter()
            .map(|(exported, source)| {
                let pending = seed
                    && self.module.declarations.iter().any(|declaration| {
                        matches!(declaration, Declaration::Variable(variable)
                    if variable.name == source && variable.annotation.is_none()
                        && self.values.get(&source) == Some(&Type::Unknown))
                    });
                (
                    exported,
                    ExportedValue {
                        surface: self.namespace_export_of(&source),
                        pending,
                        readonly: self.module.declarations.iter().any(|declaration| {
                            matches!(declaration, Declaration::Variable(variable)
                                if variable.name == source
                                    && variable.kind == crate::parser::VariableKind::Const)
                        }),
                    },
                )
            })
            .collect()
    }

    pub(in crate::checker::module) fn bind_imported_value(
        &mut self,
        local: &str,
        value: &ExportedValue,
        span: &SourceSpan,
    ) {
        self.bind_imported_surface(local, &value.surface, span, true, false, false);
        if value.pending {
            self.pending_imports.insert(local.to_string());
        }
        let source = &value.surface.source_name;
        let rename: BTreeMap<_, _> = value
            .surface
            .types
            .keys()
            .filter_map(|name| {
                let bare = name.strip_prefix("typeof ").unwrap_or(name);
                let tail = bare.strip_prefix(source)?;
                (tail.is_empty() || tail.starts_with('.')).then(|| {
                    let prefix = if name.starts_with("typeof ") {
                        "typeof "
                    } else {
                        ""
                    };
                    (name.clone(), format!("{prefix}{local}{tail}"))
                })
            })
            .collect();
        self.symbols.push(Symbol {
            name: local.to_string(),
            kind: SymbolKind::Import,
            module: self.module.id.clone(),
            span: span.clone(),
            exported: false,
            value_type: Some(
                Qualifier {
                    rename: &rename,
                    argument_prefix: None,
                }
                .ty(&value.value_type()),
            ),
        });
    }

    pub(in crate::checker::module) fn bind_module_namespace(
        &mut self,
        local: &str,
        resolved: &str,
        span: &SourceSpan,
        readonly: bool,
    ) {
        let values = self
            .exports
            .values
            .get(resolved)
            .cloned()
            .unwrap_or_default();
        if let Some(types) = self.exports.types.get(resolved).cloned() {
            for (name, mut definition) in types {
                let qualified = format!("{local}.{name}");
                if let Some(class) = self
                    .exports
                    .classes
                    .get(resolved)
                    .and_then(|classes| classes.get(&name))
                {
                    definition.value = super::super::modules::specialize_imported_class_type(
                        &class.instance_type,
                        &class.source_name,
                        &qualified,
                    );
                }
                if !self.types.contains_key(&qualified) {
                    self.insert_type(
                        &qualified,
                        definition,
                        span.clone(),
                        SymbolKind::Import,
                        false,
                    );
                }
            }
        }
        let mut fields = Vec::new();
        for (name, value) in values {
            if name == "export=" {
                continue;
            }
            let qualified = format!("{local}.{name}");
            if !self.values.contains_key(&qualified) {
                self.bind_imported_value(&qualified, &value, span);
            }
            let field_type = self
                .symbols
                .iter()
                .rev()
                .find(|symbol| symbol.name == qualified)
                .and_then(|symbol| symbol.value_type.clone())
                .unwrap_or_else(|| {
                    self.values
                        .get(&qualified)
                        .cloned()
                        .unwrap_or(Type::Unknown)
                });
            fields.push(TypeField {
                accessor_write_type: None,
                method: false,
                span: span.clone(),
                name,
                value: field_type,
                readonly: readonly || value.readonly,
                optional: false,
            });
        }
        self.insert_value(
            local,
            Type::Record(fields),
            span.clone(),
            SymbolKind::Import,
            false,
        );
        self.module_namespace_imports.insert(local.to_string());
        self.module_namespace_targets
            .insert(local.to_string(), resolved.to_string());
    }

    pub(in crate::checker::module) fn bind_export_assignment_member(
        &mut self,
        local: &str,
        member: &str,
        resolved: &str,
        span: &SourceSpan,
    ) -> bool {
        let Some(mut value) = self
            .exports
            .values
            .get(resolved)
            .and_then(|values| values.get("export="))
            .cloned()
        else {
            return false;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let member_type = match property_type(
            &value.value_type(),
            member,
            &value.surface.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found { value, .. } => value,
            PropertyType::Exhausted => {
                self.type_error(
                    span,
                    "export-assignment member lookup exceeds the type-expansion limit".to_string(),
                    DiagnosticCode::ResourceLimit,
                );
                return true;
            }
            _ => return false,
        };
        let source = format!("{}.{member}", value.surface.source_name);
        value.surface.values.insert(source.clone(), member_type);
        value.surface.source_name = source;
        self.bind_imported_value(local, &value, span);
        true
    }

    pub(in crate::checker::module) fn infer_variable_type(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        let inferred = self.infer_expression(&variable.initializer, scope);
        if inferred == Type::Unknown
            && variable
                .initializer
                .iter()
                .any(|token| self.pending_imports.contains(&token.text))
        {
            self.type_error(
                &variable.span,
                format!("TS7022: `{}` needs a type annotation because its initializer depends on a circular import", variable.name),
                DiagnosticCode::TypeMismatch,
            );
        }
        if let Some(literal) = crate::parser::widen_literal_tokens(
            &variable.initializer,
            variable.kind == crate::parser::VariableKind::Const,
        ) {
            return literal;
        }
        if variable.kind == crate::parser::VariableKind::Const {
            if matches!(inferred, Type::Symbol | Type::UniqueSymbol(_)) {
                return if self.fresh_symbol_call(&variable.initializer, scope) {
                    Type::UniqueSymbol(variable.span.clone())
                } else {
                    Type::Symbol
                };
            }
            if let [literal] = variable.initializer.as_slice() {
                if matches!(literal.kind, TokenKind::Number | TokenKind::String)
                    || literal.is("true")
                    || literal.is("false")
                {
                    return Type::Literal(literal.text.clone());
                }
            }
            inferred
        } else {
            match inferred {
                Type::UniqueSymbol(_) => Type::Symbol,
                Type::Literal(text) if text.starts_with(['\'', '"', '`']) => Type::String,
                Type::Literal(text) if matches!(text.as_str(), "true" | "false") => Type::Boolean,
                Type::Literal(text) if text.parse::<f64>().is_ok() => Type::Number,
                value => value,
            }
        }
    }

    pub(in crate::checker::module) fn alias_function_signatures(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let [source] = variable.initializer.as_slice() else {
            return None;
        };
        if scope.get(&source.text) != self.values.get(&source.text) {
            return None;
        }
        let signatures = self.functions.get(&source.text)?.clone();
        let signature = signatures.first()?;
        let value = Type::Function {
            parameters: signature.parameters.clone(),
            result: Box::new(signature.return_type.clone()),
        };
        self.functions.insert(variable.name.clone(), signatures);
        Some(value)
    }

    pub(in crate::checker::module) fn refresh_variable_symbol(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        inferred: Type,
    ) {
        if let Some(symbol) = self.symbols.iter_mut().find(|symbol| {
            symbol.kind == SymbolKind::Variable
                && symbol.name == variable.name
                && symbol.span == variable.span
        }) {
            symbol.value_type = Some(inferred);
        }
    }

    /// Qualified module calls retain overloads; a local receiver shadows the import.
    pub(in crate::checker::module) fn module_member_signatures(
        &self,
        receiver: &[Token],
        member: &str,
        scope: &BTreeMap<String, Type>,
    ) -> Option<Vec<FunctionSignature>> {
        let [receiver] = receiver else {
            return None;
        };
        if !self.module_namespace_imports.contains(&receiver.text)
            || scope.get(&receiver.text) != self.values.get(&receiver.text)
        {
            return None;
        }
        self.functions
            .get(&format!("{}.{member}", receiver.text))
            .cloned()
    }
}
