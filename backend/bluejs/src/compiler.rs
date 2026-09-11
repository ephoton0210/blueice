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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Unsupported(&'static str),
    DuplicateBinding(String),
    InvalidSyntax(&'static str),
    ProgramTooLarge,
}

/// Performs the grammar's lexical PrivateEnvironment checks without lowering
/// the program.  The parser uses this for parse-only Test262 cases; the
/// compiler repeats the lookup while assigning hidden owner bindings.
pub(crate) fn validate_private_early_errors(program: &Program) -> Result<(), CompileError> {
    validate_private_statements(&program.body, &HashSet::new())
}

fn missing_private_name() -> CompileError {
    CompileError::InvalidSyntax("private name is not declared in an enclosing class")
}

fn validate_private_name(name: &str, names: &HashSet<String>) -> Result<(), CompileError> {
    names
        .contains(name)
        .then_some(())
        .ok_or_else(missing_private_name)
}

fn validate_private_statements(
    statements: &[Stmt],
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    for statement in statements {
        validate_private_statement(statement, names)?;
    }
    Ok(())
}

fn validate_private_statement(
    statement: &Stmt,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match statement {
        Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) | Stmt::ClassPrivateBrand(_) => Ok(()),
        Stmt::Expr(expr) | Stmt::Throw(expr) => validate_private_expression(expr, names),
        Stmt::Block(statements) => validate_private_statements(statements, names),
        Stmt::VarDecl(_, declarations) => {
            for declaration in declarations {
                validate_private_pattern(&declaration.pattern, names)?;
                if let Some(initializer) = &declaration.init {
                    validate_private_expression(initializer, names)?;
                }
            }
            Ok(())
        }
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            validate_private_expression(test, names)?;
            validate_private_statement(consequent, names)?;
            if let Some(alternate) = alternate {
                validate_private_statement(alternate, names)?;
            }
            Ok(())
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init) = init {
                validate_private_for_init(init, names)?;
            }
            for expression in [test.as_ref(), update.as_ref()].into_iter().flatten() {
                validate_private_expression(expression, names)?;
            }
            validate_private_statement(body, names)
        }
        Stmt::ForIn { left, right, body }
        | Stmt::ForOf {
            left, right, body, ..
        } => {
            validate_private_for_head(left, names)?;
            validate_private_expression(right, names)?;
            validate_private_statement(body, names)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            validate_private_expression(test, names)?;
            validate_private_statement(body, names)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            validate_private_expression(discriminant, names)?;
            for case in cases {
                if let Some(test) = &case.test {
                    validate_private_expression(test, names)?;
                }
                validate_private_statements(&case.consequent, names)?;
            }
            Ok(())
        }
        Stmt::Labelled { item, .. } => validate_private_statement(item, names),
        Stmt::Return(value) => value
            .as_ref()
            .map_or(Ok(()), |value| validate_private_expression(value, names)),
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            validate_private_statements(block, names)?;
            if let Some(handler) = handler {
                if let Some(param) = &handler.param {
                    validate_private_pattern(param, names)?;
                }
                validate_private_statements(&handler.body, names)?;
            }
            if let Some(finalizer) = finalizer {
                validate_private_statements(finalizer, names)?;
            }
            Ok(())
        }
        Stmt::With { object, body } => {
            validate_private_expression(object, names)?;
            validate_private_statement(body, names)
        }
        Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
            validate_private_function(function, names)
        }
        Stmt::ClassDecl(class) => validate_private_class(class, names),
        Stmt::ClassField(statement) => validate_private_statement(statement, names),
    }
}

fn validate_private_for_init(init: &ForInit, names: &HashSet<String>) -> Result<(), CompileError> {
    match init {
        ForInit::Expr(expression) => validate_private_expression(expression, names),
        ForInit::VarDecl(_, declarations) => declarations.iter().try_for_each(|declaration| {
            validate_private_pattern(&declaration.pattern, names)?;
            declaration.init.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
    }
}

fn validate_private_for_head(head: &ForHead, names: &HashSet<String>) -> Result<(), CompileError> {
    match head {
        ForHead::Decl(_, pattern) | ForHead::Pattern(pattern) => {
            validate_private_pattern(pattern, names)
        }
        ForHead::AnnexBVarInit(pattern, initializer) => {
            validate_private_pattern(pattern, names)?;
            validate_private_expression(initializer, names)
        }
        ForHead::Expr(expression) => validate_private_expression(expression, names),
    }
}

fn validate_private_class(class: &Class, names: &HashSet<String>) -> Result<(), CompileError> {
    // ClassHeritage is evaluated in the *outer* PrivateEnvironment.  The
    // class's own names become visible only after this point.
    if let Some(base) = &class.extends {
        validate_private_expression(base, names)?;
    }
    let declarations = class_private_declarations(class)?;
    let mut class_names = names.clone();
    class_names.extend(declarations.into_iter().map(|(name, _)| name));
    for element in &class.elements {
        match element {
            ClassElement::Method { key, function, .. }
            | ClassElement::Accessor { key, function, .. } => {
                validate_private_key(key, &class_names)?;
                validate_private_function(function, &class_names)?;
            }
            ClassElement::Field {
                key, initializer, ..
            } => {
                validate_private_key(key, &class_names)?;
                if let Some(initializer) = initializer {
                    validate_private_expression(initializer, &class_names)?;
                }
            }
            ClassElement::StaticBlock(statements) => {
                validate_private_statements(statements, &class_names)?;
            }
        }
    }
    Ok(())
}

fn validate_private_function(
    function: &Function,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    for parameter in &function.params {
        validate_private_pattern(&parameter.pattern, names)?;
        if let Some(default) = &parameter.default {
            validate_private_expression(default, names)?;
        }
    }
    validate_private_statements(&function.body, names)
}

fn validate_private_pattern(
    pattern: &Pattern,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Identifier(_) => Ok(()),
        Pattern::Array(elements) => elements.iter().flatten().try_for_each(|element| {
            validate_private_pattern(&element.pattern, names)?;
            element.default.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
        Pattern::Object(properties) => properties.iter().try_for_each(|property| match property {
            ObjectPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                validate_private_key(key, names)?;
                validate_private_pattern(value, names)?;
                default.as_ref().map_or(Ok(()), |expression| {
                    validate_private_expression(expression, names)
                })
            }
            ObjectPatternProp::Rest(pattern) => validate_private_pattern(pattern, names),
        }),
    }
}

fn validate_private_assignment_pattern(
    pattern: &AssignmentPattern,
    names: &HashSet<String>,
) -> Result<(), CompileError> {
    match pattern {
        AssignmentPattern::Target(expression) => validate_private_expression(expression, names),
        AssignmentPattern::Array(elements) => elements.iter().flatten().try_for_each(|element| {
            validate_private_assignment_pattern(&element.pattern, names)?;
            element.default.as_ref().map_or(Ok(()), |expression| {
                validate_private_expression(expression, names)
            })
        }),
        AssignmentPattern::Object(properties) => {
            properties.iter().try_for_each(|property| match property {
                AssignmentPatternProp::KeyValue {
                    key,
                    value,
                    default,
                } => {
                    validate_private_key(key, names)?;
                    validate_private_assignment_pattern(value, names)?;
                    default.as_ref().map_or(Ok(()), |expression| {
                        validate_private_expression(expression, names)
                    })
                }
                AssignmentPatternProp::Rest(pattern) => {
                    validate_private_assignment_pattern(pattern, names)
                }
            })
        }
    }
}

fn validate_private_key(key: &PropertyKey, names: &HashSet<String>) -> Result<(), CompileError> {
    if let PropertyKey::Computed(expression) = key {
        validate_private_expression(expression, names)
    } else {
        Ok(())
    }
}

fn validate_private_expression(expr: &Expr, names: &HashSet<String>) -> Result<(), CompileError> {
    match expr {
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
        | Expr::ImportMeta => Ok(()),
        Expr::Parenthesized(expression)
        | Expr::Await(expression)
        | Expr::DynamicImport(expression)
        | Expr::Unary {
            arg: expression, ..
        }
        | Expr::Update {
            arg: expression, ..
        } => validate_private_expression(expression, names),
        Expr::Template { expressions, .. } => expressions
            .iter()
            .try_for_each(|expression| validate_private_expression(expression, names)),
        Expr::TaggedTemplate {
            tag, expressions, ..
        } => {
            validate_private_expression(tag, names)?;
            expressions
                .iter()
                .try_for_each(|expression| validate_private_expression(expression, names))
        }
        Expr::Array(elements) => elements
            .iter()
            .flatten()
            .try_for_each(|element| match element {
                ArrayElement::Normal(expression) | ArrayElement::Spread(expression) => {
                    validate_private_expression(expression, names)
                }
            }),
        Expr::Object(properties) => properties.iter().try_for_each(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                validate_private_key(key, names)?;
                validate_private_expression(value, names)
            }
            ObjectProp::Spread(expression) => validate_private_expression(expression, names),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                validate_private_key(key, names)?;
                validate_private_function(function, names)
            }
        }),
        Expr::Function(function) => validate_private_function(function, names),
        Expr::Class(class) => validate_private_class(class, names),
        Expr::Yield { value, .. } => value.as_deref().map_or(Ok(()), |expression| {
            validate_private_expression(expression, names)
        }),
        Expr::Arrow { params, body, .. } => {
            for parameter in params {
                validate_private_pattern(&parameter.pattern, names)?;
                if let Some(default) = &parameter.default {
                    validate_private_expression(default, names)?;
                }
            }
            match body {
                ArrowBody::Expr(expression) => validate_private_expression(expression, names),
                ArrowBody::Block(statements) => validate_private_statements(statements, names),
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            validate_private_expression(left, names)?;
            validate_private_expression(right, names)
        }
        Expr::Sequence(expressions) => expressions
            .iter()
            .try_for_each(|expression| validate_private_expression(expression, names)),
        Expr::Assign { target, value, .. } => {
            validate_private_expression(target, names)?;
            validate_private_expression(value, names)
        }
        Expr::DestructureAssign { pattern, value } => {
            validate_private_assignment_pattern(pattern, names)?;
            validate_private_expression(value, names)
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            validate_private_expression(test, names)?;
            validate_private_expression(consequent, names)?;
            validate_private_expression(alternate, names)
        }
        Expr::Call { callee, args } | Expr::New { callee, args } => {
            validate_private_expression(callee, names)?;
            args.iter().try_for_each(|argument| match argument {
                Argument::Normal(expression) | Argument::Spread(expression) => {
                    validate_private_expression(expression, names)
                }
            })
        }
        Expr::Member {
            object,
            property,
            computed,
        }
        | Expr::OptionalMember {
            object,
            property,
            computed,
        } => {
            validate_private_expression(object, names)?;
            if *computed {
                validate_private_expression(property, names)?;
            }
            if !*computed {
                if let Expr::Identifier(name) = property.as_ref() {
                    if let Some(name) = name.strip_prefix('#') {
                        if matches!(object.as_ref(), Expr::Super) {
                            return Err(CompileError::InvalidSyntax(
                                "super cannot access a private element",
                            ));
                        }
                        validate_private_name(name, names)?;
                    }
                }
            }
            Ok(())
        }
        Expr::PrivateIn { name, object } => {
            validate_private_expression(object, names)?;
            validate_private_name(name, names)
        }
    }
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
    compile_with_limit_and_mode(program, max_bytecode_bytes, false, &[], &[])
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
    )
}

fn compile_with_limit_and_mode(
    program: &Program,
    max_bytecode_bytes: u32,
    module: bool,
    module_imports: &[ImportEntry],
    module_exports: &[ExportEntry],
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

    fn statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.function_declarations(statements)?;
        self.statements_after_function_declarations(statements)
    }

    /// Module linking separates declaration instantiation from evaluation.
    /// Keeping the declaration prefix explicit lets the VM run it for every
    /// member of a cyclic graph before it starts evaluating any body.
    fn function_declarations(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        for statement in statements {
            let (function, binding_name) = match statement {
                Stmt::FunctionDecl(function) => (
                    function,
                    function.name.as_ref().expect("declaration has a name"),
                ),
                Stmt::ModuleDefaultFunction { function, binding } => (function, binding),
                _ => continue,
            };
            if matches!(statement, Stmt::ModuleDefaultFunction { binding, .. } if binding == MODULE_DEFAULT_BINDING)
            {
                // An anonymous default function declaration has a private
                // module binding, but its function object is named
                // `"default"`.  This is inference, not a named function
                // expression, so it must not create an inner `default`
                // lexical binding.
                self.function_named(function, false, Some("default"), false)?;
            } else {
                self.function(function, false)?;
            }
            let slot = self.resolve(binding_name).unwrap();
            if self.bytecode.bindings[slot as usize].lexical {
                self.emit(Opcode::InitializeBinding, slot)?;
            } else {
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
            }
            // Annex B.3.2/B.3.3 only supplies the legacy outer var for
            // ordinary functions. Generator and async declarations stay
            // exclusively lexical even in sloppy code.
            if matches!(statement, Stmt::FunctionDecl(_)) && is_annex_b_function(function) {
                if let Some(outer) = self.annex_b_outer_var_slot(slot) {
                    self.emit(Opcode::GetBinding, slot)?;
                    self.emit(Opcode::StoreBinding, outer)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
        }
        Ok(())
    }

    fn statements_after_function_declarations(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), CompileError> {
        for statement in statements {
            self.statement(statement, true)?;
        }
        Ok(())
    }

    fn statement(
        &mut self,
        statement: &Stmt,
        declarations_allowed: bool,
    ) -> Result<(), CompileError> {
        match statement {
            Stmt::Throw(value) => {
                self.expression(value)?;
                self.emit(Opcode::Throw, 0)?;
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler.as_ref(), finalizer.as_deref())?,
            Stmt::With { object, body } => {
                if self.bytecode.strict {
                    return Err(CompileError::InvalidSyntax(
                        "with is forbidden in strict mode",
                    ));
                }
                self.expression(object)?;
                self.emit(Opcode::EnterWith, 0)?;
                self.with_depth += 1;
                let result = self.statement(body, false);
                self.with_depth -= 1;
                result?;
                self.emit(Opcode::LeaveWith, 0)?;
            }
            Stmt::FunctionDecl(_) | Stmt::ModuleDefaultFunction { .. } => {}
            Stmt::ClassDecl(class) => {
                let slot = self
                    .resolve(class.name.as_deref().expect("class declaration has a name"))
                    .unwrap();
                self.class_expression_with_binding(class, None, Some(slot))?;
            }
            Stmt::ClassField(statement) => {
                self.emit(Opcode::EnterClassFieldInitializer, 0)?;
                self.statement(statement, declarations_allowed)?;
                self.emit(Opcode::LeaveClassFieldInitializer, 0)?;
            }
            Stmt::ClassPrivateBrand(binding) => {
                let slot = self.resolve(binding).ok_or(CompileError::InvalidSyntax(
                    "private brand binding is not available in this function",
                ))?;
                self.emit(Opcode::InitializePrivateBrand, slot)?;
            }
            Stmt::Expr(Expr::Class(class)) => {
                self.class_expression(class, None)?;
                self.emit(Opcode::Pop, 0)?;
            }
            Stmt::Return(value) => {
                if !self.function {
                    return Err(CompileError::InvalidSyntax("return requires a function"));
                }
                if let Some(args) = value
                    .as_ref()
                    .and_then(|value| self.self_tail_call_args(value))
                {
                    for argument in args {
                        let Argument::Normal(value) = argument else {
                            unreachable!("self tail calls exclude spread arguments")
                        };
                        self.expression(value)?;
                    }
                    let iterators: Vec<_> = self
                        .loops
                        .iter()
                        .rev()
                        .filter_map(|context| context.iterator)
                        .collect();
                    for iterator in iterators {
                        self.emit(Opcode::GetBinding, iterator)?;
                        self.emit(Opcode::IteratorClose, 0)?;
                    }
                    self.emit(
                        Opcode::TailRecur,
                        u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                    )?;
                    return Ok(());
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                let iterators: Vec<_> = self
                    .loops
                    .iter()
                    .rev()
                    .filter_map(|context| context.iterator)
                    .collect();
                for iterator in iterators {
                    self.emit(Opcode::GetBinding, iterator)?;
                    self.emit(Opcode::IteratorClose, 0)?;
                }
                self.emit(Opcode::Return, 0)?;
            }
            Stmt::Empty => {}
            Stmt::Expr(expr) => {
                self.expression(expr)?;
                self.emit(Opcode::SetCompletion, 0)?;
            }
            Stmt::Block(body) => {
                self.enter_scope(block_lexical_names(body)?, &var_names(body)?, false)?;
                self.statements(body)?;
                self.leave_scope()?;
            }
            Stmt::VarDecl(kind, declarations) => {
                if !declarations_allowed && *kind != DeclKind::Var {
                    return Err(CompileError::InvalidSyntax(
                        "a lexical declaration requires a block",
                    ));
                }
                self.declarations(*kind, declarations)?;
            }
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                self.emit(Opcode::ClearCompletion, 0)?;
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.if_clause_statement(consequent)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                if let Some(alternate) = alternate {
                    self.if_clause_statement(alternate)?;
                }
                self.patch(end, self.offset()?);
            }
            Stmt::While { test, body } => {
                self.loop_statement(None, Some(test), None, body, false, Vec::new())?
            }
            Stmt::DoWhile { body, test } => {
                self.loop_statement(None, Some(test), None, body, true, Vec::new())?
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.loop_statement(
                init.as_ref(),
                test.as_ref(),
                update.as_ref(),
                body,
                false,
                Vec::new(),
            )?,
            Stmt::ForIn { left, right, body } => self.for_in(left, right, body, Vec::new())?,
            Stmt::ForOf {
                left,
                right,
                body,
                is_await,
            } => self.for_of(left, right, body, *is_await, Vec::new())?,
            Stmt::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, Vec::new())?,
            Stmt::Labelled { label, item } => self.labelled_statement(label, item)?,
            Stmt::Break(label) => self.control_transfer(label.as_deref(), false)?,
            Stmt::Continue(label) => self.control_transfer(label.as_deref(), true)?,
        }
        Ok(())
    }

    /// Annex B.3.3 parses a sloppy FunctionDeclaration in an `if` clause as
    /// a synthetic block whose lexical function binding is then copied to the
    /// Annex B outer var binding when that clause executes.
    fn if_clause_statement(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        if !self.bytecode.strict && matches!(statement, Stmt::FunctionDecl(_)) {
            self.enter_scope(
                block_lexical_names(std::slice::from_ref(statement))?,
                &BTreeSet::new(),
                false,
            )?;
            self.statements(std::slice::from_ref(statement))?;
            self.leave_scope()
        } else {
            self.statement(statement, false)
        }
    }

    fn labelled_statement(&mut self, label: &str, item: &Stmt) -> Result<(), CompileError> {
        let mut labels = vec![label.to_string()];
        let mut item = item;
        while let Stmt::Labelled {
            label,
            item: nested,
        } = item
        {
            labels.push(label.clone());
            item = nested;
        }
        if labels
            .iter()
            .any(|label| label == "yield" && self.bytecode.strict)
        {
            return Err(CompileError::InvalidSyntax(
                "yield cannot be used as a label in strict code",
            ));
        }
        if labels.iter().any(|label| {
            labels.iter().filter(|other| *other == label).count() != 1
                || self
                    .loops
                    .iter()
                    .any(|context| context.labels.iter().any(|other| other == label))
        }) {
            return Err(CompileError::InvalidSyntax("duplicate label"));
        }
        match item {
            Stmt::While { test, body } => {
                self.loop_statement(None, Some(test), None, body, false, labels)
            }
            Stmt::DoWhile { body, test } => {
                self.loop_statement(None, Some(test), None, body, true, labels)
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.loop_statement(
                init.as_ref(),
                test.as_ref(),
                update.as_ref(),
                body,
                false,
                labels,
            ),
            Stmt::ForIn { left, right, body } => self.for_in(left, right, body, labels),
            Stmt::ForOf {
                left,
                right,
                body,
                is_await,
            } => self.for_of(left, right, body, *is_await, labels),
            Stmt::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, labels),
            Stmt::VarDecl(kind, _) if *kind != DeclKind::Var => Err(CompileError::InvalidSyntax(
                "a labelled statement cannot contain a lexical declaration",
            )),
            Stmt::ClassDecl(_) => Err(CompileError::InvalidSyntax(
                "a labelled statement cannot contain a class declaration",
            )),
            Stmt::FunctionDecl(function)
                if self.bytecode.strict || function.generator || function.is_async =>
            {
                Err(CompileError::InvalidSyntax(
                    "invalid labelled function declaration",
                ))
            }
            Stmt::FunctionDecl(function) => {
                // Annex B permits this sloppy-mode form. Its binding is
                // var-scoped, while creation occurs when the label executes.
                self.function(function, false)?;
                let slot = self
                    .resolve(function.name.as_ref().expect("declaration has a name"))
                    .unwrap();
                self.emit(Opcode::StoreBinding, slot)?;
                self.emit(Opcode::Pop, 0)?;
                Ok(())
            }
            _ => {
                self.loops.push(Loop {
                    labels,
                    breakable: false,
                    scope_depth: self.scopes.len(),
                    breaks: Vec::new(),
                    continues: None,
                    iterator: None,
                });
                self.statement(item, false)?;
                let end = self.offset()?;
                let context = self.loops.pop().expect("label control is active");
                for (jump, control) in context.breaks {
                    self.patch(jump, end);
                    self.bytecode.abrupt_jumps[control].target = end;
                }
                Ok(())
            }
        }
    }

    fn control_transfer(
        &mut self,
        label: Option<&str>,
        is_continue: bool,
    ) -> Result<(), CompileError> {
        let index = match label {
            Some(label) => self
                .loops
                .iter()
                .rposition(|context| context.labels.iter().any(|candidate| candidate == label)),
            None if is_continue => self
                .loops
                .iter()
                .rposition(|context| context.continues.is_some()),
            None => self.loops.iter().rposition(|context| context.breakable),
        };
        let Some(index) = index else {
            return Err(CompileError::InvalidSyntax(if is_continue {
                "continue requires an enclosing iteration statement"
            } else {
                "break requires an enclosing loop, switch, or label"
            }));
        };
        if is_continue && self.loops[index].continues.is_none() {
            return Err(CompileError::InvalidSyntax(
                "continue label does not name an iteration statement",
            ));
        }
        let scopes: Vec<_> = self.scopes[self.loops[index].scope_depth..]
            .iter()
            .rev()
            .copied()
            .collect();
        let first_iterator = index + usize::from(is_continue);
        let iterators: Vec<_> = self.loops[first_iterator..]
            .iter()
            .rev()
            .filter_map(|context| context.iterator)
            .collect();
        // A direct jump would skip a surrounding `finally`. Route to a local
        // cleanup gateway first; handlers resume there only after finalizers.
        let control = self.bytecode.abrupt_jumps.len();
        let control_operand = u32::try_from(control).map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.abrupt_jumps.push(AbruptJump {
            cleanup: 0,
            target: 0,
        });
        self.emit(Opcode::AbruptJump, control_operand)?;
        let cleanup = self.offset()?;
        self.bytecode.abrupt_jumps[control].cleanup = cleanup;
        for iterator in iterators {
            self.emit(Opcode::GetBinding, iterator)?;
            self.emit(Opcode::IteratorClose, 0)?;
        }
        for scope in scopes {
            self.emit(Opcode::LeaveScope, scope)?;
        }
        let jump = self.emit(Opcode::Jump, 0)?;
        let context = &mut self.loops[index];
        if is_continue {
            context
                .continues
                .as_mut()
                .expect("selected context is an iteration statement")
                .push((jump, control));
        } else {
            context.breaks.push((jump, control));
        }
        Ok(())
    }

    fn scoped_statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        self.enter_scope(
            block_lexical_names(statements)?,
            &var_names(statements)?,
            false,
        )?;
        self.statements(statements)?;
        self.leave_scope()
    }

    fn switch_statement(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        validate_switch_case_declarations(cases, self.bytecode.strict)?;
        self.emit(Opcode::ClearCompletion, 0)?;
        let lexical = switch_lexical_names(cases)?;
        let vars = switch_var_names(cases)?;
        // Switch evaluation creates its case-block lexical environment only
        // after evaluating the discriminant.  A closure created by the
        // discriminant must therefore capture the surrounding binding, while
        // closures created by case selectors or consequents capture the
        // switch-local binding.
        self.expression(discriminant)?;
        self.enter_scope(lexical, &vars, false)?;

        let mut case_entries = vec![None; cases.len()];
        for (index, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                self.emit(Opcode::Dup, 0)?;
                self.expression(test)?;
                self.emit(Opcode::StrictEqual, 0)?;
                let no_match = self.emit(Opcode::JumpIfFalse, 0)?;
                case_entries[index] = Some(self.emit(Opcode::Jump, 0)?);
                self.patch(no_match, self.offset()?);
            }
        }
        let no_match = self.emit(Opcode::Jump, 0)?;
        let no_match_cleanup = self.offset()?;
        self.emit(Opcode::Pop, 0)?;
        let no_match_exit = self.emit(Opcode::Jump, 0)?;

        let mut case_stubs = Vec::with_capacity(cases.len());
        let mut body_jumps = Vec::with_capacity(cases.len());
        for _ in cases {
            case_stubs.push(self.offset()?);
            self.emit(Opcode::Pop, 0)?;
            body_jumps.push(self.emit(Opcode::Jump, 0)?);
        }
        for (entry, stub) in case_entries.into_iter().zip(&case_stubs) {
            if let Some(entry) = entry {
                self.patch(entry, *stub);
            }
        }
        let default = cases.iter().position(|case| case.test.is_none());
        self.patch(
            no_match,
            default
                .map(|index| case_stubs[index])
                .unwrap_or(no_match_cleanup),
        );

        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: None,
            iterator: None,
        });
        for (case, jump) in cases.iter().zip(body_jumps) {
            self.patch(jump, self.offset()?);
            self.statements(&case.consequent)?;
        }
        let end = self.offset()?;
        self.patch(no_match_exit, end);
        let context = self.loops.pop().expect("switch control is active");
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        self.leave_scope()?;
        Ok(())
    }

    /// Compiles `try` as fixed bytecode plus immutable handler metadata. A
    /// runtime frame records dynamic stack/scope depths, so a throw can safely
    /// enter a catch block or run a finalizer without walking the AST.
    fn try_statement(
        &mut self,
        block: &[Stmt],
        handler: Option<&CatchClause>,
        finalizer: Option<&[Stmt]>,
    ) -> Result<(), CompileError> {
        let handler_index = u32::try_from(self.bytecode.handlers.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.handlers.push(Handler {
            try_start: 0,
            try_end: 0,
            catch: None,
            catch_end: None,
            finally: None,
        });
        self.emit(Opcode::PushHandler, handler_index)?;

        // Each TryBlock has its own Completion. An empty block must not leak
        // the value of the preceding statement into TryStatement's
        // UpdateEmpty step.
        self.emit(Opcode::ClearCompletion, 0)?;
        self.bytecode.handlers[handler_index as usize].try_start = self.offset()?;
        self.scoped_statements(block)?;
        self.bytecode.handlers[handler_index as usize].try_end = self.offset()?;
        self.emit(Opcode::PopHandler, 0)?;
        if finalizer.is_some() {
            self.emit(Opcode::SaveCompletion, 0)?;
        }
        let normal_exit = self.emit(Opcode::Jump, 0)?;

        let catch_exit = if let Some(catch) = handler {
            let start = self.offset()?;
            self.bytecode.handlers[handler_index as usize].catch = Some(start);
            let parameter_bound_names = catch.param.as_ref().map(pattern_names).unwrap_or_default();
            if self.bytecode.strict
                && parameter_bound_names
                    .iter()
                    .any(|name| matches!(name.as_str(), "eval" | "arguments"))
            {
                return Err(CompileError::InvalidSyntax(
                    "strict catch parameters cannot bind eval or arguments",
                ));
            }
            if catch_lexical_names(&catch.body)
                .into_iter()
                .any(|name| parameter_bound_names.contains(&name))
            {
                return Err(CompileError::InvalidSyntax(
                    "a catch parameter conflicts with a lexical declaration",
                ));
            }
            let parameter_names = parameter_bound_names
                .into_iter()
                .map(|name| (name, DeclKind::Let))
                .collect();
            self.enter_scope(parameter_names, &BTreeSet::new(), false)?;
            let mut catch_var_slots = HashMap::new();
            if let Some(Pattern::Identifier(name)) = &catch.param {
                let slot = self.resolve(name).expect("catch parameter was declared");
                self.bytecode.bindings[slot as usize].catch_parameter = true;
                catch_var_slots.insert(name.clone(), slot);
            }
            self.catch_var_slots.push(catch_var_slots);
            if let Some(param) = &catch.param {
                // The VM places the caught JavaScript value on the stack at
                // this destination. Binding initialization consumes it.
                self.bind_pattern(param, DeclKind::Let)?;
            } else {
                self.emit(Opcode::Pop, 0)?;
            }
            // CatchParameter initialization is not part of the Block's
            // completion value.
            self.emit(Opcode::ClearCompletion, 0)?;
            self.enter_scope(
                block_lexical_names(&catch.body)?,
                &var_names(&catch.body)?,
                false,
            )?;
            self.statements(&catch.body)?;
            self.leave_scope()?;
            self.catch_var_slots
                .pop()
                .expect("catch var override is active");
            self.leave_scope()?;
            self.bytecode.handlers[handler_index as usize].catch_end = Some(self.offset()?);
            self.emit(Opcode::PopHandler, 0)?;
            if finalizer.is_some() {
                self.emit(Opcode::SaveCompletion, 0)?;
            }
            Some(self.emit(Opcode::Jump, 0)?)
        } else {
            None
        };

        if let Some(finalizer) = finalizer {
            let start = self.offset()?;
            self.bytecode.handlers[handler_index as usize].finally = Some(start);
            // A normal finally restores its saved prior Completion only when
            // this block remains empty; a non-empty finalizer keeps its own.
            self.emit(Opcode::ClearCompletion, 0)?;
            self.scoped_statements(finalizer)?;
            // On a normal entry the handler has already been popped and this
            // restores the preceding non-empty completion. On an abrupt entry
            // it replays the pending completion after the finalizer finishes.
            self.emit(Opcode::ResumeCompletion, handler_index)?;
            let end = self.offset()?;
            self.patch(normal_exit, start);
            if let Some(exit) = catch_exit {
                self.patch(exit, start);
            }
            debug_assert!(end as usize <= self.bytecode.code.len());
        } else {
            let end = self.offset()?;
            self.patch(normal_exit, end);
            if let Some(exit) = catch_exit {
                self.patch(exit, end);
            }
        }
        Ok(())
    }

    fn declarations(
        &mut self,
        kind: DeclKind,
        declarations: &[VarDeclarator],
    ) -> Result<(), CompileError> {
        for declaration in declarations {
            if kind == DeclKind::Const && declaration.init.is_none() {
                return Err(CompileError::InvalidSyntax("const requires an initializer"));
            }
            if matches!(declaration.pattern, Pattern::Identifier(_))
                && kind == DeclKind::Var
                && declaration.init.is_none()
            {
                continue;
            }
            if let Some(value) = &declaration.init {
                let inferred_name = match (&declaration.pattern, self.bytecode.module) {
                    (Pattern::Identifier(name), true) if name == MODULE_DEFAULT_BINDING => {
                        Some("default")
                    }
                    _ => None,
                };
                self.expression_with_name(value, inferred_name)?
            } else {
                if !matches!(declaration.pattern, Pattern::Identifier(_)) {
                    return Err(CompileError::InvalidSyntax(
                        "a destructuring declaration requires an initializer",
                    ));
                }
                self.constant(Value::Undefined)?
            }
            self.bind_pattern(&declaration.pattern, kind)?;
        }
        Ok(())
    }

    /// Consumes the value at the top of the operand stack and performs a
    /// BindingInitialization for every identifier in `pattern`.  The bytecode
    /// keeps iterator records on the stack while descending into an array
    /// pattern so abrupt completions can close every active iterator.
    fn bind_pattern(&mut self, pattern: &Pattern, kind: DeclKind) -> Result<(), CompileError> {
        match pattern {
            Pattern::Identifier(name) => {
                let slot = if kind == DeclKind::Var {
                    self.catch_var_slots
                        .iter()
                        .rev()
                        .find_map(|slots| slots.get(name))
                        .copied()
                        .or_else(|| self.names[self.local_scope].get(name).copied())
                        .or_else(|| self.resolve(name))
                        .expect("var declaration has a function or eval binding")
                } else {
                    self.names.last().unwrap()[name]
                };
                if kind == DeclKind::Var {
                    self.emit(Opcode::StoreBinding, slot)?;
                    self.emit(Opcode::Pop, 0)?;
                } else {
                    self.emit(Opcode::InitializeBinding, slot)?;
                }
            }
            Pattern::Array(elements) => {
                self.emit(Opcode::GetIterator, 0)?;
                for element in elements {
                    let Some(element) = element else {
                        self.emit(Opcode::IteratorElision, 0)?;
                        continue;
                    };
                    if element.rest {
                        self.emit(Opcode::IteratorRest, 0)?;
                        self.bind_pattern(&element.pattern, kind)?;
                        return Ok(());
                    }
                    self.array_pattern_value()?;
                    self.binding_pattern_default(element.default.as_ref(), &element.pattern)?;
                    self.bind_pattern(&element.pattern, kind)?;
                }
                self.emit(Opcode::IteratorFinish, 0)?;
            }
            Pattern::Object(properties) => {
                // Even an empty object pattern performs RequireObjectCoercible.
                self.emit(Opcode::RequireObject, 0)?;
                self.emit(Opcode::NewArray, 0)?;
                for property in properties {
                    match property {
                        ObjectPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.property_key(key)?;
                            self.emit(Opcode::DestructureProperty, 0)?;
                            self.binding_pattern_default(default.as_ref(), value)?;
                            self.bind_pattern(value, kind)?;
                        }
                        ObjectPatternProp::Rest(pattern) => {
                            self.emit(Opcode::ObjectRest, 0)?;
                            self.bind_pattern(pattern, kind)?;
                            return Ok(());
                        }
                    }
                }
                self.emit(Opcode::Pop, 0)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        Ok(())
    }

    /// Leaves the array-pattern iterator record below one element value.  A
    /// record remembers exhaustion in the VM, so later elisions do not call
    /// `next` again after the first completed result.
    fn array_pattern_value(&mut self) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        let exhausted = self.emit(Opcode::IteratorStep, 0)?;
        let joined = self.emit(Opcode::Jump, 0)?;
        self.patch(exhausted, self.offset()?);
        self.constant(Value::Undefined)?;
        self.patch(joined, self.offset()?);
        Ok(())
    }

    /// Replaces an `undefined` destructuring-assignment value with its
    /// initializer. An anonymous function, class, or arrow default receives
    /// the IdentifierReference target's inferred name; `null` remains a
    /// value, as required by ECMA-262.
    fn assignment_pattern_default(
        &mut self,
        default: Option<&Expr>,
        pattern: &AssignmentPattern,
    ) -> Result<(), CompileError> {
        let Some(default) = default else {
            return Ok(());
        };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(
            default,
            match pattern {
                AssignmentPattern::Target(target) => match &**target {
                    Expr::Identifier(name) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            },
        )?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    fn binding_pattern_default(
        &mut self,
        default: Option<&Expr>,
        pattern: &Pattern,
    ) -> Result<(), CompileError> {
        let Some(default) = default else {
            return Ok(());
        };
        self.emit(Opcode::Dup, 0)?;
        self.constant(Value::Undefined)?;
        self.emit(Opcode::StrictEqual, 0)?;
        let skip = self.emit(Opcode::JumpIfFalse, 0)?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(
            default,
            match pattern {
                Pattern::Identifier(name) => Some(name.as_str()),
                _ => None,
            },
        )?;
        self.patch(skip, self.offset()?);
        Ok(())
    }

    fn expression_with_name(
        &mut self,
        expression: &Expr,
        inferred_name: Option<&str>,
    ) -> Result<(), CompileError> {
        match expression {
            Expr::Parenthesized(expression) => self.expression_with_name(expression, inferred_name),
            Expr::Function(function) if function.name.is_none() && inferred_name.is_some() => {
                self.function_named(function, false, inferred_name, false)
            }
            Expr::Class(class) if class.name.is_none() && inferred_name.is_some() => {
                self.class_expression(class, inferred_name)
            }
            Expr::Arrow {
                params,
                body,
                is_async,
            } if inferred_name.is_some() => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function_named(
                    &Function {
                        name: None,
                        params: params.clone(),
                        body,
                        generator: false,
                        is_async: *is_async,
                    },
                    true,
                    inferred_name,
                    false,
                )
            }
            _ => self.expression(expression),
        }
    }

    fn loop_statement(
        &mut self,
        init: Option<&ForInit>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
        do_first: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let lexical = match init {
            Some(ForInit::VarDecl(kind, decls)) if *kind != DeclKind::Var => {
                declarations_names(*kind, decls)?
            }
            _ => Vec::new(),
        };
        let own_scope = !lexical.is_empty();
        if own_scope {
            self.enter_scope(lexical, &var_names(std::slice::from_ref(body))?, false)?;
        }
        match init {
            Some(ForInit::VarDecl(kind, declarations)) => self.declarations(*kind, declarations)?,
            Some(ForInit::Expr(expr)) => {
                self.expression(expr)?;
                self.emit(Opcode::Pop, 0)?;
            }
            None => {}
        }
        let start = self.offset()?;
        let mut exit = None;
        if !do_first {
            if let Some(test) = test {
                self.expression(test)?;
                exit = Some(self.emit(Opcode::JumpIfFalse, 0)?);
            }
        }
        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: None,
        });
        self.statement(body, false)?;
        let continue_at = self.offset()?;
        // CreatePerIterationEnvironment happens after the body and before
        // the update expression.  That leaves closures made by this turn
        // attached to its old cells while the update writes into the next
        // iteration's bindings.  `continue` targets this point as well.
        if own_scope {
            let scope = *self
                .scopes
                .last()
                .expect("lexical for scope remains active");
            self.emit(Opcode::CloneScope, scope)?;
        }
        if let Some(update) = update {
            self.expression(update)?;
            self.emit(Opcode::Pop, 0)?;
        }
        if do_first {
            self.expression(test.expect("do-while always has a condition"))?;
            self.emit(Opcode::JumpIfTrue, start)?;
        } else {
            self.emit(Opcode::Jump, start)?;
        }
        let end = self.offset()?;
        if let Some(exit) = exit {
            self.patch(exit, end);
        }
        let context = self.loops.pop().unwrap();
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        for (jump, control) in context.continues.expect("loop has continue targets") {
            self.patch(jump, continue_at);
            self.bytecode.abrupt_jumps[control].target = continue_at;
        }
        if own_scope {
            self.leave_scope()?;
        }
        Ok(())
    }

    fn expression(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::RegExp { pattern, flags } => {
                self.constant(Value::String(pattern.clone()))?;
                self.constant(Value::String(flags.clone()))?;
                self.emit(Opcode::RegExpLiteral, 0)?;
            }
            Expr::TaggedTemplate {
                tag,
                raw,
                cooked,
                expressions,
            } => {
                if matches!(&**tag, Expr::Member { .. }) {
                    if private_member_name(tag).is_some() {
                        let owner = self.private_member_reference(tag)?;
                        self.emit(Opcode::PrivateGetMethod, owner)?;
                    } else {
                        self.member_reference(tag)?;
                        self.emit(Opcode::GetMethod, 0)?;
                    }
                } else {
                    self.expression(tag)?;
                    self.constant(Value::Undefined)?;
                }
                static NEXT_SITE: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(1);
                let id = NEXT_SITE
                    .fetch_update(
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                        |n| n.checked_add(1),
                    )
                    .map_err(|_| CompileError::ProgramTooLarge)?;
                let site = self.bytecode.templates.len() as u32;
                self.bytecode.templates.push(crate::bytecode::TemplateSite {
                    id,
                    raw: raw.clone(),
                    cooked: cooked.clone(),
                });
                self.emit(Opcode::TemplateObject, site)?;
                for expression in expressions {
                    self.expression(expression)?;
                }
                self.emit(Opcode::Call, expressions.len() as u32 + 1)?;
            }
            Expr::Number(n) => self.constant(Value::Number(*n))?,
            Expr::BigInt(n) => self.constant(Value::BigInt(n.clone()))?,
            Expr::String(s) => self.constant(Value::String(s.clone()))?,
            Expr::Bool(b) => self.constant(Value::Bool(*b))?,
            Expr::Null => self.constant(Value::Null)?,
            Expr::Identifier(name) => {
                if self.bytecode.strict && matches!(name.as_str(), "yield" | "let") {
                    return Err(CompileError::InvalidSyntax(
                        "a reserved word cannot be used as an identifier in strict code",
                    ));
                }
                if self.with_depth != 0 {
                    let index = u32::try_from(self.bytecode.constants.len())
                        .map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode
                        .constants
                        .push(Value::String(name.clone().into()));
                    self.emit(Opcode::WithGet, index)?;
                } else if let Some(slot) = self.resolve(name) {
                    self.emit(Opcode::GetBinding, slot)?;
                } else {
                    match name.as_str() {
                        "undefined" => self.constant(Value::Undefined)?,
                        "NaN" => self.constant(Value::Number(f64::NAN))?,
                        "Infinity" => self.constant(Value::Number(f64::INFINITY))?,
                        "String" => {
                            self.emit(Opcode::GlobalString, 0)?;
                        }
                        "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number"
                        | "Boolean" | "BigInt" | "Array" | "Function" | "Proxy" | "globalThis"
                        | "Intl" | "Promise" | "Error" | "TypeError" | "eval" | "isNaN"
                        | "isFinite" | "parseInt" | "parseFloat" | "JSON" | "RangeError"
                        | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError" => {
                            let index = self.bytecode.constants.len() as u32;
                            self.bytecode
                                .constants
                                .push(Value::String(name.as_str().into()));
                            self.emit(Opcode::Global, index)?;
                        }
                        _ => {
                            let index = u32::try_from(self.bytecode.constants.len())
                                .map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode
                                .constants
                                .push(Value::String(name.clone().into()));
                            self.emit(Opcode::UnboundName, index)?;
                        }
                    }
                }
            }
            Expr::Unary { op, arg } => {
                if *op == UnaryOp::Void {
                    self.expression(arg)?;
                    self.emit(Opcode::Pop, 0)?;
                    self.constant(Value::Undefined)?;
                    return Ok(());
                }
                let opcode = match op {
                    UnaryOp::Neg => Opcode::Negate,
                    UnaryOp::Plus => Opcode::ToNumber,
                    UnaryOp::Not => Opcode::Not,
                    UnaryOp::BitNot => Opcode::BitNot,
                    UnaryOp::Typeof => Opcode::Typeof,
                    UnaryOp::Delete | UnaryOp::Void => Opcode::DeleteProperty,
                };
                if *op == UnaryOp::Delete {
                    if matches!(&**arg, Expr::Member { .. }) {
                        if private_member_name(arg).is_some() {
                            return Err(CompileError::InvalidSyntax(
                                "cannot delete a private element",
                            ));
                        }
                        self.member_reference(arg)?;
                        self.emit(opcode, 0)?;
                    } else if let Expr::Identifier(name) = &**arg {
                        if self.bytecode.strict {
                            return Err(CompileError::InvalidSyntax(
                                "cannot delete a binding in strict mode",
                            ));
                        }
                        if let Some(slot) = self.resolve(name) {
                            if self.bytecode.dynamic_eval_slots.contains(&slot) {
                                self.emit(Opcode::DeleteDynamicBinding, slot)?;
                            } else {
                                self.constant(Value::Bool(false))?;
                            }
                        } else {
                            let index = u32::try_from(self.bytecode.constants.len())
                                .map_err(|_| CompileError::ProgramTooLarge)?;
                            self.bytecode
                                .constants
                                .push(Value::String(name.clone().into()));
                            self.emit(Opcode::DeleteUnboundName, index)?;
                        }
                    } else {
                        self.expression(arg)?;
                        self.emit(Opcode::Pop, 0)?;
                        self.constant(Value::Bool(true))?;
                    }
                    return Ok(());
                }
                if *op == UnaryOp::Typeof
                    && matches!(&**arg, Expr::Identifier(name) if self.resolve(name).is_none() && !matches!(name.as_str(), "undefined" | "NaN" | "Infinity" | "String" | "Symbol" | "RegExp" | "Object" | "Reflect" | "Math" | "Number" | "Boolean" | "Array" | "Function" | "Proxy" | "globalThis" | "Intl" | "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError" | "isNaN" | "isFinite" | "parseInt" | "parseFloat" | "JSON"))
                {
                    let Expr::Identifier(name) = &**arg else {
                        unreachable!()
                    };
                    let index = u32::try_from(self.bytecode.constants.len())
                        .map_err(|_| CompileError::ProgramTooLarge)?;
                    self.bytecode
                        .constants
                        .push(Value::String(name.as_str().into()));
                    self.emit(Opcode::TypeofName, index)?;
                } else {
                    self.expression(arg)?;
                    self.emit(opcode, 0)?;
                }
            }
            Expr::Binary { op, left, right } => {
                let opcode = binary_opcode(*op)?;
                self.expression(left)?;
                self.expression(right)?;
                self.emit(opcode, 0)?;
            }
            Expr::PrivateIn { name, object } => {
                let owner = self.resolve_private_name(name)?;
                self.expression(object)?;
                self.constant(Value::String(name.clone().into()))?;
                self.emit(Opcode::PrivateIn, owner)?;
            }
            Expr::Logical { op, left, right } => {
                self.expression(left)?;
                self.emit(Opcode::Dup, 0)?;
                let jump = self.emit(
                    match op {
                        LogicalOp::And => Opcode::JumpIfFalse,
                        LogicalOp::Or => Opcode::JumpIfTrue,
                        LogicalOp::Nullish => Opcode::JumpIfNotNullish,
                    },
                    0,
                )?;
                self.emit(Opcode::Pop, 0)?;
                self.expression(right)?;
                self.patch(jump, self.offset()?);
            }
            Expr::Sequence(expressions) => {
                for (index, expression) in expressions.iter().enumerate() {
                    self.expression(expression)?;
                    if index + 1 != expressions.len() {
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expression(test)?;
                let no = self.emit(Opcode::JumpIfFalse, 0)?;
                self.expression(consequent)?;
                let end = self.emit(Opcode::Jump, 0)?;
                self.patch(no, self.offset()?);
                self.expression(alternate)?;
                self.patch(end, self.offset()?);
            }
            Expr::Array(elements) => {
                if elements
                    .iter()
                    .any(|element| matches!(element, Some(ArrayElement::Spread(_))))
                {
                    self.emit(Opcode::NewArray, 0)?;
                    for element in elements {
                        let kind = match element {
                            None => {
                                self.constant(Value::Undefined)?;
                                1
                            }
                            Some(ArrayElement::Normal(value)) => {
                                self.expression(value)?;
                                0
                            }
                            Some(ArrayElement::Spread(value)) => {
                                self.expression(value)?;
                                2
                            }
                        };
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    return Ok(());
                }
                let length =
                    u32::try_from(elements.len()).map_err(|_| CompileError::ProgramTooLarge)?;
                self.emit(Opcode::NewArray, length)?;
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let ArrayElement::Normal(value) = element else {
                        return Err(CompileError::Unsupported("array spread"));
                    };
                    self.emit(Opcode::Dup, 0)?;
                    self.constant(Value::String(index.to_string().into()))?;
                    self.expression(value)?;
                    self.emit(Opcode::DefineData, 0)?;
                    self.emit(Opcode::Pop, 0)?;
                }
            }
            Expr::Object(properties) => {
                self.emit(Opcode::NewObject, 0)?;
                let mut has_proto = false;
                for property in properties {
                    if let ObjectProp::Spread(value) = property {
                        self.expression(value)?;
                        self.emit(Opcode::CopyDataProperties, 0)?;
                        continue;
                    }
                    if let ObjectProp::Method { key, function }
                    | ObjectProp::Accessor { key, function, .. } = property
                    {
                        self.emit(Opcode::Dup, 0)?;
                        self.property_key(key)?;
                        self.function(function, false)?;
                        std::rc::Rc::get_mut(self.bytecode.functions.last_mut().unwrap())
                            .unwrap()
                            .constructible = false;
                        if let ObjectProp::Accessor { getter, .. } = property {
                            self.emit(Opcode::DefineAccessor, u32::from(!getter))?;
                        } else {
                            // The operand distinguishes object-literal
                            // methods (enumerable) from class methods.
                            self.emit(Opcode::DefineMethod, 1)?;
                        }
                        self.emit(Opcode::Pop, 0)?;
                        continue;
                    }
                    let ObjectProp::KeyValue {
                        key,
                        value,
                        shorthand,
                    } = property
                    else {
                        unreachable!("spread is handled above")
                    };
                    self.emit(Opcode::Dup, 0)?;
                    let prototype_key = match key {
                        PropertyKey::Identifier(name) => name == "__proto__",
                        PropertyKey::String(name) => name == "__proto__",
                        _ => false,
                    };
                    if !shorthand && prototype_key {
                        if has_proto {
                            return Err(CompileError::InvalidSyntax(
                                "duplicate literal __proto__ setter",
                            ));
                        }
                        has_proto = true;
                        self.expression(value)?;
                        self.emit(Opcode::SetLiteralPrototype, 0)?;
                    } else {
                        self.property_key(key)?;
                        self.expression(value)?;
                        self.emit(Opcode::DefineData, 0)?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
            }
            Expr::Super => {
                return Err(CompileError::InvalidSyntax(
                    "super must be used as a property access or constructor call",
                ))
            }
            Expr::ImportMeta => return Err(CompileError::Unsupported("import.meta")),
            Expr::Member {
                object,
                property,
                computed,
            } if matches!(&**object, Expr::Super) => {
                self.super_property_key(property, *computed)?;
                self.emit(Opcode::SuperGet, 0)?;
            }
            Expr::Member { .. } if private_member_name(expr).is_some() => {
                let owner = self.private_member_reference(expr)?;
                self.emit(Opcode::PrivateGet, owner)?;
            }
            Expr::Member { .. } => {
                self.member_reference(expr)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
            Expr::Parenthesized(expr) => self.expression(expr)?,
            Expr::OptionalMember { .. } => {
                return Err(CompileError::Unsupported("optional chaining"))
            }
            Expr::Assign { op, target, value } => self.assignment(*op, target, value)?,
            Expr::DestructureAssign { pattern, value } => {
                self.destructuring_assignment(pattern, value)?
            }
            Expr::Update { op, arg, prefix } => {
                if let Expr::Identifier(name) = &**arg {
                    let binding = self.resolve(name);
                    let name_index = if binding.is_none() {
                        Some(self.name_constant(name)?)
                    } else {
                        None
                    };
                    if let Some(slot) = binding {
                        self.emit(Opcode::ResolveBindingReference, slot)?;
                        self.emit(Opcode::LoadBindingReference, 0)?;
                    } else {
                        self.emit(Opcode::UnboundName, name_index.unwrap())?;
                    }
                    self.emit(Opcode::ToNumber, 0)?;
                    if !prefix {
                        self.emit(Opcode::Dup, 0)?;
                    }
                    self.constant(Value::Number(1.0))?;
                    self.emit(
                        if *op == UpdateOp::Inc {
                            Opcode::Add
                        } else {
                            Opcode::Subtract
                        },
                        0,
                    )?;
                    if binding.is_some() {
                        // Postfix update keeps the previous numeric value on
                        // the stack as its expression result.
                        self.emit(Opcode::StoreBindingReference, u32::from(!*prefix))?;
                    } else {
                        self.emit(Opcode::SetUnboundName, name_index.unwrap())?;
                    }
                    if !prefix {
                        self.emit(Opcode::Pop, 0)?;
                    }
                } else if let Expr::Member {
                    object,
                    property,
                    computed,
                } = arg.as_ref()
                {
                    if matches!(&**object, Expr::Super) {
                        self.super_property_key(property, *computed)?;
                        self.emit(
                            Opcode::SuperUpdate,
                            u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1),
                        )?;
                    } else {
                        self.member_reference(arg)?;
                        self.emit(
                            Opcode::UpdateProperty,
                            u32::from(*op == UpdateOp::Dec) | (u32::from(*prefix) << 1),
                        )?;
                    }
                } else if matches!(&**arg, Expr::Call { .. }) {
                    if self.bytecode.strict {
                        return Err(CompileError::InvalidSyntax(
                            "a CallExpression cannot be an assignment target in strict code",
                        ));
                    }
                    self.expression(arg)?;
                    self.emit(Opcode::InvalidAssignmentTarget, 0)?;
                } else {
                    return Err(CompileError::InvalidSyntax("invalid assignment/member AST"));
                }
            }
            Expr::Template {
                quasis,
                expressions,
            } => {
                if quasis.len() != expressions.len() + 1 {
                    return Err(CompileError::InvalidSyntax("invalid template AST"));
                }
                self.constant(Value::String(quasis[0].clone()))?;
                for (expr, tail) in expressions.iter().zip(&quasis[1..]) {
                    self.expression(expr)?;
                    self.emit(Opcode::ToString, 0)?;
                    self.emit(Opcode::Add, 0)?;
                    self.constant(Value::String(tail.clone()))?;
                    self.emit(Opcode::Add, 0)?;
                }
            }
            Expr::Call { callee, args } | Expr::New { callee, args } => {
                let construct = matches!(expr, Expr::New { .. });
                if !construct && matches!(&**callee, Expr::Super) {
                    if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                        self.emit(Opcode::NewArray, 0)?;
                        for arg in args {
                            let (value, kind) = match arg {
                                Argument::Normal(value) => (value, 0),
                                Argument::Spread(value) => (value, 2),
                            };
                            self.expression(value)?;
                            self.emit(Opcode::ArrayPush, kind)?;
                        }
                        self.emit(Opcode::SuperCallSpread, 0)?;
                    } else {
                        for arg in args {
                            let Argument::Normal(expr) = arg else {
                                unreachable!("super call spreads take the array path")
                            };
                            self.expression(expr)?;
                        }
                        self.emit(
                            Opcode::SuperCall,
                            u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                        )?;
                    }
                    return Ok(());
                }
                if !construct
                    && matches!(&**callee, Expr::Member { object, .. } if matches!(&**object, Expr::Super))
                {
                    let Expr::Member {
                        property, computed, ..
                    } = callee.as_ref()
                    else {
                        unreachable!()
                    };
                    self.super_property_key(property, *computed)?;
                    self.emit(Opcode::SuperGetMethod, 0)?;
                } else if !construct && matches!(&**callee, Expr::Member { .. }) {
                    if private_member_name(callee).is_some() {
                        let owner = self.private_member_reference(callee)?;
                        self.emit(Opcode::PrivateGetMethod, owner)?;
                    } else {
                        self.member_reference(callee)?;
                        self.emit(Opcode::GetMethod, 0)?;
                    }
                } else {
                    self.expression(callee)?;
                    self.constant(Value::Undefined)?;
                }
                if args.iter().any(|arg| matches!(arg, Argument::Spread(_))) {
                    self.emit(Opcode::NewArray, 0)?;
                    for arg in args {
                        let (value, kind) = match arg {
                            Argument::Normal(value) => (value, 0),
                            Argument::Spread(value) => (value, 2),
                        };
                        self.expression(value)?;
                        self.emit(Opcode::ArrayPush, kind)?;
                    }
                    self.emit(
                        if !construct
                            && matches!(&**callee, Expr::Identifier(name) if name == "eval")
                        {
                            Opcode::DirectEvalSpread
                        } else {
                            Opcode::CallSpread
                        },
                        u32::from(construct),
                    )?;
                    return Ok(());
                }
                for arg in args {
                    let Argument::Normal(expr) = arg else {
                        unreachable!("spread calls are emitted above")
                    };
                    self.expression(expr)?;
                }
                self.emit(
                    if construct {
                        Opcode::Construct
                    } else if matches!(&**callee, Expr::Identifier(name) if name == "eval") {
                        Opcode::DirectEval
                    } else {
                        Opcode::Call
                    },
                    u32::try_from(args.len()).map_err(|_| CompileError::ProgramTooLarge)?,
                )?;
            }
            Expr::This => {
                self.emit(Opcode::This, 0)?;
            }
            Expr::NewTarget => {
                if !self.bytecode.new_target_allowed {
                    return Err(CompileError::InvalidSyntax(
                        "new.target is not valid in this context",
                    ));
                }
                self.emit(Opcode::NewTarget, 0)?;
            }
            Expr::Function(function) => self.function_expression(function)?,
            Expr::Class(class) => self.class_expression(class, None)?,
            Expr::Yield { value, delegate } => {
                if !self.bytecode.generator {
                    return Err(CompileError::InvalidSyntax(
                        "yield requires a generator function",
                    ));
                }
                if *delegate {
                    return Err(CompileError::Unsupported("yield*"));
                }
                if let Some(value) = value {
                    self.expression(value)?;
                } else {
                    self.constant(Value::Undefined)?;
                }
                self.emit(Opcode::Yield, 0)?;
            }
            Expr::Await(expression) => {
                if !self.bytecode.async_function && !self.bytecode.module {
                    return Err(CompileError::InvalidSyntax(
                        "await is only valid in async functions or modules",
                    ));
                }
                self.expression(expression)?;
                self.emit(Opcode::Await, 0)?;
            }
            Expr::DynamicImport(specifier) => {
                self.expression(specifier)?;
                self.emit(Opcode::DynamicImport, 0)?;
            }
            Expr::Arrow {
                params,
                body,
                is_async,
            } => {
                let body = match body {
                    ArrowBody::Expr(expr) => vec![Stmt::Return(Some(*expr.clone()))],
                    ArrowBody::Block(body) => body.clone(),
                };
                self.function(
                    &Function {
                        name: None,
                        params: params.clone(),
                        body,
                        generator: false,
                        is_async: *is_async,
                    },
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn for_in(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.for_each(left, right, body, true, false, labels)
    }

    fn for_of(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        is_await: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.for_each(left, right, body, false, is_await, labels)
    }

    fn for_each(
        &mut self,
        left: &ForHead,
        right: &Expr,
        body: &Stmt,
        for_in: bool,
        is_await: bool,
        labels: Vec<String>,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::ClearCompletion, 0)?;
        let (pattern, kind, annex_b_initializer) = match left {
            ForHead::Decl(kind, pattern) => (Some(pattern), Some(*kind), None),
            ForHead::AnnexBVarInit(pattern, initializer) => {
                if self.bytecode.strict || !for_in {
                    return Err(CompileError::InvalidSyntax(
                        "a for-in declaration initializer is valid only in sloppy var code",
                    ));
                }
                (Some(pattern), Some(DeclKind::Var), Some(initializer))
            }
            ForHead::Pattern(pattern) => (Some(pattern), None, None),
            ForHead::Expr(_) => (None, None, None),
        };
        let lexical = kind.is_some_and(|kind| kind != DeclKind::Var);
        let mut declarations = vec![("*iterator*".to_owned(), DeclKind::Let)];
        if lexical {
            declarations.extend(
                pattern_names(pattern.expect("declaration heads have a pattern"))
                    .into_iter()
                    .map(|name| (name, kind.unwrap())),
            );
        }
        self.enter_scope(declarations, &BTreeSet::new(), false)?;
        let iterator = self.resolve("*iterator*").unwrap();
        if let Some(initializer) = annex_b_initializer {
            self.expression(initializer)?;
            self.bind_pattern(
                pattern.expect("Annex B initializer has a declaration pattern"),
                DeclKind::Var,
            )?;
        }
        self.expression(right)?;
        if for_in {
            self.emit(Opcode::ForInKeys, 0)?;
        }
        self.emit(
            if is_await {
                Opcode::GetAsyncIterator
            } else {
                Opcode::GetIterator
            },
            0,
        )?;
        self.emit(Opcode::InitializeBinding, iterator)?;
        let start = self.offset()?;
        self.emit(Opcode::GetBinding, iterator)?;
        let exit = if is_await {
            self.emit(Opcode::AsyncIteratorNext, 0)?;
            self.emit(Opcode::Await, 0)?;
            self.emit(Opcode::AsyncIteratorStep, 0)?
        } else {
            self.emit(Opcode::IteratorStep, 0)?
        };
        self.loops.push(Loop {
            labels,
            breakable: true,
            scope_depth: self.scopes.len(),
            breaks: Vec::new(),
            continues: Some(Vec::new()),
            iterator: Some(iterator),
        });
        if lexical {
            self.enter_scope(
                pattern_names(pattern.expect("declaration heads have a pattern"))
                    .into_iter()
                    .map(|name| (name, kind.unwrap()))
                    .collect(),
                &BTreeSet::new(),
                false,
            )?;
        }
        match left {
            ForHead::Decl(kind, pattern) => self.bind_pattern(pattern, *kind)?,
            ForHead::AnnexBVarInit(pattern, _) => self.bind_pattern(pattern, DeclKind::Var)?,
            ForHead::Pattern(pattern) => {
                let Pattern::Identifier(name) = pattern else {
                    return Err(CompileError::Unsupported(if for_in {
                        "a destructuring for-in assignment target"
                    } else {
                        "a destructuring for-of assignment target"
                    }));
                };
                if let Some(slot) = self.resolve(name) {
                    self.emit(Opcode::StoreBinding, slot)?;
                } else {
                    let index = self.name_constant(name)?;
                    self.emit(Opcode::SetUnboundName, index)?;
                }
                self.emit(Opcode::Pop, 0)?;
            }
            ForHead::Expr(target) => {
                if self.bytecode.strict {
                    return Err(CompileError::InvalidSyntax(
                        "a CallExpression cannot be an assignment target in strict code",
                    ));
                }
                self.expression(target)?;
                self.emit(Opcode::InvalidAssignmentTarget, 0)?;
            }
        }
        self.statement(body, false)?;
        if lexical {
            self.leave_scope()?;
        }
        self.emit(Opcode::Jump, start)?;
        let end = self.offset()?;
        self.patch(exit, end);
        let context = self.loops.pop().unwrap();
        for (jump, control) in context.breaks {
            self.patch(jump, end);
            self.bytecode.abrupt_jumps[control].target = end;
        }
        for (jump, control) in context.continues.expect("for-of loop has continue targets") {
            self.patch(jump, start);
            self.bytecode.abrupt_jumps[control].target = start;
        }
        self.leave_scope()?;
        Ok(())
    }

    fn assignment(
        &mut self,
        op: AssignOp,
        target: &Expr,
        value: &Expr,
    ) -> Result<(), CompileError> {
        let logical_assignment = is_logical_assignment(op);
        // AssignmentExpression gives an anonymous function definition the
        // syntactic IdentifierReference target's name. Member references and
        // compound assignments deliberately do not participate.
        let inferred_name = match (op, target) {
            (AssignOp::Assign, Expr::Identifier(name)) if !logical_assignment => {
                Some(name.as_str())
            }
            (_, Expr::Identifier(name)) if logical_assignment => Some(name.as_str()),
            _ => None,
        };
        // A CoverParenthesizedExpression can still evaluate to a reference,
        // but it is not an IdentifierReference for SetFunctionName.
        let target = match target {
            Expr::Parenthesized(inner) => inner.as_ref(),
            target => target,
        };
        if matches!(target, Expr::Call { .. }) {
            if self.bytecode.strict {
                return Err(CompileError::InvalidSyntax(
                    "a CallExpression cannot be an assignment target in strict code",
                ));
            }
            // Annex B's web-compat extension evaluates the call but never
            // evaluates the RHS or performs coercion on the returned value.
            self.expression(target)?;
            self.emit(Opcode::InvalidAssignmentTarget, 0)?;
            return Ok(());
        }
        if let Expr::Member {
            object,
            property,
            computed,
        } = target
        {
            if matches!(&**object, Expr::Super) {
                self.super_property_key(property, *computed)?;
                if logical_assignment {
                    self.emit(Opcode::Dup, 0)?;
                    self.emit(Opcode::SuperGet, 0)?;
                    self.logical_assignment(op, 1, value, inferred_name, Opcode::SuperSet, 0)?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::Dup, 0)?;
                    self.emit(Opcode::SuperGet, 0)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::SuperSet, 0)?;
                return Ok(());
            }
        }
        if private_member_name(target).is_some() {
            let owner = self.private_member_reference(target)?;
            if logical_assignment {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::PrivateGet, owner)?;
                self.logical_assignment(op, 2, value, inferred_name, Opcode::PrivateSet, owner)?;
                return Ok(());
            }
            if op != AssignOp::Assign {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::PrivateGet, owner)?;
            }
            self.expression_with_name(value, inferred_name)?;
            if let Some(opcode) = compound_assignment_opcode(op) {
                self.emit(opcode, 0)?;
            }
            self.emit(Opcode::PrivateSet, owner)?;
            return Ok(());
        }
        if let Expr::Identifier(name) = target {
            if self.with_depth != 0 {
                let index = self.name_constant(name)?;
                // Resolve the object-environment binding before evaluating
                // the RHS. A deletion or eval in that RHS must not redirect
                // PutValue to a later binding lookup.
                self.emit(Opcode::ResolveWithReference, index)?;
                if logical_assignment {
                    self.emit(Opcode::LoadWithReference, 0)?;
                    self.logical_assignment(
                        op,
                        2,
                        value,
                        inferred_name,
                        Opcode::StoreWithReference,
                        0,
                    )?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::LoadWithReference, 0)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::StoreWithReference, 0)?;
                return Ok(());
            }
        }
        if let Expr::Identifier(name) = target {
            if self.resolve(name).is_none() {
                let index = self.name_constant(name)?;
                if logical_assignment {
                    self.emit(Opcode::UnboundName, index)?;
                    self.logical_assignment(
                        op,
                        0,
                        value,
                        inferred_name,
                        Opcode::SetUnboundName,
                        index,
                    )?;
                    return Ok(());
                }
                if op != AssignOp::Assign {
                    self.emit(Opcode::UnboundName, index)?;
                }
                self.expression_with_name(value, inferred_name)?;
                if let Some(opcode) = compound_assignment_opcode(op) {
                    self.emit(opcode, 0)?;
                }
                self.emit(Opcode::SetUnboundName, index)?;
                return Ok(());
            }
        }
        let binding = if let Expr::Identifier(name) = target {
            if let Some(slot) = self.resolve(name) {
                Some(slot)
            } else {
                let index = u32::try_from(self.bytecode.constants.len())
                    .map_err(|_| CompileError::ProgramTooLarge)?;
                self.bytecode
                    .constants
                    .push(Value::String("globalThis".into()));
                self.emit(Opcode::Global, index)?;
                self.constant(Value::String(name.clone().into()))?;
                self.emit(Opcode::ToPropertyKey, 0)?;
                None
            }
        } else {
            if op == AssignOp::Assign {
                // A simple assignment evaluates the computed property
                // expression with its base first, but ToPropertyKey runs in
                // PutValue after the RHS. Keep the raw key on the stack for
                // SetProperty to convert at that later point.
                self.member_reference_uncoerced(target)?;
            } else {
                self.member_reference(target)?;
            }
            None
        };
        if let Some(slot) = binding {
            // Evaluate an IdentifierReference before the RHS, as required by
            // PutValue. In particular, a sloppy direct eval in the RHS may
            // introduce a same-named var binding, but it cannot retarget the
            // reference that was already resolved here.
            self.emit(Opcode::ResolveBindingReference, slot)?;
        }
        if logical_assignment {
            if binding.is_some() {
                self.emit(Opcode::LoadBindingReference, 0)?;
                self.logical_assignment(
                    op,
                    2,
                    value,
                    inferred_name,
                    Opcode::StoreBindingReference,
                    0,
                )?;
            } else {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::GetProperty, 0)?;
                self.logical_assignment(op, 2, value, inferred_name, Opcode::SetProperty, 0)?;
            }
            return Ok(());
        }
        if op != AssignOp::Assign {
            if binding.is_some() {
                self.emit(Opcode::LoadBindingReference, 0)?;
            } else {
                self.emit(Opcode::Dup2, 0)?;
                self.emit(Opcode::GetProperty, 0)?;
            }
        }
        self.expression_with_name(value, inferred_name)?;
        if let Some(opcode) = compound_assignment_opcode(op) {
            self.emit(opcode, 0)?;
        }
        if binding.is_some() {
            self.emit(Opcode::StoreBindingReference, 0)?;
        } else {
            self.emit(Opcode::SetProperty, 0)?;
        }
        Ok(())
    }

    /// The logical-assignment productions retain their original Reference
    /// across the truthiness/nullish decision. A bypass returns the existing
    /// value without evaluating the RHS or invoking PutValue; an assignment
    /// consumes the retained reference with the supplied store opcode.
    fn logical_assignment(
        &mut self,
        op: AssignOp,
        reference_values: u32,
        value: &Expr,
        inferred_name: Option<&str>,
        store: Opcode,
        store_operand: u32,
    ) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        let bypass = self.emit(
            match op {
                AssignOp::LogicalAndAssign => Opcode::JumpIfFalse,
                AssignOp::LogicalOrAssign => Opcode::JumpIfTrue,
                AssignOp::NullishAssign => Opcode::JumpIfNotNullish,
                _ => unreachable!("logical assignment helper has a logical operator"),
            },
            0,
        )?;
        self.emit(Opcode::Pop, 0)?;
        self.expression_with_name(value, inferred_name)?;
        self.emit(store, store_operand)?;
        let done = self.emit(Opcode::Jump, 0)?;
        self.patch(bypass, self.offset()?);
        if reference_values != 0 {
            self.emit(Opcode::DiscardReference, reference_values)?;
        }
        self.patch(done, self.offset()?);
        Ok(())
    }

    /// AssignmentPatternEvaluation. The first copy of the RHS is the
    /// expression's result; the second is consumed by the recursive pattern.
    fn destructuring_assignment(
        &mut self,
        pattern: &AssignmentPattern,
        value: &Expr,
    ) -> Result<(), CompileError> {
        self.expression(value)?;
        self.emit(Opcode::Dup, 0)?;
        self.assign_pattern(pattern)
    }

    fn assign_pattern(&mut self, pattern: &AssignmentPattern) -> Result<(), CompileError> {
        match pattern {
            AssignmentPattern::Target(target) => self.assign_pattern_target(target)?,
            AssignmentPattern::Array(elements) => {
                self.emit(Opcode::GetIterator, 0)?;
                for element in elements {
                    let Some(element) = element else {
                        self.emit(Opcode::IteratorElision, 0)?;
                        continue;
                    };
                    if element.rest {
                        match &element.pattern {
                            AssignmentPattern::Target(target)
                                if matches!(&**target, Expr::Member { .. }) =>
                            {
                                self.emit(Opcode::Dup, 0)?;
                                self.member_reference_uncoerced(target)?;
                                self.emit(Opcode::IteratorRestReference, 0)?;
                                self.assign_prepared_pattern_target(target)?;
                                // IteratorRestReference keeps the original
                                // record below the prepared reference while
                                // collecting. Rest exhaustion marks it done,
                                // so discard that retained record now.
                                self.emit(Opcode::Pop, 0)?;
                            }
                            _ => {
                                self.emit(Opcode::IteratorRest, 0)?;
                                self.assign_pattern(&element.pattern)?;
                            }
                        }
                        return Ok(());
                    }
                    let prepared_member_target = match &element.pattern {
                        AssignmentPattern::Target(target)
                            if matches!(&**target, Expr::Member { .. }) =>
                        {
                            self.emit(Opcode::Dup, 0)?;
                            self.member_reference_uncoerced(target)?;
                            self.array_pattern_reference_value()?;
                            Some(target.as_ref())
                        }
                        _ => {
                            self.array_pattern_value()?;
                            None
                        }
                    };
                    self.assignment_pattern_default(element.default.as_ref(), &element.pattern)?;
                    if let Some(target) = prepared_member_target {
                        self.assign_prepared_pattern_target(target)?;
                    } else {
                        self.assign_pattern(&element.pattern)?;
                    }
                }
                self.emit(Opcode::IteratorFinish, 0)?;
            }
            AssignmentPattern::Object(properties) => {
                self.emit(Opcode::RequireObject, 0)?;
                self.emit(Opcode::NewArray, 0)?;
                for property in properties {
                    match property {
                        AssignmentPatternProp::KeyValue {
                            key,
                            value,
                            default,
                        } => {
                            self.property_key(key)?;
                            let prepared_member_target = match value {
                                AssignmentPattern::Target(target)
                                    if matches!(&**target, Expr::Member { .. }) =>
                                {
                                    // Preserve the already-coerced source
                                    // key while evaluating the assignment
                                    // target reference before GetV(source,
                                    // key), as KeyedDestructuringAssignment
                                    // Evaluation requires.
                                    self.emit(Opcode::Dup, 0)?;
                                    self.member_reference_uncoerced(target)?;
                                    self.emit(Opcode::DestructurePropertyReference, 0)?;
                                    Some(target.as_ref())
                                }
                                _ => {
                                    self.emit(Opcode::DestructureProperty, 0)?;
                                    None
                                }
                            };
                            self.assignment_pattern_default(default.as_ref(), value)?;
                            if let Some(target) = prepared_member_target {
                                self.assign_prepared_pattern_target(target)?;
                            } else {
                                self.assign_pattern(value)?;
                            }
                        }
                        AssignmentPatternProp::Rest(pattern) => {
                            self.emit(Opcode::ObjectRest, 0)?;
                            self.assign_pattern(pattern)?;
                            return Ok(());
                        }
                    }
                }
                self.emit(Opcode::Pop, 0)?;
                self.emit(Opcode::Pop, 0)?;
            }
        }
        Ok(())
    }

    /// Consumes a leaf value while assigning an existing binding or member;
    /// the outer assignment pattern keeps its duplicate RHS beneath it.
    fn assign_pattern_target(&mut self, target: &Expr) -> Result<(), CompileError> {
        if let Expr::Identifier(name) = target {
            if let Some(slot) = self.resolve(name) {
                self.emit(Opcode::StoreBinding, slot)?;
            } else {
                let index = self.name_constant(name)?;
                self.emit(Opcode::SetUnboundName, index)?;
            }
        } else {
            self.member_reference(target)?;
            self.emit(Opcode::SetDestructureProperty, 0)?;
        }
        self.emit(Opcode::Pop, 0)?;
        Ok(())
    }

    /// Completes a member assignment whose object and raw key were evaluated
    /// before IteratorStep. Destructuring requires that ordering, while
    /// ToPropertyKey and PutValue happen only after the element is obtained.
    fn assign_prepared_pattern_target(&mut self, target: &Expr) -> Result<(), CompileError> {
        if !matches!(target, Expr::Member { .. }) {
            return Err(CompileError::InvalidSyntax(
                "prepared destructuring target must be a member reference",
            ));
        }
        self.emit(Opcode::SetDestructurePropertyReference, 0)?;
        self.emit(Opcode::Pop, 0)?;
        Ok(())
    }

    /// Like [`Self::array_pattern_value`], but an already-evaluated member
    /// reference is above the iterator record on the operand stack.
    fn array_pattern_reference_value(&mut self) -> Result<(), CompileError> {
        let exhausted = self.emit(Opcode::IteratorStepReference, 0)?;
        let joined = self.emit(Opcode::Jump, 0)?;
        self.patch(exhausted, self.offset()?);
        self.constant(Value::Undefined)?;
        self.patch(joined, self.offset()?);
        Ok(())
    }

    fn member_reference(&mut self, target: &Expr) -> Result<(), CompileError> {
        self.member_reference_with_key(target, true)
    }

    fn member_reference_uncoerced(&mut self, target: &Expr) -> Result<(), CompileError> {
        self.member_reference_with_key(target, false)
    }

    fn member_reference_with_key(
        &mut self,
        target: &Expr,
        coerce_key: bool,
    ) -> Result<(), CompileError> {
        let Expr::Member {
            object,
            property,
            computed,
        } = target
        else {
            return Err(CompileError::InvalidSyntax("invalid assignment/member AST"));
        };
        if matches!(&**object, Expr::Super) {
            return Err(CompileError::InvalidSyntax(
                "super member requires a dedicated operation",
            ));
        }
        self.expression(object)?;
        if *computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = &**property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed member AST",
            ));
        }
        if coerce_key {
            // Computed-property Reference evaluation requires the base to be
            // object-coercible before it converts the property key. Keep the
            // resulting canonical key beside the base so a later PutValue
            // does not repeat observable ToPropertyKey work.
            self.emit(Opcode::PreparePropertyReference, 0)?;
        }
        Ok(())
    }

    fn private_member_reference(&mut self, target: &Expr) -> Result<u32, CompileError> {
        let Expr::Member {
            object,
            property,
            computed: false,
        } = target
        else {
            return Err(CompileError::InvalidSyntax("invalid private member AST"));
        };
        let Expr::Identifier(name) = property.as_ref() else {
            return Err(CompileError::InvalidSyntax("invalid private member name"));
        };
        let Some(name) = name.strip_prefix('#') else {
            return Err(CompileError::InvalidSyntax("invalid private member name"));
        };
        let owner = self.resolve_private_name(name)?;
        self.expression(object)?;
        self.constant(Value::String(name.into()))?;
        Ok(owner)
    }

    fn name_constant(&mut self, name: &str) -> Result<u32, CompileError> {
        let index = u32::try_from(self.bytecode.constants.len())
            .map_err(|_| CompileError::ProgramTooLarge)?;
        self.bytecode.constants.push(Value::String(name.into()));
        Ok(index)
    }

    fn super_property_key(&mut self, property: &Expr, computed: bool) -> Result<(), CompileError> {
        if computed {
            self.expression(property)?;
        } else if let Expr::Identifier(name) = property {
            self.constant(Value::String(name.clone().into()))?;
        } else {
            return Err(CompileError::InvalidSyntax(
                "invalid non-computed super member AST",
            ));
        }
        // SuperGet/SuperSet own ToPropertyKey. Keeping the raw computed key
        // here makes simple `super[key] = rhs` evaluate the RHS before key
        // conversion, as PutValue requires.
        Ok(())
    }

    fn property_key(&mut self, key: &PropertyKey) -> Result<(), CompileError> {
        match key {
            PropertyKey::Identifier(name) => self.constant(Value::String(name.clone().into()))?,
            PropertyKey::String(name) => self.constant(Value::String(name.clone()))?,
            PropertyKey::Number(n) => self.constant(Value::Number(*n))?,
            PropertyKey::Computed(expr) => self.expression(expr)?,
        }
        self.emit(Opcode::ToPropertyKey, 0)?;
        Ok(())
    }

    fn function(&mut self, function: &Function, arrow: bool) -> Result<(), CompileError> {
        self.function_named(function, arrow, None, false)
    }

    fn function_expression(&mut self, function: &Function) -> Result<(), CompileError> {
        self.function_named(function, false, None, function.name.is_some())
    }

    fn class_expression(
        &mut self,
        class: &Class,
        inferred_name: Option<&str>,
    ) -> Result<(), CompileError> {
        let Some(name) = class.name.as_ref() else {
            return self.class_expression_with_binding(class, inferred_name, None);
        };
        self.enter_scope(
            vec![(name.clone(), DeclKind::Const)],
            &BTreeSet::new(),
            true,
        )?;
        let binding = self
            .resolve(name)
            .expect("class name was entered into its expression scope");
        let result = self.class_expression_with_binding(class, inferred_name, Some(binding));
        self.leave_scope()?;
        result
    }

    fn class_expression_with_binding(
        &mut self,
        class: &Class,
        inferred_name: Option<&str>,
        binding: Option<u32>,
    ) -> Result<(), CompileError> {
        let private_declarations = class_private_declarations(class)?;
        let private_scope_id = self.next_private_scope;
        self.next_private_scope = self.next_private_scope.saturating_add(1);
        let mut private_scope = HashMap::new();
        let mut private_bindings = Vec::new();
        for (name, is_static) in &private_declarations {
            // This cannot collide with source text (U+0000 is not a source
            // character), while preserving separate lexical environments for
            // nested classes that reuse a private name.
            let binding = format!(
                "{PRIVATE_OWNER_BINDING_PREFIX}{private_scope_id}_{}_{}",
                if *is_static { "static" } else { "instance" },
                name
            );
            private_scope.insert(name.clone(), binding.clone());
            private_bindings.push((binding, DeclKind::Const));
        }
        let has_private_scope = !private_bindings.is_empty();
        if has_private_scope {
            self.enter_scope(private_bindings, &BTreeSet::new(), false)?;
            self.private_scopes.push(private_scope.clone());
        }
        let constructor = class.elements.iter().find_map(|element| match element {
            ClassElement::Method {
                key,
                function,
                is_static: false,
            } if class_property_name(key).is_some_and(|name| name == "constructor") => {
                Some(function.clone())
            }
            _ => None,
        });
        let default_constructor = constructor.is_none();
        let mut constructor = constructor.unwrap_or(Function {
            name: class.name.clone(),
            params: Vec::new(),
            body: Vec::new(),
            generator: false,
            is_async: false,
        });
        constructor.name = class.name.clone();
        let mut fields: Vec<_> = class
            .elements
            .iter()
            .filter_map(|element| match element {
                ClassElement::Field {
                    key,
                    initializer,
                    is_static: false,
                } => Some(class_instance_field(key, initializer.as_ref())),
                _ => None,
            })
            .collect();
        if let Some((name, _)) = private_declarations
            .iter()
            .find(|(_, is_static)| !*is_static)
        {
            // Private methods and accessors brand each constructed instance
            // even when the class has no private data field.  The marker is
            // deliberately before all instance field initializers, so an
            // earlier public initializer can access a declared private
            // method just as it can in ECMAScript.
            fields.insert(
                0,
                Stmt::ClassPrivateBrand(
                    private_scope
                        .get(name)
                        .expect("private instance declaration has an owner binding")
                        .clone(),
                ),
            );
        }
        let constructor_body = std::mem::take(&mut constructor.body);
        let body = if class.extends.is_some() {
            if default_constructor {
                fields.clone()
            } else {
                derived_constructor_body(constructor_body, fields.clone())?
            }
        } else {
            let mut body = fields.clone();
            body.extend(constructor_body);
            body
        };
        constructor.body = body;
        self.function_named_with(
            &constructor,
            false,
            inferred_name,
            false,
            FunctionCompileOptions {
                constructible: true,
                force_strict: true,
                class_constructor: true,
                derived_constructor: class.extends.is_some(),
                default_derived_constructor: class.extends.is_some() && default_constructor,
                class_method: false,
            },
        )?;
        self.emit(Opcode::SetClassHome, 0)?;
        if let Some(base) = &class.extends {
            self.expression(base)?;
            self.emit(Opcode::SetClassHeritage, 0)?;
        }
        if let Some(slot) = binding {
            self.emit(Opcode::Dup, 0)?;
            self.emit(Opcode::InitializeBinding, slot)?;
        }
        // The class object and its prototype now exist.  Initialize the
        // hidden owner cells before creating element closures, so every
        // ordinary nested function can capture the lexical private-name
        // environment rather than relying on a [[HomeObject]].
        for (name, is_static) in &private_declarations {
            self.class_property_target(*is_static)?;
            let owner = self
                .resolve(
                    private_scope
                        .get(name)
                        .expect("private declaration has an owner binding"),
                )
                .expect("private owner binding is in the active class scope");
            self.emit(Opcode::InitializeBinding, owner)?;
        }
        for element in &class.elements {
            match element {
                ClassElement::Method {
                    key,
                    function,
                    is_static,
                } => {
                    if !is_static
                        && class_property_name(key).is_some_and(|name| name == "constructor")
                    {
                        continue;
                    }
                    self.class_property_target(*is_static)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    if private_class_name(key).is_some() {
                        self.emit(Opcode::DefinePrivateMethod, u32::from(*is_static))?;
                    } else {
                        self.emit(Opcode::DefineMethod, 0)?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
                ClassElement::Accessor {
                    key,
                    function,
                    getter,
                    is_static,
                } => {
                    self.class_property_target(*is_static)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    self.function_named_with(
                        function,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    if private_class_name(key).is_some() {
                        self.emit(
                            Opcode::DefinePrivateAccessor,
                            u32::from(!getter) | (u32::from(*is_static) << 1),
                        )?;
                    } else {
                        self.emit(Opcode::DefineClassAccessor, u32::from(!getter))?;
                        self.emit(Opcode::Pop, 0)?;
                    }
                }
                ClassElement::Field {
                    key,
                    initializer,
                    is_static: true,
                } => {
                    self.class_property_target(true)?;
                    if let Some(name) = private_class_name(key) {
                        self.constant(Value::String(name.into()))?;
                        self.emit(Opcode::DefinePrivateField, 1)?;
                        self.class_property_target(true)?;
                        self.constant(Value::String(name.into()))?;
                    } else {
                        self.property_key(key)?;
                    }
                    let value = initializer.clone().unwrap_or_else(undefined_expression);
                    let initializer = Function {
                        name: None,
                        params: Vec::new(),
                        body: vec![Stmt::Return(Some(value))],
                        generator: false,
                        is_async: false,
                    };
                    self.function_named_with(
                        &initializer,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(
                        if private_class_name(key).is_some() {
                            Opcode::DefinePrivateStaticField
                        } else {
                            Opcode::DefineClassStaticField
                        },
                        0,
                    )?;
                }
                ClassElement::Field {
                    key,
                    is_static: false,
                    ..
                } => {
                    if private_class_name(key).is_some() {
                        self.class_property_target(false)?;
                        self.constant(Value::String(
                            private_class_name(key)
                                .expect("private field check above")
                                .into(),
                        ))?;
                        self.emit(Opcode::DefinePrivateField, 0)?;
                    }
                }
                ClassElement::StaticBlock(body) => {
                    let block = Function {
                        name: None,
                        params: Vec::new(),
                        body: body.clone(),
                        generator: false,
                        is_async: false,
                    };
                    self.function_named_with(
                        &block,
                        false,
                        None,
                        false,
                        FunctionCompileOptions::class_method(),
                    )?;
                    self.emit(Opcode::CallClassStaticBlock, 0)?;
                }
            }
        }
        if has_private_scope {
            self.private_scopes.pop();
            self.leave_scope()?;
        }
        Ok(())
    }

    fn class_property_target(&mut self, is_static: bool) -> Result<(), CompileError> {
        self.emit(Opcode::Dup, 0)?;
        if !is_static {
            self.constant(Value::String("prototype".into()))?;
            self.emit(Opcode::GetProperty, 0)?;
        }
        Ok(())
    }

    fn function_named(
        &mut self,
        function: &Function,
        arrow: bool,
        inferred_name: Option<&str>,
        named_expression: bool,
    ) -> Result<(), CompileError> {
        self.function_named_with(
            function,
            arrow,
            inferred_name,
            named_expression,
            FunctionCompileOptions {
                constructible: !arrow && !function.generator && !function.is_async,
                force_strict: false,
                class_constructor: false,
                derived_constructor: false,
                default_derived_constructor: false,
                class_method: false,
            },
        )
    }

    fn function_named_with(
        &mut self,
        function: &Function,
        arrow: bool,
        inferred_name: Option<&str>,
        named_expression: bool,
        options: FunctionCompileOptions,
    ) -> Result<(), CompileError> {
        let child_budget = self.max_bytecode_bytes.saturating_sub(self.offset()?);
        let mut child = Compiler {
            bytecode: Bytecode::empty(),
            names: vec![HashMap::new()],
            private_scopes: self.private_scopes.clone(),
            next_private_scope: self.next_private_scope,
            scopes: Vec::new(),
            loops: Vec::new(),
            catch_var_slots: Vec::new(),
            max_bytecode_bytes: child_budget,
            function: true,
            local_scope: 1,
            with_depth: 0,
        };
        child.bytecode.strict =
            options.force_strict || self.bytecode.strict || strict_body(&function.body);
        validate_function_early_errors(
            function,
            child.bytecode.strict,
            !options.class_method && !options.class_constructor,
            arrow,
        )?;
        child.bytecode.arrow = arrow;
        // Arrow functions inherit the containing function's `new.target`
        // syntactic context. A regular nested function introduces its own
        // context (whose runtime value may still be `undefined`).
        child.bytecode.new_target_allowed = !arrow || self.bytecode.new_target_allowed;
        child.bytecode.generator = function.generator;
        child.bytecode.async_function = function.is_async;
        child.bytecode.constructible = options.constructible;
        child.bytecode.class_constructor = options.class_constructor;
        child.bytecode.derived_constructor = options.derived_constructor;
        child.bytecode.function_name = function
            .name
            .clone()
            .or_else(|| inferred_name.map(str::to_owned))
            .unwrap_or_default();
        child.bytecode.function_length = function
            .params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count() as u32;
        let mut visible = std::collections::BTreeMap::new();
        for scope in &self.names {
            visible.extend(scope.iter().map(|(name, slot)| (name.clone(), *slot)));
        }
        for (name, slot) in visible {
            let index = child.bytecode.bindings.len() as u32;
            child.names[0].insert(name, index);
            child
                .bytecode
                .bindings
                .push(self.bytecode.bindings[slot as usize].clone());
            child.bytecode.captures.push(slot);
        }
        if named_expression {
            let name = function
                .name
                .as_ref()
                .expect("named function expression has a name")
                .clone();
            let slot = u32::try_from(child.bytecode.bindings.len())
                .map_err(|_| CompileError::ProgramTooLarge)?;
            child.names[0].insert(name.clone(), slot);
            child.bytecode.bindings.push(Binding {
                name,
                mutable: false,
                strict_immutable: false,
                lexical: true,
                catch_parameter: false,
            });
            child.bytecode.self_slot = Some(slot);
        }
        let mut vars = top_level_var_names(&function.body)?;
        let parameters: BTreeSet<_> = function
            .params
            .iter()
            .flat_map(|param| pattern_names(&param.pattern))
            .collect();
        let lexical = lexical_names(&function.body)?;
        if !child.bytecode.strict {
            vars.extend(
                annex_b_function_names(&function.body, &lexical)
                    .into_iter()
                    .filter(|name| !lexical.iter().any(|(lexical_name, _)| lexical_name == name)),
            );
        }
        if let Some((name, _)) = lexical.iter().find(|(name, _)| parameters.contains(name)) {
            return Err(CompileError::DuplicateBinding(name.clone()));
        }
        let simple_parameter_list = function.params.iter().all(|param| {
            !param.rest
                && param.default.is_none()
                && matches!(param.pattern, Pattern::Identifier(_))
        });
        // Non-simple formal parameters need the separate parameter/body
        // environment even when a destructuring pattern has no computed key
        // or default. The same distinction selects unmapped arguments.
        let parameter_expressions = !simple_parameter_list;
        child.bytecode.generator_initializes_parameters = parameter_expressions;
        // Arrow functions inherit `arguments`; ordinary functions introduce a
        // fresh binding unless a formal or a function-body lexical declaration
        // already occupies that name.  A `var arguments` declaration shares
        // this function binding rather than creating another one.
        let arguments_needed = !arrow
            && !parameters.contains("arguments")
            && !lexical.iter().any(|(name, _)| name == "arguments");
        if parameter_expressions {
            // Parameter expressions must not resolve into body declarations.
            // All parameter cells exist, uninitialized, before the first
            // initializer; closures keep those cells when the body later
            // creates a separate variable environment.
            let mut parameter_bindings: Vec<_> = parameters
                .iter()
                .map(|name| (name.clone(), DeclKind::Let))
                .collect();
            if arguments_needed {
                parameter_bindings.push(("arguments".into(), DeclKind::Let));
                // The arguments binding lives in the parameter environment.
                // A body `var arguments` is its redeclaration, not a second
                // binding in the body variable environment.
                vars.remove("arguments");
            }
            child.enter_scope(parameter_bindings, &BTreeSet::new(), true)?;
        } else {
            vars.extend(parameters.iter().cloned());
            if arguments_needed {
                vars.insert("arguments".into());
            }
            child.enter_scope(lexical.clone(), &vars, true)?;
        }
        if arguments_needed {
            let slot = child
                .resolve("arguments")
                .expect("function arguments binding was entered");
            child.bytecode.arguments_slot = Some(slot);
            if !child.bytecode.strict && simple_parameter_list {
                child.bytecode.arguments_mapped = true;
                let mut mapped_names = BTreeSet::new();
                let mut mapped_slots = vec![None; function.params.len()];
                for (index, parameter) in function.params.iter().enumerate().rev() {
                    let Pattern::Identifier(name) = &parameter.pattern else {
                        unreachable!("simple parameter list contains only identifiers")
                    };
                    if mapped_names.insert(name.clone()) {
                        mapped_slots[index] = child.resolve(name);
                    }
                }
                child.bytecode.arguments_mapped_slots = mapped_slots;
            }
            child.emit(Opcode::ArgumentsObject, 0)?;
        }
        for (index, param) in function.params.iter().enumerate() {
            child.emit(
                if param.rest {
                    Opcode::RestArguments
                } else {
                    Opcode::Argument
                },
                index as u32,
            )?;
            child.binding_pattern_default(param.default.as_ref(), &param.pattern)?;
            child.bind_pattern(&param.pattern, DeclKind::Let)?;
        }
        if parameter_expressions {
            let parameter_slots = child.names.last().unwrap().clone();
            child.local_scope = child.names.len();
            child.enter_scope(lexical, &vars, true)?;
            child.bytecode.variable_scope = child.scopes.last().copied().unwrap();
            // A redeclared var starts with the parameter's value. A function
            // declaration instead supplies its own value during hoisting.
            for name in vars.intersection(&parameters) {
                if function.body.iter().any(|statement| matches!(statement, Stmt::FunctionDecl(function) if function.name.as_ref() == Some(name))) {
                    continue;
                }
                child.emit(Opcode::GetBinding, parameter_slots[name])?;
                child.emit(
                    Opcode::InitializeBinding,
                    child.names[child.local_scope][name],
                )?;
            }
        }
        if child.bytecode.generator {
            child.bytecode.generator_entry = child.offset()?;
        }
        if options.default_derived_constructor {
            child.emit(Opcode::SuperCallForward, 0)?;
            child.emit(Opcode::Pop, 0)?;
        }
        child.statements(&function.body)?;
        child.constant(Value::Undefined)?;
        child.emit(Opcode::Return, 0)?;
        let child_bytes = child_budget - child.max_bytecode_bytes + child.offset()?;
        self.max_bytecode_bytes = self
            .max_bytecode_bytes
            .checked_sub(child_bytes)
            .ok_or(CompileError::ProgramTooLarge)?;
        let index = self.bytecode.functions.len() as u32;
        self.bytecode
            .functions
            .push(std::rc::Rc::new(child.bytecode));
        self.emit(Opcode::Closure, index)?;
        Ok(())
    }

    fn self_tail_call_args<'a>(&self, value: &'a Expr) -> Option<&'a [Argument]> {
        let Expr::Call { callee, args } = value else {
            return None;
        };
        let Expr::Identifier(name) = callee.as_ref() else {
            return None;
        };
        let slot = self.bytecode.self_slot?;
        (self.bytecode.strict
            && self.resolve(name) == Some(slot)
            && args
                .iter()
                .all(|argument| matches!(argument, Argument::Normal(_))))
        .then_some(args)
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
        Expr::Call { callee, args } | Expr::New { callee, args } => {
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
/// More complex control flow needs a dedicated derived-this state machine;
/// report it as unsupported instead of initializing fields at an incorrect
/// point.
fn derived_constructor_body(
    mut body: Vec<Stmt>,
    fields: Vec<Stmt>,
) -> Result<Vec<Stmt>, CompileError> {
    if fields.is_empty() {
        return Ok(body);
    }
    let Some(index) = body.iter().position(|statement| matches!(statement, Stmt::Expr(Expr::Call { callee, .. }) if matches!(&**callee, Expr::Super))) else {
        return Err(CompileError::Unsupported("instance fields in an explicit derived constructor without a direct super() call"));
    };
    body.splice(index + 1..index + 1, fields);
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
