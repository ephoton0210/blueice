// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import and export declarations.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_umd_export(&mut self, start: usize) {
        self.expect("as");
        self.expect("namespace");
        let name = self.require_identifier("expected global module namespace name");
        self.consume(";");
        let span = SourceSpan::new(&self.id, start, self.previous().end);
        let code = if self.namespace_depth > 0 {
            Some(1316)
        } else if !self.id.ends_with(".d.ts") {
            Some(1315)
        } else {
            None
        };
        if let Some(code) = code {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    span,
                    "global module exports require a top-level declaration module",
                )
                .with_typescript(code, Vec::new()),
            );
            return;
        }
        self.declarations
            .push(Declaration::UmdExport(UmdExportDeclaration { name, span }));
    }

    pub(in crate::parser::implementation) fn validate_umd_exports(&mut self) {
        let external = self
            .declarations
            .iter()
            .any(|declaration| match declaration {
                Declaration::Import(_)
                | Declaration::TypeExport(_)
                | Declaration::ValueExport(_)
                | Declaration::DefaultExport(_) => true,
                Declaration::TypeAlias(alias) => alias.exported,
                Declaration::Interface(interface) => interface.exported,
                Declaration::Variable(variable) => variable.exported,
                Declaration::Function(function) => function.exported,
                Declaration::Class(class) => class.exported,
                Declaration::Enum(item) => item.exported,
                Declaration::Namespace(namespace) => namespace.exported,
                Declaration::Ambient(_) | Declaration::UmdExport(_) | Declaration::Raw(_) => false,
            });
        if !external {
            for declaration in &self.declarations {
                if let Declaration::UmdExport(export) = declaration {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParseError,
                            export.span.clone(),
                            "global module exports require an external module",
                        )
                        .with_typescript(1314, Vec::new()),
                    );
                }
            }
        }
    }

    pub(in crate::parser::implementation) fn parse_import(&mut self, start: usize) {
        self.expect("import");
        let type_only = self.peek("type")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| !matches!(token.text.as_str(), "from" | "," | "="))
            && self.consume("type");
        let clause_start = if type_only {
            self.previous().start
        } else {
            self.current().start
        };
        if (self.current().kind == TokenKind::Identifier || self.peek("type") || self.peek("of"))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is("="))
        {
            let local = self.current().text.clone();
            self.bump();
            self.bump();
            let is_require = self.consume("require")
                && self.consume("(")
                && self.current().kind == TokenKind::String;
            if !is_require {
                self.unsupported(
                    self.current().span(&self.id),
                    "only `import name = require(\"module\")` is supported as an `import =` form",
                );
                self.skip_statement();
                return;
            }
            let specifier = string_contents(self.current()).unwrap_or_default();
            let specifier_span = self.current().span(&self.id);
            self.bump();
            self.expect(")");
            self.consume(";");
            let end = self.previous().end;
            self.declarations
                .push(Declaration::Import(ImportDeclaration {
                    equals_require: true,
                    type_only,
                    specifier,
                    specifier_span,
                    attributes: None,
                    bindings: vec![ImportBinding {
                        imported: "export=".to_string(),
                        local,
                        type_only,
                        span: SourceSpan::new(&self.id, start, end),
                    }],
                    span: SourceSpan::new(&self.id, start, end),
                }));
            if type_only {
                self.edits.push(TextEdit {
                    start,
                    end,
                    replacement: String::new(),
                });
            }
            return;
        }
        let mut bindings = Vec::new();
        let mut default_clause = false;
        let mut specifier = None;
        let mut specifier_span = None;
        if self.current().kind == TokenKind::String {
            specifier = string_contents(self.current());
            specifier_span = Some(self.current().span(&self.id));
            self.bump();
        } else {
            if self.consume("{") {
                self.parse_named_import_bindings(&mut bindings, type_only);
            } else if let Some(local) = self.consume_identifier() {
                default_clause = true;
                bindings.push(ImportBinding {
                    imported: "default".to_string(),
                    local,
                    type_only,
                    span: self.previous().span(&self.id),
                });
                if self.consume(",") && self.consume("{") {
                    self.parse_named_import_bindings(&mut bindings, type_only);
                } else if self.consume("*") {
                    self.expect("as");
                    let local = self.require_identifier("expected namespace import name");
                    bindings.push(ImportBinding {
                        imported: "*".to_string(),
                        local,
                        type_only,
                        span: self.previous().span(&self.id),
                    });
                }
            } else if self.consume("*") {
                self.expect("as");
                let local = self.require_identifier("expected namespace import name");
                bindings.push(ImportBinding {
                    imported: "*".to_string(),
                    local,
                    type_only,
                    span: self.previous().span(&self.id),
                });
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an import clause or module specifier",
                );
            }
            if type_only && default_clause && bindings.len() > 1 {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParseError,
                        SourceSpan::new(&self.id, clause_start, self.previous().end),
                        "a type-only import cannot combine a default import with named bindings",
                    )
                    .with_typescript(1363, Vec::new()),
                );
            }
            self.expect("from");
            if self.current().kind == TokenKind::String {
                specifier = string_contents(self.current());
                specifier_span = Some(self.current().span(&self.id));
                self.bump();
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a string module specifier",
                );
                if !self.at_eof() && !self.peek(";") {
                    let diagnostic = self.diagnostics.last_mut().expect("module specifier error");
                    *diagnostic = diagnostic.clone().with_typescript(1141, Vec::new());
                }
            }
        }
        let attributes = self.parse_import_attributes(type_only);
        self.consume(";");
        let end = self.previous().end;
        let span = SourceSpan::new(&self.id, start, end);
        let (Some(specifier), Some(specifier_span)) = (specifier, specifier_span) else {
            return;
        };
        if type_only || bindings.iter().all(|binding| binding.type_only) && !bindings.is_empty() {
            self.edits.push(TextEdit {
                start,
                end,
                replacement: String::new(),
            });
        }
        self.declarations
            .push(Declaration::Import(ImportDeclaration {
                equals_require: false,
                type_only,
                specifier,
                specifier_span,
                attributes,
                bindings,
                span,
            }));
    }

    fn parse_named_import_bindings(&mut self, bindings: &mut Vec<ImportBinding>, type_only: bool) {
        while !self.at_eof() && !self.consume("}") {
            let binding_start = self.current().start;
            let modifier = self.peek("type")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| !matches!(token.text.as_str(), "as" | "," | "}"));
            let binding_type_only = modifier && self.consume("type");
            if type_only && binding_type_only {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParseError,
                        self.previous().span(&self.id),
                        "a type-only import cannot repeat the type modifier",
                    )
                    .with_typescript(2206, Vec::new()),
                );
            }
            let imported = if self.current().kind == TokenKind::String {
                let value = string_contents(self.current()).unwrap_or_default();
                self.bump();
                value
            } else if let Some(name) = self.consume_identifier_or_keyword() {
                name
            } else {
                self.error_here(DiagnosticCode::ParseError, "expected an imported binding");
                self.skip_until(&["}", ";"]);
                self.consume("}");
                break;
            };
            let local = if self.consume("as") {
                self.require_identifier("expected a local import name")
            } else {
                imported.clone()
            };
            bindings.push(ImportBinding {
                imported,
                local,
                type_only: type_only || binding_type_only,
                span: SourceSpan::new(&self.id, binding_start, self.previous().end),
            });
            if !self.consume(",") {
                self.expect("}");
                break;
            }
        }
    }

    /// `export = name;` with the cursor on the `=`.
    pub(in crate::parser::implementation) fn parse_export_assignment(&mut self, start: usize) {
        self.expect("=");
        let name_span = self.current().span(&self.id);
        let name = self.consume_identifier().filter(|_| {
            // The whole statement is the name: `export = a.b` is a member access.
            self.at_eof()
                || !matches!(
                    self.current().text.as_str(),
                    "." | "?." | "[" | "(" | "<" | "!"
                )
        });
        let Some(name) = name else {
            self.unsupported(
                name_span,
                "only `export = name;` of a local declaration is supported",
            );
            self.skip_statement();
            return;
        };
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::ValueExport(ValueExportDeclaration {
                export_assignment: true,
                specifier: None,
                specifier_span: None,
                attributes: None,
                star: false,
                namespace: None,
                bindings: vec![ValueExportBinding {
                    local: name,
                    exported: "export=".to_string(),
                    type_only: false,
                    span: name_span,
                }],
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_type_export(&mut self, start: usize) {
        let mut bindings = Vec::new();
        let mut namespace = None;
        let star = self.consume("*");
        if !star {
            self.expect("{");
            while !self.at_eof() && !self.consume("}") {
                let binding_start = self.current().start;
                let local = self.require_export_name("expected an exported type name");
                let exported = if self.consume("as") {
                    self.require_export_name("expected an exported type name")
                } else {
                    local.clone()
                };
                bindings.push(ValueExportBinding {
                    local,
                    exported,
                    type_only: true,
                    span: SourceSpan::new(&self.id, binding_start, self.previous().end),
                });
                if !self.consume(",") {
                    self.expect("}");
                    break;
                }
            }
        } else {
            if self.consume("as") {
                namespace = Some(self.require_export_name("expected an exported type name"));
            }
        }
        let mut specifier_span = None;
        let specifier = if self.consume("from") {
            if self.current().kind == TokenKind::String {
                specifier_span = Some(self.current().span(&self.id));
                let value = string_contents(self.current());
                self.bump();
                value
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a string module specifier",
                );
                if !self.at_eof() && !self.peek(";") {
                    let diagnostic = self.diagnostics.last_mut().expect("module specifier error");
                    *diagnostic = diagnostic.clone().with_typescript(1141, Vec::new());
                }
                None
            }
        } else {
            None
        };
        let attributes = self.parse_import_attributes(true);
        self.consume(";");
        let end = self.previous().end;
        self.edits.push(TextEdit {
            start,
            end,
            replacement: String::new(),
        });
        self.declarations
            .push(Declaration::TypeExport(TypeExportDeclaration {
                bindings,
                star,
                specifier,
                specifier_span,
                attributes,
                namespace,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_default_export(&mut self, start: usize) {
        let name = self.require_identifier("expected a default export name");
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::DefaultExport(DefaultExportDeclaration {
                name,
                expression: false,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_value_export(&mut self, start: usize) {
        let star = self.consume("*");
        let namespace = if star && self.consume("as") {
            Some(self.require_identifier("expected a namespace export name"))
        } else {
            None
        };
        if !star {
            self.expect("{");
        }
        let mut bindings = Vec::new();
        while !star && !self.at_eof() && !self.consume("}") {
            let binding_start = self.current().start;
            let type_only = self.peek("type")
                && self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|token| !matches!(token.text.as_str(), "as" | "," | "}"))
                && self.consume("type");
            let local = self.require_export_name("expected an export binding name");
            let exported = if self.consume("as") {
                self.require_export_name("expected an export binding name")
            } else {
                local.clone()
            };
            bindings.push(ValueExportBinding {
                local,
                exported,
                type_only,
                span: SourceSpan::new(&self.id, binding_start, self.previous().end),
            });
            if !self.consume(",") {
                self.expect("}");
                break;
            }
        }
        let (specifier, specifier_span) = if self.consume("from") {
            let span = self.current().span(&self.id);
            let value = string_contents(self.current());
            if value.is_some() {
                self.bump();
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a string module specifier",
                );
            }
            (value, Some(span))
        } else {
            if star {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected from after a star export",
                );
            }
            (None, None)
        };
        let attributes = self.parse_import_attributes(false);
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::ValueExport(ValueExportDeclaration {
                export_assignment: false,
                bindings,
                specifier,
                specifier_span,
                attributes,
                star,
                namespace,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    fn require_export_name(&mut self, expected: &str) -> String {
        if self.current().kind == TokenKind::String {
            let value = string_contents(self.current()).unwrap_or_default();
            self.bump();
            value
        } else {
            self.consume_identifier_or_keyword().unwrap_or_else(|| {
                self.error_here(DiagnosticCode::ParseError, expected);
                "<error>".to_string()
            })
        }
    }
}
