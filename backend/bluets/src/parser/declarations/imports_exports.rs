// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import and export declarations.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_import(&mut self, start: usize) {
        self.expect("import");
        let type_only = self.consume("type");
        if !type_only
            && self.current().kind == TokenKind::Identifier
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
                    bindings: vec![ImportBinding {
                        imported: "export=".to_string(),
                        local,
                        type_only,
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
        let mut specifier = None;
        let mut specifier_span = None;
        if self.current().kind == TokenKind::String {
            specifier = string_contents(self.current());
            specifier_span = Some(self.current().span(&self.id));
            self.bump();
        } else {
            if self.consume("{") {
                while !self.at_eof() && !self.consume("}") {
                    let binding_type_only = self.consume("type");
                    let Some(imported) = self.consume_identifier() else {
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
                    });
                    if !self.consume(",") {
                        self.expect("}");
                        break;
                    }
                }
            } else if let Some(local) = self.consume_identifier() {
                bindings.push(ImportBinding {
                    imported: "default".to_string(),
                    local,
                    type_only,
                });
                self.consume(",");
                if self.consume("*") {
                    self.expect("as");
                    let local = self.require_identifier("expected namespace import name");
                    bindings.push(ImportBinding {
                        imported: "*".to_string(),
                        local,
                        type_only,
                    });
                }
            } else if self.consume("*") {
                self.expect("as");
                let local = self.require_identifier("expected namespace import name");
                bindings.push(ImportBinding {
                    imported: "*".to_string(),
                    local,
                    type_only,
                });
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an import clause or module specifier",
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
        self.consume(";");
        let end = self.previous().end;
        let span = SourceSpan::new(&self.id, start, end);
        let (Some(specifier), Some(specifier_span)) = (specifier, specifier_span) else {
            return;
        };
        if !type_only && bindings.iter().any(|binding| binding.type_only) {
            self.unsupported(
                span.clone(),
                "mixed value/type imports are not in the initial BlueTS matrix; use a separate `import type` declaration",
            );
        }
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
                bindings,
                span,
            }));
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
                bindings: vec![ValueExportBinding {
                    local: name,
                    exported: "export=".to_string(),
                    span: name_span,
                }],
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_type_export(&mut self, start: usize) {
        let mut bindings = Vec::new();
        if self.consume("{") {
            while !self.at_eof() && !self.consume("}") {
                let local = self.require_identifier("expected a type export name");
                let exported = if self.consume("as") {
                    self.require_identifier("expected an exported type name")
                } else {
                    local.clone()
                };
                bindings.push(if local == exported {
                    local
                } else {
                    format!("{local} as {exported}")
                });
                if !self.consume(",") {
                    self.expect("}");
                    break;
                }
            }
        } else {
            self.expect("*");
            bindings.push("*".to_string());
        }
        let specifier = if self.consume("from") {
            if self.current().kind == TokenKind::String {
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
                specifier,
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
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_value_export(&mut self, start: usize) {
        self.expect("{");
        let mut bindings = Vec::new();
        while !self.at_eof() && !self.consume("}") {
            if self.peek("type") {
                self.unsupported(
                    self.current().span(&self.id),
                    "type-only bindings in a value export are not in the initial BlueTS matrix; use `export type`",
                );
                self.skip_statement();
                return;
            }
            let binding_start = self.current().start;
            let local = self.require_identifier("expected a value export name");
            let exported = if self.consume("as") {
                self.require_identifier("expected an exported value name")
            } else {
                local.clone()
            };
            if exported == "default" {
                self.unsupported(
                    SourceSpan::new(&self.id, binding_start, self.previous().end),
                    "default aliases in named value exports are not in the initial BlueTS matrix",
                );
            }
            bindings.push(ValueExportBinding {
                local,
                exported,
                span: SourceSpan::new(&self.id, binding_start, self.previous().end),
            });
            if !self.consume(",") {
                self.expect("}");
                break;
            }
        }
        if self.consume("from") {
            self.unsupported(
                self.previous().span(&self.id),
                "value re-exports from another module are not in the initial BlueTS matrix",
            );
            self.skip_statement();
            return;
        }
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::ValueExport(ValueExportDeclaration {
                export_assignment: false,
                bindings,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }
}
