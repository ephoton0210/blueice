// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AST -> operand-stack bytecode, per Phase 13's first execution slice.
//! Binding resolution happens here; runtime execution never walks an AST.
//! Every lexical scope has its own slots, reset on entry/exit. Abrupt
//! loop exits emit the same scope cleanup as ordinary block exits.

use crate::bytecode::{
    AbruptJump, Binding, Handler, ModuleExport as CompiledModuleExport,
    ModuleImport as CompiledModuleImport, ModuleImportName as CompiledModuleImportName,
    ModuleRequest as CompiledModuleRequest,
};
use crate::*;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Parser-private binding used to represent an anonymous `export default`
/// declaration.  It can never be spelled by ECMAScript source, which lets
/// compilation retain the binding separately from the `"default"` inferred
/// function/class name required by SetFunctionName.
const MODULE_DEFAULT_BINDING: &str = "\0bluejs_module_default";
const PRIVATE_OWNER_BINDING_PREFIX: &str = "\0bluejs_private_owner_";
use std::fmt;

mod expressions;
mod functions;
mod private_validation;
mod statements;
pub(crate) use private_validation::validate_private_early_errors;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Unsupported(&'static str),
    DuplicateBinding(String),
    InvalidSyntax(&'static str),
    ProgramTooLarge,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => {
                write!(f, "BlueJS execution does not yet support {feature}")
            }
            Self::DuplicateBinding(name) => write!(f, "duplicate or conflicting binding: {name}"),
            Self::InvalidSyntax(message) => f.write_str(message),
            Self::ProgramTooLarge => f.write_str("BlueJS program exceeds the bytecode size limit"),
        }
    }
}
impl std::error::Error for CompileError {}

/// Compiles the supported executable subset. The parser deliberately
/// accepts more than the VM can run; unsupported syntax is rejected even
/// in unreachable branches, before any execution or heap mutation.
pub fn compile(program: &Program) -> Result<Bytecode, CompileError> {
    compile_with_limit(program, u32::MAX)
}

/// Compiles one parsed module before graph linking. Its declarative entries
/// retain resolved local slots in bytecode, while top-level function
/// declarations get a declaration-instantiation prefix for cyclic graphs.
pub fn compile_module(module: &Module) -> Result<Bytecode, CompileError> {
    compile_module_with_limit(module, u32::MAX)
}

/// Compiles with an inclusive limit on emitted instruction bytes.
/// A limit failure returns [`CompileError::ProgramTooLarge`], never partial
/// bytecode. This does not bound AST depth, constant payloads or total memory.
pub fn compile_with_limit(
    program: &Program,
    max_bytecode_bytes: u32,
) -> Result<Bytecode, CompileError> {
    compile_with_limit_and_mode(program, max_bytecode_bytes, false, &[], &[], &[])
}

/// Like [`compile_module`], with the Test262 adapter's bytecode resource
/// limit. This is public so an embedder can apply the same bound before
/// module linking has expanded to a graph.
pub fn compile_module_with_limit(
    module: &Module,
    max_bytecode_bytes: u32,
) -> Result<Bytecode, CompileError> {
    compile_with_limit_and_mode(
        &Program {
            body: module.body.clone(),
        },
        max_bytecode_bytes,
        true,
        &module.imports,
        &module.exports,
        &module.requests,
    )
}

fn compile_with_limit_and_mode(
    program: &Program,
    max_bytecode_bytes: u32,
    module: bool,
    module_imports: &[ImportEntry],
    module_exports: &[ExportEntry],
    module_requests: &[String],
) -> Result<Bytecode, CompileError> {
    let mut compiler = Compiler {
        bytecode: Bytecode::empty(),
        names: Vec::new(),
        private_scopes: Vec::new(),
        next_private_scope: 0,
        scopes: Vec::new(),
        loops: Vec::new(),
        catch_var_slots: Vec::new(),
        max_bytecode_bytes,
        function: false,
        local_scope: 0,
        with_depth: 0,
    };
    compiler.bytecode.strict = module || strict_body(&program.body);
    compiler.bytecode.module = module;
    compiler.bytecode.import_meta_allowed = module;
    if compiler.bytecode.strict && strict_assignment_to_restricted_name(&program.body) {
        return Err(CompileError::InvalidSyntax(
            "strict code cannot assign to eval or arguments",
        ));
    }
    compiler.bytecode.global_function_names = program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDecl(function) => function.name.clone(),
            _ => None,
        })
        .collect();
    let mut lexical = lexical_names(&program.body)?;
    let mut vars = top_level_var_names(&program.body)?;
    if module {
        for import in module_imports {
            if let Some(local_name) = &import.local_name {
                lexical.push((local_name.clone(), DeclKind::Const));
            }
        }
        // Module function declarations are lexical bindings, rather than the
        // classic-script global var bindings used by the existing compiler.
        let function_names: BTreeSet<_> = program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Stmt::FunctionDecl(function) => function.name.clone(),
                _ => None,
            })
            .collect();
        vars.retain(|name| !function_names.contains(name));
        lexical.extend(function_names.into_iter().map(|name| (name, DeclKind::Let)));
        lexical.extend(program.body.iter().filter_map(|statement| match statement {
            Stmt::ModuleDefaultFunction { binding, .. } => Some((binding.clone(), DeclKind::Let)),
            _ => None,
        }));
    }
    if !compiler.bytecode.strict {
        vars.extend(
            annex_b_function_names(&program.body, &lexical)
                .into_iter()
                .filter(|name| !lexical.iter().any(|(lexical_name, _)| lexical_name == name)),
        );
    }
    compiler.enter_scope(lexical, &vars, true)?;
    if module {
        compiler.function_declarations(&program.body)?;
        compiler.bytecode.module_evaluate_entry = Some(compiler.offset()?);
        compiler.statements_after_function_declarations(&program.body)?;
    } else {
        compiler.statements(&program.body)?;
    }
    compiler.emit(Opcode::Halt, 0)?;
    if module {
        compiler.bytecode.module_imports = module_imports
            .iter()
            .map(|import| {
                let local_slot = import.local_name.as_ref().map(|local_name| {
                    compiler
                        .resolve(local_name)
                        .expect("module import binding was declared in the outer scope")
                });
                CompiledModuleImport {
                    module_request: import.module_request.clone(),
                    import_name: match &import.import_name {
                        ImportName::Named(name) => CompiledModuleImportName::Named(name.clone()),
                        ImportName::Namespace => CompiledModuleImportName::Namespace,
                        ImportName::Source => CompiledModuleImportName::Source,
                    },
                    local_slot,
                }
            })
            .collect();
        // An `export { local }` that names an imported binding is not a
        // local export in a Source Text Module Record.  It is an indirect
        // export of the original imported name (or a namespace export for
        // `import * as local`).  Keeping that distinction is essential for
        // ResolveExport: a local slot is private to this record, whereas the
        // export must retain the dependency edge through cycles and star
        // export ambiguity checks.
        let imported_locals: HashMap<&str, &ImportEntry> = module_imports
            .iter()
            .filter_map(|import| {
                import
                    .local_name
                    .as_deref()
                    .map(|local_name| (local_name, import))
            })
            .collect();
        compiler.bytecode.module_exports = module_exports
            .iter()
            .map(|export| match export {
                ExportEntry::Local {
                    export_name,
                    local_name,
                } => match imported_locals.get(local_name.as_str()) {
                    Some(ImportEntry {
                        module_request,
                        import_name: ImportName::Named(import_name),
                        ..
                    }) => Ok(CompiledModuleExport::Indirect {
                        export_name: export_name.clone(),
                        module_request: module_request.clone(),
                        import_name: import_name.clone(),
                    }),
                    Some(ImportEntry {
                        module_request,
                        import_name: ImportName::Namespace,
                        ..
                    }) => Ok(CompiledModuleExport::Namespace {
                        export_name: export_name.clone(),
                        module_request: module_request.clone(),
                    }),
                    Some(ImportEntry {
                        module_request,
                        import_name: ImportName::Source,
                        ..
                    }) => Ok(CompiledModuleExport::Source {
                        export_name: export_name.clone(),
                        module_request: module_request.clone(),
                    }),
                    None => Ok(CompiledModuleExport::Local {
                        export_name: export_name.clone(),
                        local_slot: compiler.resolve(local_name).ok_or(
                            CompileError::InvalidSyntax(
                                "export references an undeclared local binding",
                            ),
                        )?,
                    }),
                },
                ExportEntry::Indirect {
                    export_name,
                    module_request,
                    import_name,
                } => Ok(CompiledModuleExport::Indirect {
                    export_name: export_name.clone(),
                    module_request: module_request.clone(),
                    import_name: import_name.clone(),
                }),
                ExportEntry::Star { module_request } => Ok(CompiledModuleExport::Star {
                    module_request: module_request.clone(),
                }),
                ExportEntry::Namespace {
                    export_name,
                    module_request,
                } => Ok(CompiledModuleExport::Namespace {
                    export_name: export_name.clone(),
                    module_request: module_request.clone(),
                }),
            })
            .collect::<Result<Vec<_>, CompileError>>()?;
        let mut seen = HashSet::new();
        compiler.bytecode.module_requests = module_requests
            .iter()
            .filter(|request| seen.insert(request.as_str()))
            .map(|module_request| CompiledModuleRequest {
                module_request: module_request.clone(),
            })
            .collect();
    }
    Ok(compiler.bytecode)
}

/// Compiles direct-eval source with cells for the caller's visible bindings.
/// The runtime supplies `visible` from its active lexical environments and
/// installs the matching cells before running the resulting bytecode.
pub(crate) fn compile_eval(
    program: &Program,
    visible: &[(String, Binding, u32)],
    variable_environment_names: &[String],
    lexical_conflicts: &[String],
    strict: bool,
    new_target_allowed: bool,
    with_depth: usize,
) -> Result<Bytecode, CompileError> {
    let mut compiler = Compiler {
        bytecode: Bytecode::empty(),
        names: vec![HashMap::new()],
        private_scopes: vec![HashMap::new()],
        next_private_scope: 0,
        scopes: Vec::new(),
        loops: Vec::new(),
        catch_var_slots: Vec::new(),
        max_bytecode_bytes: u32::MAX,
        function: false,
        local_scope: 1,
        with_depth,
    };
    compiler.bytecode.strict = strict || strict_body(&program.body);
    compiler.bytecode.new_target_allowed = new_target_allowed;
    if compiler.bytecode.strict && strict_assignment_to_restricted_name(&program.body) {
        return Err(CompileError::InvalidSyntax(
            "strict code cannot assign to eval or arguments",
        ));
    }
    compiler.bytecode.global_function_names = program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDecl(function) => function.name.clone(),
            _ => None,
        })
        .collect();
    for (name, binding, caller_slot) in visible {
        let slot = u32::try_from(compiler.bytecode.bindings.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        compiler.names[0].insert(name.clone(), slot);
        if let Some((scope, private_name)) = private_owner_binding_name(name) {
            // Direct eval inherits lexical private names just as it inherits
            // ordinary captured bindings.  The innermost live private scope
            // wins when a nested class shadows a name.
            let scope_map = compiler
                .private_scopes
                .first_mut()
                .expect("eval has a private scope");
            let replace = scope_map
                .get(&private_name)
                .and_then(|binding| private_owner_binding_name(binding))
                .is_none_or(|(existing, _)| scope >= existing);
            if replace {
                scope_map.insert(private_name, name.clone());
            }
        }
        compiler.bytecode.bindings.push(binding.clone());
        compiler.bytecode.captures.push(*caller_slot);
    }
    let lexical = lexical_names(&program.body)?;
    let mut vars = top_level_var_names(&program.body)?;
    if !compiler.bytecode.strict {
        vars.extend(
            annex_b_function_names(&program.body, &lexical)
                .into_iter()
                .filter(|name| !lexical.iter().any(|(lexical_name, _)| lexical_name == name)),
        );
    }
    // EvalDeclarationInstantiation walks from its fresh lexical environment
    // toward the caller's VariableEnvironment. A sloppy eval `var` cannot
    // cross a caller lexical (including a non-simple parameter) with the
    // same name. The compiler receives those caller cells as `visible`.
    if !compiler.bytecode.strict
        && vars
            .iter()
            .any(|name| lexical_conflicts.iter().any(|conflict| conflict == name))
    {
        return Err(CompileError::InvalidSyntax(
            "eval var declaration conflicts with a lexical binding",
        ));
    }
    // Strict eval has its own VariableEnvironment, so its `var` bindings
    // shadow caller names. Sloppy direct eval extends only the immediately
    // enclosing VariableEnvironment: a name captured from an outer function
    // remains visible for reads, but must not prevent a new local eval `var`.
    let new_vars = if compiler.bytecode.strict {
        vars
    } else {
        vars.into_iter()
            .filter(|name| !variable_environment_names.contains(name))
            .collect()
    };
    compiler.enter_scope(lexical, &new_vars, true)?;
    if !compiler.bytecode.strict {
        compiler.bytecode.dynamic_eval_slots = new_vars
            .iter()
            .filter_map(|name| {
                compiler
                    .names
                    .last()
                    .and_then(|scope| scope.get(name))
                    .copied()
            })
            .collect();
    }
    compiler.statements(&program.body)?;
    compiler.emit(Opcode::Halt, 0)?;
    Ok(compiler.bytecode)
}

struct Loop {
    labels: Vec<String>,
    breakable: bool,
    scope_depth: usize,
    breaks: Vec<(usize, usize)>,
    continues: Option<Vec<(usize, usize)>>,
    iterator: Option<u32>,
}

struct Compiler {
    bytecode: Bytecode,
    names: Vec<HashMap<String, u32>>,
    /// Each lexical class private-name environment maps the source spelling
    /// (without `#`) to an internal binding holding that name's declaring
    /// class owner.  The binding is captured like any other lexical value,
    /// which is the crucial distinction from a function [[HomeObject]].
    private_scopes: Vec<HashMap<String, String>>,
    next_private_scope: u32,
    scopes: Vec<u32>,
    loops: Vec<Loop>,
    // Annex B permits a simple catch parameter to be redeclared with `var`
    // in its block. Those declaration writes target the catch binding.
    catch_var_slots: Vec<HashMap<String, u32>>,
    max_bytecode_bytes: u32,
    function: bool,
    local_scope: usize,
    with_depth: usize,
}

#[derive(Clone, Copy)]
struct FunctionCompileOptions {
    constructible: bool,
    force_strict: bool,
    class_constructor: bool,
    derived_constructor: bool,
    default_derived_constructor: bool,
    class_method: bool,
}

impl FunctionCompileOptions {
    fn class_method() -> Self {
        Self {
            constructible: false,
            force_strict: true,
            class_constructor: false,
            derived_constructor: false,
            default_derived_constructor: false,
            class_method: true,
        }
    }
}

impl Compiler {
    fn offset(&self) -> Result<u32, CompileError> {
        u32::try_from(self.bytecode.code.len()).map_err(|_| CompileError::ProgramTooLarge)
    }

    fn emit(&mut self, opcode: Opcode, operand: u32) -> Result<usize, CompileError> {
        let offset = self.offset()? as usize;
        if offset
            .checked_add(opcode.width())
            .is_none_or(|end| end > self.max_bytecode_bytes as usize)
        {
            return Err(CompileError::ProgramTooLarge);
        }
        self.bytecode.code.push(opcode as u8);
        if opcode.width() == 5 {
            self.bytecode.code.extend_from_slice(&operand.to_le_bytes());
        }
        Ok(offset)
    }

    fn patch(&mut self, jump: usize, target: u32) {
        self.bytecode.code[jump + 1..jump + 5].copy_from_slice(&target.to_le_bytes());
    }

    fn constant(&mut self, value: Value) -> Result<(), CompileError> {
        let index = u32::try_from(self.bytecode.constants.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.constants.push(value);
        self.emit(Opcode::Constant, index)?;
        Ok(())
    }

    fn enter_scope(
        &mut self,
        lexical: Vec<(String, DeclKind)>,
        vars: &BTreeSet<String>,
        global: bool,
    ) -> Result<(), CompileError> {
        let mut names = HashMap::new();
        let mut slots = Vec::new();
        let declarations = vars
            .iter()
            .filter(|_| global)
            .map(|name| (name.clone(), DeclKind::Var))
            .chain(lexical);
        for (name, kind) in declarations {
            if self.bytecode.strict
                && matches!(
                    name.as_str(),
                    "implements"
                        | "interface"
                        | "package"
                        | "private"
                        | "protected"
                        | "public"
                        | "static"
                        | "yield"
                )
            {
                return Err(CompileError::InvalidSyntax(
                    "strict mode binding uses a reserved word",
                ));
            }
            if names.contains_key(&name) || (kind != DeclKind::Var && vars.contains(&name)) {
                return Err(CompileError::DuplicateBinding(name));
            }
            let slot = u32::try_from(self.bytecode.bindings.len())
                .map_err(|_| CompileError::ProgramTooLarge)?;
            self.bytecode.bindings.push(Binding {
                name: name.clone(),
                mutable: kind != DeclKind::Const,
                strict_immutable: kind == DeclKind::Const,
                lexical: kind != DeclKind::Var,
                catch_parameter: false,
            });
            names.insert(name, slot);
            slots.push(slot);
        }
        let scope =
            u32::try_from(self.bytecode.scopes.len()).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.scopes.push(slots);
        self.names.push(names);
        self.scopes.push(scope);
        self.emit(Opcode::EnterScope, scope)?;
        Ok(())
    }

    fn leave_scope(&mut self) -> Result<(), CompileError> {
        let scope = self.scopes.pop().expect("compiler scopes are balanced");
        self.names.pop();
        self.emit(Opcode::LeaveScope, scope)?;
        Ok(())
    }

    fn resolve(&self, name: &str) -> Option<u32> {
        self.names
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn resolve_private_name(&self, name: &str) -> Result<u32, CompileError> {
        let binding = self
            .private_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .ok_or(CompileError::InvalidSyntax(
                "private name is not declared in an enclosing class",
            ))?;
        self.resolve(binding).ok_or(CompileError::InvalidSyntax(
            "private name binding is not available in this function",
        ))
    }

    /// Annex B creates a var binding in the enclosing variable environment
    /// for eligible sloppy block functions. The block function itself remains
    /// lexical, so each time its block is evaluated the function value is
    /// copied into that outer var binding.
    fn annex_b_outer_var_slot(&self, slot: u32) -> Option<u32> {
        if self.bytecode.strict || !self.bytecode.bindings[slot as usize].lexical {
            return None;
        }
        let name = &self.bytecode.bindings[slot as usize].name;
        for scope in self.names[..self.names.len() - 1].iter().rev() {
            let Some(&candidate) = scope.get(name) else {
                continue;
            };
            let binding = &self.bytecode.bindings[candidate as usize];
            if !binding.lexical {
                return Some(candidate);
            }
            // Annex B.3.5 permits the function's var binding to pass through
            // a simple catch parameter. Other lexical bindings prevent the
            // legacy outer var from being introduced.
            if self
                .catch_var_slots
                .iter()
                .any(|slots| slots.get(name) == Some(&candidate))
            {
                continue;
            }
            return None;
        }
        None
    }
}

fn strict_body(body: &[Stmt]) -> bool {
    body.iter()
        .take_while(|stmt| matches!(stmt, Stmt::Expr(Expr::String(_))))
        .any(|stmt| matches!(stmt, Stmt::Expr(Expr::String(s)) if s == "use strict"))
}

fn strict_assignment_to_restricted_name(statements: &[Stmt]) -> bool {
    statements.iter().any(strict_assignment_in_statement)
}

fn strict_assignment_in_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::Empty
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::FunctionDecl(_)
        | Stmt::ModuleDefaultFunction { .. }
        | Stmt::ClassDecl(_)
        | Stmt::ClassPrivateBrand(_) => false,
        Stmt::Expr(expr) | Stmt::Throw(expr) => strict_assignment_in_expression(expr),
        Stmt::Block(statements) => strict_assignment_to_restricted_name(statements),
        Stmt::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            strict_assignment_in_pattern(&declaration.pattern)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(strict_assignment_in_expression)
        }),
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            strict_assignment_in_expression(test)
                || strict_assignment_in_statement(consequent)
                || alternate
                    .as_deref()
                    .is_some_and(strict_assignment_in_statement)
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            init.as_ref().is_some_and(strict_assignment_in_for_init)
                || test.as_ref().is_some_and(strict_assignment_in_expression)
                || update.as_ref().is_some_and(strict_assignment_in_expression)
                || strict_assignment_in_statement(body)
        }
        Stmt::ForIn { left, right, body }
        | Stmt::ForOf {
            left, right, body, ..
        } => {
            strict_assignment_in_for_head(left)
                || strict_assignment_in_expression(right)
                || strict_assignment_in_statement(body)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            strict_assignment_in_expression(test) || strict_assignment_in_statement(body)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            strict_assignment_in_expression(discriminant)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(strict_assignment_in_expression)
                        || strict_assignment_to_restricted_name(&case.consequent)
                })
        }
        Stmt::Labelled { item, .. } | Stmt::ClassField(item) => {
            strict_assignment_in_statement(item)
        }
        Stmt::Return(value) => value.as_ref().is_some_and(strict_assignment_in_expression),
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            strict_assignment_to_restricted_name(block)
                || handler.as_ref().is_some_and(|handler| {
                    handler
                        .param
                        .as_ref()
                        .is_some_and(strict_assignment_in_pattern)
                        || strict_assignment_to_restricted_name(&handler.body)
                })
                || finalizer
                    .as_deref()
                    .is_some_and(strict_assignment_to_restricted_name)
        }
        Stmt::With { object, body } => {
            strict_assignment_in_expression(object) || strict_assignment_in_statement(body)
        }
    }
}

fn strict_assignment_in_for_init(init: &ForInit) -> bool {
    match init {
        ForInit::Expr(expression) => strict_assignment_in_expression(expression),
        ForInit::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            strict_assignment_in_pattern(&declaration.pattern)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(strict_assignment_in_expression)
        }),
    }
}

fn strict_assignment_in_for_head(head: &ForHead) -> bool {
    match head {
        ForHead::Decl(_, pattern) => strict_assignment_in_pattern(pattern),
        ForHead::AnnexBVarInit(pattern, initializer) => {
            strict_assignment_in_pattern(pattern) || strict_assignment_in_expression(initializer)
        }
        ForHead::Pattern(pattern) => pattern_names(pattern)
            .iter()
            .any(|name| restricted_name(name)),
        ForHead::Expr(expression) => strict_assignment_in_expression(expression),
    }
}

fn strict_assignment_in_pattern(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Identifier(_) => false,
        Pattern::Array(elements) => elements.iter().flatten().any(|element| {
            strict_assignment_in_pattern(&element.pattern)
                || element
                    .default
                    .as_ref()
                    .is_some_and(strict_assignment_in_expression)
        }),
        Pattern::Object(properties) => properties.iter().any(|property| match property {
            ObjectPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                strict_assignment_in_property_key(key)
                    || strict_assignment_in_pattern(value)
                    || default
                        .as_ref()
                        .is_some_and(strict_assignment_in_expression)
            }
            ObjectPatternProp::Rest(pattern) => strict_assignment_in_pattern(pattern),
        }),
    }
}

fn strict_assignment_in_assignment_pattern(pattern: &AssignmentPattern) -> bool {
    match pattern {
        AssignmentPattern::Target(target) => strict_assignment_target(target),
        AssignmentPattern::Array(elements) => elements.iter().flatten().any(|element| {
            strict_assignment_in_assignment_pattern(&element.pattern)
                || element
                    .default
                    .as_ref()
                    .is_some_and(strict_assignment_in_expression)
        }),
        AssignmentPattern::Object(properties) => properties.iter().any(|property| match property {
            AssignmentPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                strict_assignment_in_property_key(key)
                    || strict_assignment_in_assignment_pattern(value)
                    || default
                        .as_ref()
                        .is_some_and(strict_assignment_in_expression)
            }
            AssignmentPatternProp::Rest(pattern) => {
                strict_assignment_in_assignment_pattern(pattern)
            }
        }),
    }
}

fn strict_assignment_in_property_key(key: &PropertyKey) -> bool {
    matches!(key, PropertyKey::Computed(expression) if strict_assignment_in_expression(expression))
}

fn strict_assignment_target(expression: &Expr) -> bool {
    match expression {
        Expr::Identifier(name) => restricted_name(name),
        Expr::Parenthesized(expression) => strict_assignment_target(expression),
        expression => strict_assignment_in_expression(expression),
    }
}

fn restricted_name(name: &str) -> bool {
    matches!(name, "eval" | "arguments")
}

fn strict_assignment_in_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Number(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::Identifier(_)
        | Expr::RegExp { .. }
        | Expr::Super
        | Expr::NewTarget
        | Expr::ImportMeta
        | Expr::Function(_)
        | Expr::Class(_) => false,
        Expr::Parenthesized(expression) => strict_assignment_in_expression(expression),
        Expr::Template { expressions, .. } => {
            expressions.iter().any(strict_assignment_in_expression)
        }
        Expr::TaggedTemplate {
            tag, expressions, ..
        } => {
            strict_assignment_in_expression(tag)
                || expressions.iter().any(strict_assignment_in_expression)
        }
        Expr::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayElement::Normal(expression) | ArrayElement::Spread(expression) => {
                strict_assignment_in_expression(expression)
            }
        }),
        Expr::Object(properties) => properties.iter().any(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                strict_assignment_in_property_key(key) || strict_assignment_in_expression(value)
            }
            ObjectProp::Spread(expression) => strict_assignment_in_expression(expression),
            ObjectProp::Method { key, .. } | ObjectProp::Accessor { key, .. } => {
                strict_assignment_in_property_key(key)
            }
        }),
        Expr::Yield { value, .. } => value
            .as_deref()
            .is_some_and(strict_assignment_in_expression),
        Expr::Await(expression)
        | Expr::DynamicImport(expression)
        | Expr::Unary {
            arg: expression, ..
        } => strict_assignment_in_expression(expression),
        Expr::Update { arg, .. } => strict_assignment_target(arg),
        Expr::Arrow { params, body, .. } => {
            params.iter().any(|param| {
                strict_assignment_in_pattern(&param.pattern)
                    || param
                        .default
                        .as_ref()
                        .is_some_and(strict_assignment_in_expression)
            }) || match body {
                ArrowBody::Expr(expression) => strict_assignment_in_expression(expression),
                ArrowBody::Block(statements) => strict_assignment_to_restricted_name(statements),
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            strict_assignment_in_expression(left) || strict_assignment_in_expression(right)
        }
        Expr::Sequence(expressions) => expressions.iter().any(strict_assignment_in_expression),
        Expr::Assign { target, value, .. } => {
            strict_assignment_target(target) || strict_assignment_in_expression(value)
        }
        Expr::DestructureAssign { pattern, value } => {
            strict_assignment_in_assignment_pattern(pattern)
                || strict_assignment_in_expression(value)
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            strict_assignment_in_expression(test)
                || strict_assignment_in_expression(consequent)
                || strict_assignment_in_expression(alternate)
        }
        Expr::Call { callee, args }
        | Expr::OptionalCall { callee, args }
        | Expr::New { callee, args } => {
            strict_assignment_in_expression(callee)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expression) | Argument::Spread(expression) => {
                        strict_assignment_in_expression(expression)
                    }
                })
        }
        Expr::Member {
            object, property, ..
        } => strict_assignment_in_expression(object) || strict_assignment_in_expression(property),
        Expr::PrivateIn { object, .. } => strict_assignment_in_expression(object),
        Expr::OptionalMember {
            object, property, ..
        } => strict_assignment_in_expression(object) || strict_assignment_in_expression(property),
    }
}

fn validate_function_early_errors(
    function: &Function,
    strict: bool,
    name_is_binding: bool,
    arrow: bool,
) -> Result<(), CompileError> {
    let simple = function.params.iter().all(|param| {
        !param.rest && param.default.is_none() && matches!(param.pattern, Pattern::Identifier(_))
    });
    if strict_body(&function.body) && !simple {
        return Err(CompileError::InvalidSyntax(
            "a function with non-simple parameters cannot contain a use strict directive",
        ));
    }
    let names: Vec<_> = function
        .params
        .iter()
        .flat_map(|param| pattern_names(&param.pattern))
        .collect();
    // Arrow parameter lists are `UniqueFormalParameters` even in a sloppy
    // surrounding script. Ordinary sloppy functions retain the Annex B
    // duplicate-name allowance for a simple list.
    if strict || !simple || arrow {
        let mut unique = BTreeSet::new();
        if names.iter().any(|name| !unique.insert(name)) {
            return Err(CompileError::InvalidSyntax("duplicate parameter name"));
        }
    }
    if strict
        && function
            .name
            .iter()
            .filter(|_| name_is_binding)
            .chain(names.iter())
            .any(|name| matches!(name.as_str(), "eval" | "arguments" | "yield"))
    {
        return Err(CompileError::InvalidSyntax(
            "strict functions cannot bind eval, arguments, or yield",
        ));
    }
    if function.generator && names.iter().any(|name| name == "yield") {
        return Err(CompileError::InvalidSyntax(
            "generator parameters cannot bind yield",
        ));
    }
    if strict && strict_assignment_to_restricted_name(&function.body) {
        return Err(CompileError::InvalidSyntax(
            "strict code cannot assign to eval or arguments",
        ));
    }
    Ok(())
}

fn class_property_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::Identifier(name) => Some(name.clone()),
        PropertyKey::String(name) => Some(name.to_utf8().unwrap_or_default()),
        PropertyKey::Number(number) => Some(number.to_string()),
        PropertyKey::Computed(_) => None,
    }
}

fn private_class_name(key: &PropertyKey) -> Option<&str> {
    match key {
        PropertyKey::Identifier(name) => name.strip_prefix('#'),
        _ => None,
    }
}

/// Decode a compiler-private owner binding while reconstructing the private
/// environment for direct eval.  The source name follows an unambiguous
/// static/instance marker; it may otherwise contain arbitrary identifier
/// characters (including underscores).
fn private_owner_binding_name(binding: &str) -> Option<(u32, String)> {
    let suffix = binding.strip_prefix(PRIVATE_OWNER_BINDING_PREFIX)?;
    let (scope, name) = suffix
        .split_once("_static_")
        .or_else(|| suffix.split_once("_instance_"))?;
    Some((scope.parse().ok()?, name.to_owned()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateDeclarationKind {
    FieldOrMethod,
    Accessor { getter: bool },
}

/// Collect own private names and establish the class-element duplicate early
/// errors before any bytecode is emitted.  A getter/setter pair is the only
/// permitted repeated private name, and both halves must have the same
/// static-ness.
fn class_private_declarations(class: &Class) -> Result<Vec<(String, bool)>, CompileError> {
    let mut declarations = Vec::new();
    let mut seen: HashMap<String, (bool, PrivateDeclarationKind)> = HashMap::new();
    for element in &class.elements {
        let (key, is_static, kind) = match element {
            ClassElement::Method { key, is_static, .. } => {
                (key, *is_static, PrivateDeclarationKind::FieldOrMethod)
            }
            ClassElement::Accessor {
                key,
                getter,
                is_static,
                ..
            } => (
                key,
                *is_static,
                PrivateDeclarationKind::Accessor { getter: *getter },
            ),
            ClassElement::Field { key, is_static, .. } => {
                (key, *is_static, PrivateDeclarationKind::FieldOrMethod)
            }
            ClassElement::StaticBlock(_) => continue,
        };
        let Some(name) = private_class_name(key) else {
            continue;
        };
        let name = name.to_owned();
        match seen.get(&name).copied() {
            None => {
                seen.insert(name.clone(), (is_static, kind));
                declarations.push((name, is_static));
            }
            Some((previous_static, PrivateDeclarationKind::Accessor { getter: previous }))
                if previous_static == is_static
                    && matches!(kind, PrivateDeclarationKind::Accessor { getter } if getter != previous) =>
                {}
            Some(_) => {
                return Err(CompileError::InvalidSyntax(
                    "duplicate private name in class body",
                ));
            }
        }
    }
    Ok(declarations)
}

fn private_member_name(expr: &Expr) -> Option<&str> {
    let Expr::Member {
        property,
        computed: false,
        ..
    } = expr
    else {
        return None;
    };
    let Expr::Identifier(name) = property.as_ref() else {
        return None;
    };
    name.strip_prefix('#')
}

fn undefined_expression() -> Expr {
    Expr::Unary {
        op: UnaryOp::Void,
        arg: Box::new(Expr::Number(0.0)),
    }
}

fn class_instance_field(key: &PropertyKey, initializer: Option<&Expr>) -> Stmt {
    let (property, computed) = match key {
        PropertyKey::Identifier(name) => (Expr::Identifier(name.clone()), false),
        PropertyKey::String(name) => (Expr::String(name.clone()), true),
        PropertyKey::Number(number) => (Expr::Number(*number), true),
        PropertyKey::Computed(expression) => ((*expression.clone()), true),
    };
    Stmt::ClassField(Box::new(Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(Expr::Member {
            object: Box::new(Expr::This),
            property: Box::new(property),
            computed,
        }),
        value: Box::new(initializer.cloned().unwrap_or_else(undefined_expression)),
    })))
}

/// The VM establishes `this` while executing `super()`. For explicit derived
/// constructors, fields therefore follow the first direct constructor call.
/// A direct top-level call gets the exact specified placement. When the call
/// is nested in a closure or control-flow expression, preserve the constructor
/// body and defer the field list to its normal completion. This keeps `this`
/// uninitialized until the nested `super()` actually executes, rather than
/// rejecting otherwise valid derived class syntax during compilation.
fn derived_constructor_body(
    mut body: Vec<Stmt>,
    fields: Vec<Stmt>,
) -> Result<Vec<Stmt>, CompileError> {
    if fields.is_empty() {
        return Ok(body);
    }
    if let Some(index) = body.iter().position(|statement| {
        matches!(statement, Stmt::Expr(Expr::Call { callee, .. }) if matches!(&**callee, Expr::Super))
    }) {
        body.splice(index + 1..index + 1, fields);
    } else {
        body.extend(fields);
    }
    Ok(body)
}

fn binary_opcode(op: BinaryOp) -> Result<Opcode, CompileError> {
    Ok(match op {
        BinaryOp::Add => Opcode::Add,
        BinaryOp::Sub => Opcode::Subtract,
        BinaryOp::Mul => Opcode::Multiply,
        BinaryOp::Exponent => Opcode::Exponentiate,
        BinaryOp::Div => Opcode::Divide,
        BinaryOp::Mod => Opcode::Remainder,
        BinaryOp::ShiftLeft => Opcode::ShiftLeft,
        BinaryOp::ShiftRight => Opcode::ShiftRight,
        BinaryOp::UnsignedShiftRight => Opcode::UnsignedShiftRight,
        BinaryOp::BitAnd => Opcode::BitAnd,
        BinaryOp::BitXor => Opcode::BitXor,
        BinaryOp::BitOr => Opcode::BitOr,
        BinaryOp::StrictEq => Opcode::StrictEqual,
        BinaryOp::StrictNotEq => Opcode::StrictNotEqual,
        BinaryOp::Eq => Opcode::Equal,
        BinaryOp::NotEq => Opcode::NotEqual,
        BinaryOp::Lt => Opcode::Less,
        BinaryOp::Gt => Opcode::Greater,
        BinaryOp::LtEq => Opcode::LessEqual,
        BinaryOp::GtEq => Opcode::GreaterEqual,
        BinaryOp::Instanceof => Opcode::Instanceof,
        BinaryOp::In => Opcode::In,
    })
}

fn compound_assignment_opcode(op: AssignOp) -> Option<Opcode> {
    match op {
        AssignOp::Assign => None,
        AssignOp::AddAssign => Some(Opcode::Add),
        AssignOp::SubAssign => Some(Opcode::Subtract),
        AssignOp::MulAssign => Some(Opcode::Multiply),
        AssignOp::ExponentAssign => Some(Opcode::Exponentiate),
        AssignOp::DivAssign => Some(Opcode::Divide),
        AssignOp::ModAssign => Some(Opcode::Remainder),
        AssignOp::ShiftLeftAssign => Some(Opcode::ShiftLeft),
        AssignOp::ShiftRightAssign => Some(Opcode::ShiftRight),
        AssignOp::UnsignedShiftRightAssign => Some(Opcode::UnsignedShiftRight),
        AssignOp::BitAndAssign => Some(Opcode::BitAnd),
        AssignOp::BitXorAssign => Some(Opcode::BitXor),
        AssignOp::BitOrAssign => Some(Opcode::BitOr),
        AssignOp::LogicalAndAssign | AssignOp::LogicalOrAssign | AssignOp::NullishAssign => None,
    }
}

fn is_logical_assignment(op: AssignOp) -> bool {
    matches!(
        op,
        AssignOp::LogicalAndAssign | AssignOp::LogicalOrAssign | AssignOp::NullishAssign
    )
}

fn pattern_names(pattern: &Pattern) -> Vec<String> {
    match pattern {
        Pattern::Identifier(name) => vec![name.clone()],
        Pattern::Array(elements) => elements
            .iter()
            .flatten()
            .flat_map(|element| pattern_names(&element.pattern))
            .collect(),
        Pattern::Object(properties) => properties
            .iter()
            .flat_map(|property| match property {
                ObjectPatternProp::KeyValue { value, .. } | ObjectPatternProp::Rest(value) => {
                    pattern_names(value)
                }
            })
            .collect(),
    }
}

fn declarations_names(
    kind: DeclKind,
    declarations: &[VarDeclarator],
) -> Result<Vec<(String, DeclKind)>, CompileError> {
    let mut names = Vec::new();
    for declaration in declarations {
        for name in pattern_names(&declaration.pattern) {
            names.push((name, kind));
        }
    }
    Ok(names)
}

fn lexical_names(statements: &[Stmt]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    let mut names = Vec::new();
    for statement in statements {
        if let Stmt::VarDecl(kind, declarations) = statement {
            if *kind != DeclKind::Var {
                names.extend(declarations_names(*kind, declarations)?);
            }
        }
        if let Stmt::ClassDecl(class) = statement {
            names.push((
                class.name.clone().expect("class declaration has a name"),
                DeclKind::Let,
            ));
        }
    }
    Ok(names)
}

fn block_lexical_names(statements: &[Stmt]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    let mut names = lexical_names(statements)?;
    for statement in statements {
        if let Stmt::FunctionDecl(function) = statement {
            names.push((
                function
                    .name
                    .clone()
                    .expect("function declaration has a name"),
                DeclKind::Let,
            ));
        }
    }
    Ok(names)
}

fn switch_lexical_names(cases: &[SwitchCase]) -> Result<Vec<(String, DeclKind)>, CompileError> {
    Ok(switch_case_lexical_declarations(cases)?
        .into_iter()
        .map(|(name, kind, _)| (name, kind))
        .collect())
}

fn switch_case_lexical_declarations(
    cases: &[SwitchCase],
) -> Result<Vec<(String, DeclKind, bool)>, CompileError> {
    let mut lexical = Vec::new();
    for case in cases {
        for statement in &case.consequent {
            match statement {
                Stmt::VarDecl(kind, declarations) if *kind != DeclKind::Var => {
                    lexical.extend(
                        declarations_names(*kind, declarations)?
                            .into_iter()
                            .map(|(name, kind)| (name, kind, false)),
                    );
                }
                Stmt::ClassDecl(class) => lexical.push((
                    class.name.clone().expect("class declaration has a name"),
                    DeclKind::Let,
                    false,
                )),
                Stmt::FunctionDecl(function) => lexical.push((
                    function.name.clone().expect("declaration has a name"),
                    DeclKind::Let,
                    !function.generator && !function.is_async,
                )),
                _ => {}
            }
        }
    }
    Ok(lexical)
}

/// CaseBlock has its own static declaration rules. Function declarations are
/// lexical there, unlike at script/function scope; Annex B preserves the
/// duplicate ordinary-function exception only for sloppy code.
fn validate_switch_case_declarations(
    cases: &[SwitchCase],
    strict: bool,
) -> Result<(), CompileError> {
    let lexical = switch_case_lexical_declarations(cases)?;

    for (index, (name, _, annex_b_function)) in lexical.iter().enumerate() {
        for (other, _, other_annex_b_function) in &lexical[..index] {
            if name == other && (strict || !annex_b_function || !other_annex_b_function) {
                return Err(CompileError::InvalidSyntax(
                    "duplicate lexical declaration in switch statement",
                ));
            }
        }
    }

    let vars = switch_var_names(cases)?;
    if lexical.iter().any(|(name, _, _)| vars.contains(name)) {
        return Err(CompileError::InvalidSyntax(
            "a switch lexical declaration conflicts with a var declaration",
        ));
    }
    Ok(())
}

/// `CatchParameter` has an additional early error against lexical names in
/// its directly nested block. Function declarations participate even though
/// their broader binding behavior is handled separately for Annex B.
fn catch_lexical_names(statements: &[Stmt]) -> Vec<String> {
    let mut names = Vec::new();
    for statement in statements {
        match statement {
            Stmt::VarDecl(kind, declarations) if *kind != DeclKind::Var => {
                names.extend(
                    declarations
                        .iter()
                        .flat_map(|declaration| pattern_names(&declaration.pattern)),
                );
            }
            Stmt::FunctionDecl(function) => {
                names.push(function.name.clone().expect("declaration has a name"))
            }
            Stmt::ClassDecl(class) => {
                names.push(class.name.clone().expect("class declaration has a name"))
            }
            _ => {}
        }
    }
    names
}

fn top_level_var_names(statements: &[Stmt]) -> Result<BTreeSet<String>, CompileError> {
    let mut names = var_names(statements)?;
    names.extend(statements.iter().filter_map(|statement| match statement {
        Stmt::FunctionDecl(function) => function.name.clone(),
        _ => None,
    }));
    Ok(names)
}

fn var_names(statements: &[Stmt]) -> Result<BTreeSet<String>, CompileError> {
    let mut names = BTreeSet::new();
    let mut pending: Vec<_> = statements.iter().collect();
    while let Some(statement) = pending.pop() {
        match statement {
            Stmt::VarDecl(DeclKind::Var, declarations) => {
                for declaration in declarations {
                    names.extend(pattern_names(&declaration.pattern));
                }
            }
            Stmt::Block(body) => pending.extend(body),
            Stmt::If {
                consequent,
                alternate,
                ..
            } => {
                pending.push(consequent);
                if let Some(alternate) = alternate {
                    pending.push(alternate);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::With { body, .. } => {
                pending.push(body)
            }
            Stmt::Labelled { item, .. } => pending.push(item),
            Stmt::For { init, body, .. } => {
                if let Some(ForInit::VarDecl(DeclKind::Var, declarations)) = init {
                    for declaration in declarations {
                        names.extend(pattern_names(&declaration.pattern));
                    }
                }
                pending.push(body);
            }
            Stmt::ForIn { left, body, .. } | Stmt::ForOf { left, body, .. } => {
                if let ForHead::Decl(DeclKind::Var, pattern) | ForHead::AnnexBVarInit(pattern, _) =
                    left
                {
                    names.extend(pattern_names(pattern));
                }
                pending.push(body);
            }
            Stmt::Switch { cases, .. } => {
                pending.extend(cases.iter().flat_map(|case| case.consequent.iter()))
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                pending.extend(block);
                if let Some(handler) = handler {
                    pending.extend(&handler.body);
                }
                if let Some(finalizer) = finalizer {
                    pending.extend(finalizer);
                }
            }
            _ => {}
        }
    }
    Ok(names)
}

fn switch_var_names(cases: &[SwitchCase]) -> Result<BTreeSet<String>, CompileError> {
    var_names(
        &cases
            .iter()
            .flat_map(|case| case.consequent.iter().cloned())
            .collect::<Vec<_>>(),
    )
}

fn annex_b_function_names(
    statements: &[Stmt],
    root_lexical: &[(String, DeclKind)],
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let blocked = root_lexical.iter().map(|(name, _)| name.clone()).collect();
    collect_annex_b_function_names(statements, &blocked, &mut names);
    names
}

fn is_annex_b_function(function: &Function) -> bool {
    !function.generator && !function.is_async
}

fn insert_annex_b_function_name(
    function: &Function,
    blocked: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
) {
    if is_annex_b_function(function) {
        let name = function
            .name
            .clone()
            .expect("function declaration has a name");
        if !blocked.contains(&name) {
            names.insert(name);
        }
    }
}

/// Annex B only introduces the outer var when replacing the block-level
/// function with `var f` would not cause a script early error. Track lexical
/// ancestors while collecting candidates so a nested `let f`, loop binding or
/// destructuring catch parameter suppresses that legacy outer binding.
fn collect_annex_b_function_names(
    statements: &[Stmt],
    blocked: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
) {
    for statement in statements {
        match statement {
            Stmt::Block(body) => {
                for statement in body {
                    if let Stmt::FunctionDecl(function) = statement {
                        insert_annex_b_function_name(function, blocked, names);
                    }
                }
                let mut nested_blocked = blocked.clone();
                extend_block_lexical_names(&mut nested_blocked, body);
                collect_annex_b_function_names(body, &nested_blocked, names);
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for statement in &case.consequent {
                        if let Stmt::FunctionDecl(function) = statement {
                            insert_annex_b_function_name(function, blocked, names);
                        }
                    }
                }
                let mut nested_blocked = blocked.clone();
                for case in cases {
                    extend_block_lexical_names(&mut nested_blocked, &case.consequent);
                }
                for case in cases {
                    collect_annex_b_function_names(&case.consequent, &nested_blocked, names);
                }
            }
            Stmt::If {
                consequent,
                alternate,
                ..
            } => {
                if let Stmt::FunctionDecl(function) = &**consequent {
                    insert_annex_b_function_name(function, blocked, names);
                } else {
                    collect_annex_b_function_names(
                        std::slice::from_ref(&**consequent),
                        blocked,
                        names,
                    );
                }
                if let Some(alternate) = alternate {
                    if let Stmt::FunctionDecl(function) = &**alternate {
                        insert_annex_b_function_name(function, blocked, names);
                    } else {
                        collect_annex_b_function_names(
                            std::slice::from_ref(&**alternate),
                            blocked,
                            names,
                        );
                    }
                }
            }
            Stmt::For { init, body, .. } => {
                let mut nested_blocked = blocked.clone();
                if let Some(ForInit::VarDecl(kind, declarations)) = init {
                    if *kind != DeclKind::Var {
                        nested_blocked.extend(
                            declarations
                                .iter()
                                .flat_map(|declaration| pattern_names(&declaration.pattern)),
                        );
                    }
                }
                collect_annex_b_function_names(
                    std::slice::from_ref(&**body),
                    &nested_blocked,
                    names,
                );
            }
            Stmt::ForIn { left, body, .. } | Stmt::ForOf { left, body, .. } => {
                let mut nested_blocked = blocked.clone();
                if let ForHead::Decl(kind, pattern) = left {
                    if *kind != DeclKind::Var {
                        nested_blocked.extend(pattern_names(pattern));
                    }
                }
                collect_annex_b_function_names(
                    std::slice::from_ref(&**body),
                    &nested_blocked,
                    names,
                );
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::With { body, .. }
            | Stmt::Labelled { item: body, .. } => {
                collect_annex_b_function_names(std::slice::from_ref(&**body), blocked, names)
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                collect_annex_b_function_names(block, blocked, names);
                if let Some(handler) = handler {
                    let mut nested_blocked = blocked.clone();
                    // Annex B.3.5 makes a simple catch identifier a special
                    // case: the synthesized var passes through it. A pattern
                    // parameter still makes the replacement an early error.
                    if let Some(parameter) = &handler.param {
                        if !matches!(parameter, Pattern::Identifier(_)) {
                            nested_blocked.extend(pattern_names(parameter));
                        }
                    }
                    collect_annex_b_function_names(&handler.body, &nested_blocked, names);
                }
                if let Some(finalizer) = finalizer {
                    collect_annex_b_function_names(finalizer, blocked, names);
                }
            }
            _ => {}
        }
    }
}

/// An OptionalChain includes every unparenthesized member access and call
/// built on top of its first optional suffix.  Parentheses intentionally end
/// the chain, so `(a?.b).c` still attempts the final ordinary access.
fn optional_chain_root(expr: &Expr) -> bool {
    match expr {
        Expr::OptionalMember { .. } | Expr::OptionalCall { .. } => true,
        Expr::Member { object, .. } => optional_chain_root(object),
        Expr::Call { callee, .. } => optional_chain_root(callee),
        _ => false,
    }
}

fn extend_block_lexical_names(blocked: &mut BTreeSet<String>, statements: &[Stmt]) {
    for statement in statements {
        match statement {
            Stmt::VarDecl(kind, declarations) if *kind != DeclKind::Var => blocked.extend(
                declarations
                    .iter()
                    .flat_map(|declaration| pattern_names(&declaration.pattern)),
            ),
            Stmt::ClassDecl(class) => {
                blocked.insert(class.name.clone().expect("class declaration has a name"));
            }
            Stmt::FunctionDecl(function) => {
                blocked.insert(
                    function
                        .name
                        .clone()
                        .expect("function declaration has a name"),
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computed_class_keys_do_not_have_constructor_names() {
        assert_eq!(
            class_property_name(&PropertyKey::Computed(Box::new(Expr::Identifier(
                "key".into()
            )))),
            None
        );
    }
}
