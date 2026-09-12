// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::module::pattern_bound_names;
use super::*;

impl Parser {
    /// Parse the import-attributes `with { ... }` clause.  Module records in
    /// this host currently retain only the module-request string, but the
    /// grammar and duplicate-key early error are observable before host
    /// resolution and therefore belong in the parser rather than in the
    /// Test262 adapter.
    pub(super) fn parse_import_attributes(&mut self) -> Result<(), ParseError> {
        if !self.eat_identifier("with") {
            return Ok(());
        }
        self.expect_punct(Punct::LBrace)?;
        let mut keys = std::collections::HashSet::new();
        while !self.check_punct(Punct::RBrace) {
            let key = self.expect_module_export_name()?;
            if !keys.insert(key) {
                return Err(self.syntax_error("duplicate import attribute key"));
            }
            self.expect_punct(Punct::Colon)?;
            match self.advance() {
                Token::String(_) => {}
                _ => return Err(self.syntax_error("import attribute values must be strings")),
            }
            if self.eat_punct(Punct::Comma) {
                if self.check_punct(Punct::RBrace) {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect_punct(Punct::RBrace)
    }

    /// Consumes a statement-terminating `;`, or applies automatic
    /// semicolon insertion (see `token.rs`'s [`SpannedToken`] doc
    /// comment): a `}`, EOF, or a preceding line terminator all count
    /// as an implicit semicolon, matching the common cases real
    /// hand-written scripts rely on (MVP scope doesn't need the full
    /// spec algorithm's edge cases, e.g. the restricted-token-list
    /// exceptions).
    pub(super) fn consume_semicolon(&mut self) -> Result<(), ParseError> {
        if self.eat_punct(Punct::Semicolon) {
            return Ok(());
        }
        if self.check_punct(Punct::RBrace) || self.at_eof() || self.newline_before() {
            return Ok(());
        }
        // A completed statement expression can only be followed by an ASI
        // boundary or a semicolon.  This is not an unsupported production.
        Err(self.syntax_error("expected ';'"))
    }

    // ---- Statements ----

    pub(super) fn parse_import_declaration(&mut self) -> Result<Vec<ImportEntry>, ParseError> {
        debug_assert!(self.check_identifier("import"));
        self.advance();
        if matches!(self.peek(), Token::String(_)) {
            let module_request = self.expect_module_name()?;
            self.parse_import_attributes()?;
            self.consume_semicolon()?;
            // A side-effect-only import still creates a requested module.
            return Ok(vec![ImportEntry {
                module_request,
                import_name: ImportName::Named(String::new()),
                local_name: None,
            }]);
        }

        // Source phase imports deliberately use contextual identifiers: both
        // `source` and `from` remain valid imported binding names. Recognize
        // this before the ordinary default-import branch, where
        // `import source local from "..."` would otherwise be read as a
        // default binding followed by an unexpected identifier.
        if self.check_identifier("source")
            && matches!(self.peek_at(1), Token::Identifier(_))
            && self.check_identifier_at(2, "from")
        {
            self.advance();
            let local_name = self.expect_binding_identifier()?;
            if !self.eat_identifier("from") {
                return Err(self.syntax_error("source import requires 'from'"));
            }
            let module_request = self.expect_module_name()?;
            self.parse_import_attributes()?;
            self.consume_semicolon()?;
            return Ok(vec![ImportEntry {
                module_request,
                import_name: ImportName::Source,
                local_name: Some(local_name),
            }]);
        }

        let mut entries = Vec::new();
        let mut has_following_clause = true;
        if !self.check_punct(Punct::LBrace) && !self.check_punct(Punct::Star) {
            let local_name = self.expect_binding_identifier()?;
            entries.push((ImportName::Named("default".to_string()), local_name));
            has_following_clause = self.eat_punct(Punct::Comma);
            if !has_following_clause && !self.check_identifier("from") {
                return Err(self.syntax_error("default import requires 'from' or ','"));
            }
        }
        if has_following_clause && self.eat_punct(Punct::Star) {
            if !self.eat_identifier("as") {
                return Err(self.syntax_error("namespace import requires 'as'"));
            }
            entries.push((ImportName::Namespace, self.expect_binding_identifier()?));
        } else if has_following_clause {
            self.expect_punct(Punct::LBrace)?;
            while !self.check_punct(Punct::RBrace) {
                let import_name = self.expect_module_export_name()?;
                let local_name = if self.eat_identifier("as") {
                    self.expect_binding_identifier()?
                } else {
                    import_name.clone()
                };
                entries.push((ImportName::Named(import_name), local_name));
                if !self.check_punct(Punct::RBrace) {
                    self.expect_punct(Punct::Comma)?;
                }
            }
            self.expect_punct(Punct::RBrace)?;
        }
        if !self.eat_identifier("from") {
            return Err(self.syntax_error("import declaration requires 'from'"));
        }
        let module_request = self.expect_module_name()?;
        self.parse_import_attributes()?;
        self.consume_semicolon()?;
        if entries.is_empty() {
            return Ok(vec![ImportEntry {
                module_request,
                import_name: ImportName::Named(String::new()),
                local_name: None,
            }]);
        }
        Ok(entries
            .into_iter()
            .map(|(import_name, local_name)| ImportEntry {
                module_request: module_request.clone(),
                import_name,
                local_name: Some(local_name),
            })
            .collect())
    }

    pub(super) fn parse_export_declaration(
        &mut self,
        body: &mut Vec<Stmt>,
        exports: &mut Vec<ExportEntry>,
    ) -> Result<Option<String>, ParseError> {
        debug_assert!(self.check_identifier("export"));
        self.advance();
        if self.eat_punct(Punct::Star) {
            let export_name = if self.eat_identifier("as") {
                Some(self.expect_module_export_name()?)
            } else {
                None
            };
            if !self.eat_identifier("from") {
                return Err(self.syntax_error("star export requires 'from'"));
            }
            let module_request = self.expect_module_name()?;
            self.parse_import_attributes()?;
            self.consume_semicolon()?;
            let request = module_request.clone();
            exports.push(match export_name {
                Some(export_name) => ExportEntry::Namespace {
                    export_name,
                    module_request,
                },
                None => ExportEntry::Star { module_request },
            });
            return Ok(Some(request));
        }
        if self.eat_identifier("default") || self.eat_keyword(Keyword::Default) {
            let hidden = "\0bluejs_module_default".to_string();
            let (local_name, consume_terminator) = match self.peek().clone() {
                Token::Keyword(Keyword::Function) => {
                    self.advance();
                    let function = self.parse_function()?;
                    let binding = function.name.clone().unwrap_or_else(|| hidden.clone());
                    body.push(Stmt::ModuleDefaultFunction {
                        function,
                        binding: binding.clone(),
                    });
                    (binding, false)
                }
                Token::Identifier(name) if name == "async" && self.async_function_follows() => {
                    self.require_unescaped_async()?;
                    self.advance();
                    self.expect_keyword(Keyword::Function)?;
                    let function = self.parse_function_with_async(true)?;
                    let binding = function.name.clone().unwrap_or_else(|| hidden.clone());
                    body.push(Stmt::ModuleDefaultFunction {
                        function,
                        binding: binding.clone(),
                    });
                    (binding, false)
                }
                Token::Identifier(name) if name == "class" => {
                    self.advance();
                    let class = self.parse_class()?;
                    if let Some(binding) = class.name.clone() {
                        body.push(Stmt::ClassDecl(class));
                        (binding, false)
                    } else {
                        body.push(Stmt::VarDecl(
                            DeclKind::Const,
                            vec![VarDeclarator {
                                pattern: Pattern::Identifier(hidden.clone()),
                                init: Some(Expr::Class(class)),
                            }],
                        ));
                        (hidden.clone(), false)
                    }
                }
                Token::Keyword(Keyword::Var | Keyword::Let | Keyword::Const) => {
                    return Err(
                        self.syntax_error("a default export cannot declare a variable binding")
                    );
                }
                _ => {
                    body.push(Stmt::VarDecl(
                        DeclKind::Const,
                        vec![VarDeclarator {
                            pattern: Pattern::Identifier(hidden.clone()),
                            init: Some(self.parse_assignment()?),
                        }],
                    ));
                    (hidden, true)
                }
            };
            if !consume_terminator && self.check_punct(Punct::LParen) {
                return Err(self.syntax_error(
                    "a default function or class declaration cannot be invoked directly",
                ));
            }
            if consume_terminator {
                self.consume_semicolon()?;
            }
            exports.push(ExportEntry::Local {
                export_name: "default".to_string(),
                local_name,
            });
            return Ok(None);
        }
        if self.eat_punct(Punct::LBrace) {
            let mut specifiers = Vec::new();
            while !self.check_punct(Punct::RBrace) {
                let local_is_string = matches!(self.peek(), Token::String(_));
                let local_name = self.expect_module_export_name()?;
                let export_name = if self.eat_identifier("as") {
                    self.expect_module_export_name()?
                } else {
                    local_name.clone()
                };
                specifiers.push((local_name, export_name, local_is_string));
                if !self.check_punct(Punct::RBrace) {
                    self.expect_punct(Punct::Comma)?;
                }
            }
            self.expect_punct(Punct::RBrace)?;
            let request = if self.eat_identifier("from") {
                let module_request = self.expect_module_name()?;
                self.parse_import_attributes()?;
                for (import_name, export_name, _) in specifiers {
                    exports.push(ExportEntry::Indirect {
                        export_name,
                        module_request: module_request.clone(),
                        import_name,
                    });
                }
                Some(module_request)
            } else {
                for (local_name, export_name, local_is_string) in specifiers {
                    if local_is_string {
                        return Err(
                            self.syntax_error("a local module export name must be an identifier")
                        );
                    }
                    exports.push(ExportEntry::Local {
                        export_name,
                        local_name,
                    });
                }
                None
            };
            self.consume_semicolon()?;
            return Ok(request);
        }

        let statement = match self.peek().clone() {
            Token::Keyword(Keyword::Var) => self.parse_var_decl_stmt(DeclKind::Var)?,
            Token::Keyword(Keyword::Let) => self.parse_var_decl_stmt(DeclKind::Let)?,
            Token::Keyword(Keyword::Const) => self.parse_var_decl_stmt(DeclKind::Const)?,
            Token::Keyword(Keyword::Function) => {
                self.advance();
                let function = self.parse_function()?;
                if function.name.is_none() {
                    return Err(self.syntax_error("function declarations require a name"));
                }
                Stmt::FunctionDecl(function)
            }
            Token::Identifier(name) if name == "class" => {
                self.advance();
                let class = self.parse_class()?;
                if class.name.is_none() {
                    return Err(self.syntax_error("class declarations require a name"));
                }
                Stmt::ClassDecl(class)
            }
            _ => return Err(self.syntax_error("expected an export declaration")),
        };
        let names = match &statement {
            Stmt::VarDecl(_, declarations) => declarations
                .iter()
                .flat_map(|declaration| pattern_bound_names(&declaration.pattern))
                .collect(),
            Stmt::FunctionDecl(function) => vec![function.name.clone().unwrap()],
            Stmt::ClassDecl(class) => vec![class.name.clone().unwrap()],
            _ => unreachable!("module export parser only produces declarations"),
        };
        for name in names {
            exports.push(ExportEntry::Local {
                export_name: name.clone(),
                local_name: name,
            });
        }
        body.push(statement);
        Ok(None)
    }
}
