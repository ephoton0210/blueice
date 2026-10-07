// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Name binding and deterministic, deliberately bounded type checking.

use crate::compiler::{is_declaration_module, Project};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    BindingPattern, Declaration, FunctionBodyItem, FunctionCatchClause, FunctionDeclaration,
    FunctionElseBranch, FunctionIfStatement, FunctionTryStatement, FunctionWhileStatement,
    InterfaceDeclaration, Module, Parameter, TupleTypeElement, TypeField, TypeParameter,
};
use crate::syntax::{Token, TokenKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub use crate::parser::Type;

mod inference;
use inference::infer_call_substitutions;

mod checking;
pub(crate) use checking::check_incremental;
mod member_calls;
use member_calls::{member_call_parts, member_call_ranges};
mod properties;
mod scopes;
pub(crate) use scopes::flow::VERSION as FLOW_VERSION;
mod type_operators;
mod type_relations;
use properties::{
    contains_readonly_member, mutation_field_type, property_type, readonly_property, PropertyType,
    TypeExpansionBudget,
};
pub(crate) use type_relations::type_label;
use type_relations::{
    accepts_strict_unknown, complete_type_arguments, expand_concrete_tuple_spreads,
    instantiate_named, is_assignable, substitute_type, tuple_type_at_length, type_identity,
    TupleSpreadError,
};

const MAX_LITERAL_INFERENCE_CONTAINERS: usize = 128;
const MAX_LOGICAL_ASSIGNMENT_INFERENCE_OPERATORS: usize = 128;
const INFERRED_LOGICAL_ASSIGNMENT_OPERATORS: &[&str] = &["&&=", "||=", "??="];
const MAX_LOGICAL_EXPRESSION_INFERENCE_OPERATORS: usize = 128;
const INFERRED_LOGICAL_EXPRESSION_OPERATORS: &[&str] = &["&&", "||", "??"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Import,
    TypeAlias,
    Interface,
    Variable,
    Function,
}

/// A source-level binding.  It has no runtime identity and is suitable for
/// diagnostics, declarations, source maps, and future debugger metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub module: String,
    pub span: SourceSpan,
    pub exported: bool,
    pub value_type: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct CheckedModule {
    pub module: Module,
    pub symbols: Vec<Symbol>,
    /// Return types inferred without changing source annotations or runtime policy.
    pub(crate) inferred_returns: BTreeMap<usize, Type>,
    pub(crate) inferred_parameters: BTreeMap<usize, Type>,
    /// The namespaces the module exports, as an importer binds them.
    pub(crate) namespace_exports: BTreeMap<String, NamespaceExport>,
    /// Checked runtime bindings, retained for incremental importers.
    pub(crate) value_exports: BTreeMap<String, module::ExportedValue>,
}

#[derive(Debug, Clone)]
pub struct CheckedProject {
    pub modules: BTreeMap<String, CheckedModule>,
}

/// A locally bound type declaration.  Keeping its parameters alongside its
/// body lets the checker instantiate erased generic aliases and interfaces at
/// their use sites without making a type parameter visible outside its own
/// declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeDefinition {
    kind: TypeDefinitionKind,
    parameters: Vec<TypeParameter>,
    value: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeDefinitionKind {
    Alias,
    Interface,
    /// An original library interface; owner/source replacements use Interface.
    LibraryInterface,
    Class,
    /// An enum type: the union of its member types.
    Enum,
    /// One member of an enum, keyed `E.A`. Its value is the member's constant
    /// as a literal type (`0`, `"a"`); the type the member has in annotations
    /// and expressions is the literal type named `E.A`.
    EnumMember,
    /// The value `E`: an object with the enum's members, keyed `typeof E`.
    EnumObject,
    /// The value `N` of a namespace: an object with its exported values, keyed
    /// `typeof N`.
    NamespaceObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionSignature {
    parameters: Vec<Parameter>,
    type_parameters: Vec<TypeParameter>,
    return_type: Type,
}

#[derive(Clone, PartialEq, Eq)]
struct ClassConstructorBinding {
    signatures: Vec<FunctionSignature>,
    /// An omitted derived constructor whose base signature is not yet bound.
    inherited: bool,
    /// Who may `new` the class: an omitted constructor takes its base's.
    visibility: crate::parser::Visibility,
}

#[derive(Clone, PartialEq, Eq)]
struct ExportedClass {
    source_name: String,
    type_parameters: Vec<TypeParameter>,
    instance_type: Type,
    constructor_type: Type,
    constructor_binding: ClassConstructorBinding,
    value_exported: bool,
    heritage_depth: usize,
}

/// An enum another module can import: its members, evaluated.
#[derive(Clone)]
pub(crate) struct ExportedEnum {
    pub(crate) is_const: bool,
    pub(crate) declared: bool,
    pub(crate) members: Vec<crate::enum_eval::EvaluatedMember>,
}

/// The enums each module exports, by module and exported name.
pub(crate) fn exported_enums(
    project: &Project,
) -> BTreeMap<String, BTreeMap<String, ExportedEnum>> {
    project::exported_enums(project)
}

struct ProjectExports {
    values: BTreeMap<String, BTreeMap<String, module::ExportedValue>>,
    /// The names each module exports (and whether more may be re-exported), to
    /// check what is imported.
    exported_names: BTreeMap<String, (BTreeSet<String>, bool)>,
    enums: BTreeMap<String, BTreeMap<String, ExportedEnum>>,
    types: BTreeMap<String, BTreeMap<String, TypeDefinition>>,
    classes: BTreeMap<String, BTreeMap<String, ExportedClass>>,
}

/// Static declarations selected by the host and injected into ordinary source
/// modules after their local bindings. These declarations have no emitted
/// JavaScript, symbols, runtime values, or resolution authority of their own.
/// A source module's local/imported name deliberately wins over an ambient
/// name, matching the ordinary lexical lookup model without making host
/// metadata look like a source-local debugger symbol.
#[derive(Default)]
struct AmbientDeclarations {
    types: BTreeMap<String, TypeDefinition>,
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    binding_kinds: BTreeMap<String, scopes::BindingKind>,
}

fn ambient_declarations(project: &Project) -> (AmbientDeclarations, Vec<Diagnostic>) {
    let mut ambient = AmbientDeclarations::default();
    let mut diagnostics = Vec::new();
    for module_id in &project.ambient_declaration_modules {
        let Some(module) = project.modules.get(module_id) else {
            continue;
        };
        for declaration in &module.declarations {
            match declaration {
                Declaration::TypeAlias(alias) => insert_ambient_type(
                    &mut ambient,
                    &mut diagnostics,
                    &alias.name,
                    TypeDefinition {
                        kind: TypeDefinitionKind::Alias,
                        parameters: alias.type_parameters.clone(),
                        value: alias.value.clone(),
                    },
                    &alias.span,
                ),
                Declaration::Interface(interface) => insert_ambient_type(
                    &mut ambient,
                    &mut diagnostics,
                    &interface.name,
                    TypeDefinition {
                        kind: TypeDefinitionKind::Interface,
                        parameters: interface.type_parameters.clone(),
                        value: interface_value(interface),
                    },
                    &interface.span,
                ),
                Declaration::Variable(variable) if variable.declared => {
                    insert_ambient_value(
                        &mut ambient,
                        &mut diagnostics,
                        &variable.name,
                        variable.annotation.clone().unwrap_or(Type::Unknown),
                        &variable.span,
                    );
                    if variable.kind == crate::parser::VariableKind::Const {
                        ambient
                            .binding_kinds
                            .insert(variable.name.clone(), scopes::BindingKind::Const);
                    }
                }
                Declaration::Function(function) if function.declared || function.overload => {
                    insert_ambient_function(&mut ambient, &mut diagnostics, function);
                    ambient
                        .binding_kinds
                        .insert(function.name.clone(), scopes::BindingKind::Function);
                }
                _ => {}
            }
        }
    }
    (ambient, diagnostics)
}

fn insert_ambient_type(
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    definition: TypeDefinition,
    span: &SourceSpan,
) {
    if ambient.types.insert(name.to_string(), definition).is_some() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DuplicateDeclaration,
            span.clone(),
            format!("duplicate ambient declaration of `{name}`"),
        ));
    }
}

fn insert_ambient_value(
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    value: Type,
    span: &SourceSpan,
) {
    if ambient.values.insert(name.to_string(), value).is_some() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DuplicateDeclaration,
            span.clone(),
            format!("duplicate ambient declaration of `{name}`"),
        ));
    }
}

fn insert_ambient_function(
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    function: &FunctionDeclaration,
) {
    let signature = FunctionSignature {
        parameters: function.parameters.clone(),
        type_parameters: function.type_parameters.clone(),
        return_type: function.return_type.clone().unwrap_or(Type::Unknown),
    };
    if ambient.values.contains_key(&function.name)
        && !ambient.functions.contains_key(&function.name)
    {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::DuplicateDeclaration,
            function.span.clone(),
            format!("duplicate ambient declaration of `{}`", function.name),
        ));
        return;
    }
    ambient
        .values
        .entry(function.name.clone())
        .or_insert_with(|| signature.return_type.clone());
    ambient
        .functions
        .entry(function.name.clone())
        .or_default()
        .push(signature);
}

mod module;
pub(crate) use module::CheckerPolicy;
pub(crate) use module::NamespaceExport;

/// What each module exports as a namespace, by module and exported name.
type NamespaceExports = BTreeMap<String, BTreeMap<String, NamespaceExport>>;
mod project;

fn interface_value(interface: &InterfaceDeclaration) -> Type {
    interface_value_with_heritage(&interface.heritage, &interface.fields)
}

fn interface_value_with_heritage(heritage: &[Type], fields: &[TypeField]) -> Type {
    if heritage.is_empty() {
        return Type::Record(fields.to_vec());
    }
    let mut parts = heritage.to_vec();
    if !fields.is_empty() {
        parts.push(Type::Record(fields.to_vec()));
    }
    if parts.len() == 1 {
        parts.pop().expect("one inherited interface type")
    } else {
        Type::Intersection(parts)
    }
}

fn function_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Option<BTreeMap<String, Type>> {
    match explicit_type_arguments {
        Some(arguments) => complete_type_arguments(&signature.type_parameters, arguments)
            .map(|arguments| type_parameter_substitutions(&signature.type_parameters, arguments)),
        None => Some(infer_call_substitutions(
            signature, actuals, aliases, budget,
        )),
    }
}

fn function_signature_matches(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
    checking: crate::CheckingOptions,
) -> Result<bool, ()> {
    if !function_signature_accepts_argument_count(signature, actuals.len()) {
        return Ok(false);
    }
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    budget.checking = checking;
    let Some(substitutions) = function_call_substitutions(
        signature,
        actuals,
        explicit_type_arguments,
        aliases,
        &mut budget,
    ) else {
        return Ok(false);
    };
    if budget.exhausted {
        return Err(());
    }
    for parameter in &signature.type_parameters {
        let Some(constraint) = &parameter.constraint else {
            continue;
        };
        let actual = substitutions
            .get(&parameter.name)
            .expect("function substitutions contain every type parameter");
        let expected = substitute_type(constraint, &substitutions);
        if !is_assignable(actual, &expected, aliases, &mut HashSet::new(), &mut budget) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    for (index, actual) in actuals.iter().enumerate() {
        let parameter = function_parameter_for_argument(signature, index)
            .expect("a matching function signature has a parameter for every argument");
        let expected = call_parameter_expected_type(parameter, &substitutions);
        if !is_assignable(actual, &expected, aliases, &mut HashSet::new(), &mut budget) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    Ok(true)
}

fn overload_is_compatible_with_implementation(
    overload: &FunctionDeclaration,
    implementation: &FunctionDeclaration,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let overload_required = overload
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    let implementation_required = implementation
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if overload_required < implementation_required
        || overload.parameters.len() > implementation.parameters.len()
    {
        return Ok(false);
    }
    let overload_substitutions = type_parameter_constraint_substitutions(&overload.type_parameters);
    let implementation_substitutions =
        type_parameter_constraint_substitutions(&implementation.type_parameters);
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for (overload_parameter, implementation_parameter) in
        overload.parameters.iter().zip(&implementation.parameters)
    {
        let actual = parameter_expected_type(overload_parameter, &overload_substitutions);
        let expected =
            parameter_expected_type(implementation_parameter, &implementation_substitutions);
        if !is_assignable(
            &actual,
            &expected,
            aliases,
            &mut HashSet::new(),
            &mut budget,
        ) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    let actual = overload
        .return_type
        .as_ref()
        .map(|value| substitute_type(value, &overload_substitutions))
        .unwrap_or(Type::Unknown);
    let expected = implementation
        .return_type
        .as_ref()
        .map(|value| substitute_type(value, &implementation_substitutions))
        .unwrap_or(Type::Unknown);
    let compatible = is_assignable(
        &actual,
        &expected,
        aliases,
        &mut HashSet::new(),
        &mut budget,
    );
    if budget.exhausted {
        Err(())
    } else {
        Ok(compatible)
    }
}

fn parameter_expected_type(parameter: &Parameter, substitutions: &BTreeMap<String, Type>) -> Type {
    let value = parameter
        .annotation
        .as_ref()
        .map(|annotation| substitute_type(annotation, substitutions))
        .unwrap_or(Type::Unknown);
    if parameter.optional {
        Type::Union(vec![value, Type::Undefined])
    } else {
        value
    }
}

fn call_parameter_expected_type(
    parameter: &Parameter,
    substitutions: &BTreeMap<String, Type>,
) -> Type {
    let value = parameter_expected_type(parameter, substitutions);
    if parameter.rest {
        match value {
            Type::Array(element) => *element,
            _ => Type::Unknown,
        }
    } else {
        value
    }
}

fn rest_parameter_element_annotation(parameter: &Parameter) -> Option<&Type> {
    match parameter.annotation.as_ref()? {
        Type::Array(element) => Some(element),
        _ => None,
    }
}

fn function_signature_required_arguments(signature: &FunctionSignature) -> usize {
    signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.rest && !parameter.optional)
        .count()
}

fn function_signature_accepts_argument_count(
    signature: &FunctionSignature,
    actual_count: usize,
) -> bool {
    actual_count >= function_signature_required_arguments(signature)
        && (signature
            .parameters
            .last()
            .is_some_and(|parameter| parameter.rest)
            || actual_count <= signature.parameters.len())
}

fn function_parameter_for_argument(
    signature: &FunctionSignature,
    argument_index: usize,
) -> Option<&Parameter> {
    signature.parameters.get(argument_index).or_else(|| {
        signature
            .parameters
            .last()
            .filter(|parameter| parameter.rest)
    })
}

fn type_parameter_constraint_substitutions(parameters: &[TypeParameter]) -> BTreeMap<String, Type> {
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.clone(),
                parameter.constraint.clone().unwrap_or(Type::Unknown),
            )
        })
        .collect()
}

fn type_parameter_substitutions(
    parameters: &[TypeParameter],
    arguments: Vec<Type>,
) -> BTreeMap<String, Type> {
    parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect()
}

struct DirectCall<'a> {
    callee: &'a Token,
    arguments: &'a [Token],
    generic: bool,
}

fn direct_call_parts(tokens: &[Token]) -> Option<DirectCall<'_>> {
    let callee = tokens.first()?;
    if callee.kind != TokenKind::Identifier {
        return None;
    }
    if tokens.get(1).is_some_and(|token| token.is("(")) {
        return Some(DirectCall {
            callee,
            arguments: &tokens[2..],
            generic: false,
        });
    }
    if !tokens.get(1).is_some_and(|token| token.is("<")) {
        return None;
    }
    let close = matching_call_angle_bracket(tokens, 1)?;
    if !tokens.get(close + 1).is_some_and(|token| token.is("(")) {
        return None;
    }
    Some(DirectCall {
        callee,
        arguments: &tokens[close + 2..],
        generic: true,
    })
}

struct ConstructorCall<'a> {
    callee: Token,
    arguments: &'a [Token],
    receiver: Option<&'a Token>,
    generic: bool,
}

fn constructor_call_parts(tokens: &[Token]) -> Option<ConstructorCall<'_>> {
    let tokens = strip_outer_parentheses(tokens);
    let keyword = tokens.first()?;
    let first = tokens.get(1)?;
    if !keyword.is("new") || first.kind != TokenKind::Identifier {
        return None;
    }
    let mut callee = first.clone();
    let mut index = 2;
    while tokens.get(index).is_some_and(|token| token.is(".")) {
        let member = tokens.get(index + 1)?;
        if member.kind != TokenKind::Identifier {
            return None;
        }
        callee.text.push('.');
        callee.text.push_str(&member.text);
        callee.end = member.end;
        index += 2;
    }
    let receiver = (index > 2).then_some(first);
    let generic = tokens.get(index).is_some_and(|token| token.is("<"));
    if generic {
        index = explicit_generic_call_close(tokens, index - 1)? + 1;
    }
    if !tokens.get(index).is_some_and(|token| token.is("(")) {
        return None;
    }
    let arguments = &tokens[index + 1..];
    split_call_arguments(arguments)?;
    Some(ConstructorCall {
        callee,
        arguments,
        receiver,
        generic,
    })
}

fn matching_call_angle_bracket(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_call_arguments(tokens: &[Token]) -> Option<Vec<&[Token]>> {
    let mut depth = 0usize;
    let mut close = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" if depth == 0 => {
                close = Some(index);
                break;
            }
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    let close = close?;
    if close + 1 != tokens.len() || depth != 0 {
        return None;
    }
    if close == 0 {
        return Some(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, token) in tokens[..close].iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "," if depth == 0 => {
                arguments.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    arguments.push(&tokens[start..close]);
    Some(arguments)
}

/// The type of a single-token element (a literal or a variable) inside a
/// tuple literal, widening a string or template literal to `string`.
fn infer_simple(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let Some(first) = tokens.first() else {
        return Type::Undefined;
    };
    if first.kind == TokenKind::String || first.kind == TokenKind::Template {
        Type::String
    } else if first.kind == TokenKind::Number {
        Type::Number
    } else if matches!(first.text.as_str(), "true" | "false") {
        Type::Boolean
    } else if first.text == "null" {
        Type::Null
    } else if first.text == "undefined" {
        scope.get("undefined").cloned().unwrap_or(Type::Undefined)
    } else if first.kind == TokenKind::Identifier {
        scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
    } else {
        Type::Unknown
    }
}

/// Removes matching parentheses that wrap an entire expression. This does not
/// parse JavaScript generally; it only exposes a nested expression to the
/// bounded inference rules below.
fn strip_outer_parentheses(mut tokens: &[Token]) -> &[Token] {
    while tokens.len() >= 2 && tokens.first().is_some_and(|token| token.is("(")) {
        let mut depth = 0usize;
        let mut closes_at_end = false;
        for (index, token) in tokens.iter().enumerate() {
            match token.text.as_str() {
                "(" => depth += 1,
                ")" if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        closes_at_end = index + 1 == tokens.len();
                        break;
                    }
                }
                _ => {}
            }
        }
        if !closes_at_end {
            break;
        }
        tokens = &tokens[1..tokens.len() - 1];
    }
    tokens
}

/// Returns the operands and final top-level operator from `operators`.
/// Selecting the final occurrence preserves left associativity for the
/// bounded expression operators this checker supports.
fn top_level_binary_parts<'a>(
    tokens: &'a [Token],
    operators: &[&str],
    is_explicit_generic_call: impl Fn(usize) -> bool,
) -> Option<(&'a [Token], &'a Token, &'a [Token])> {
    let mut depth = 0usize;
    let mut operator_index = None;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind == TokenKind::Identifier && is_explicit_generic_call(token.start) {
            if let Some(close) = explicit_generic_call_close(tokens, index) {
                index = close + 1;
                continue;
            }
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            _ if depth == 0
                && operators.contains(&token.text.as_str())
                && !is_prefix_arithmetic_operator(tokens, index)
                && !is_shift_operator_token(tokens, index) =>
            {
                operator_index = Some(index);
            }
            _ => {}
        }
        index += 1;
    }
    let index = operator_index?;
    (index > 0 && index + 1 < tokens.len()).then_some((
        &tokens[..index],
        &tokens[index],
        &tokens[index + 1..],
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftOperator {
    Left,
    Right,
    UnsignedRight,
}

impl ShiftOperator {
    fn text(self) -> &'static str {
        match self {
            Self::Left => "<<",
            Self::Right => ">>",
            Self::UnsignedRight => ">>>",
        }
    }
}

/// Returns the operands and final top-level shift operator. Runtime `>>` and
/// `>>>` arrive as adjacent `>` tokens because the parser splits generic
/// closers for type syntax, so recognize only source-contiguous runs here.
fn top_level_shift_parts(
    tokens: &[Token],
    is_explicit_generic_call: impl Fn(usize) -> bool,
) -> Option<(&[Token], ShiftOperator, &[Token])> {
    let mut depth = 0usize;
    let mut operator = None;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind == TokenKind::Identifier && is_explicit_generic_call(token.start) {
            if let Some(close) = explicit_generic_call_close(tokens, index) {
                index = close + 1;
                continue;
            }
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            _ if depth == 0 => {
                if let Some((kind, width)) = shift_operator_at(tokens, index) {
                    operator = Some((index, kind, width));
                    index += width;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let (index, operator, width) = operator?;
    (index > 0 && index + width < tokens.len()).then_some((
        &tokens[..index],
        operator,
        &tokens[index + width..],
    ))
}

fn is_shift_operator_token(tokens: &[Token], index: usize) -> bool {
    (index.saturating_sub(2)..=index).any(|start| {
        shift_operator_at(tokens, start)
            .is_some_and(|(_, width)| start <= index && index < start + width)
    })
}

fn shift_operator_at(tokens: &[Token], index: usize) -> Option<(ShiftOperator, usize)> {
    if tokens.get(index).is_some_and(|token| token.is("<<")) {
        return Some((ShiftOperator::Left, 1));
    }
    let first = tokens.get(index)?;
    let second = tokens.get(index + 1)?;
    if !first.is(">") || !second.is(">") || first.end != second.start {
        return None;
    }
    if let Some(third) = tokens.get(index + 2) {
        if third.is(">") && second.end == third.start {
            return Some((ShiftOperator::UnsignedRight, 3));
        }
    }
    Some((ShiftOperator::Right, 2))
}

fn is_prefix_arithmetic_operator(tokens: &[Token], index: usize) -> bool {
    if !tokens
        .get(index)
        .is_some_and(|token| matches!(token.text.as_str(), "+" | "-"))
    {
        return false;
    }
    index == 0
        || tokens.get(index - 1).is_some_and(|previous| {
            matches!(
                previous.text.as_str(),
                "(" | "[" | "{" | "?" | ":" | "," | "=" | "+" | "-" | "*" | "/" | "%"
            )
        })
}

/// Finds the closing angle bracket for a parser-confirmed explicit generic
/// call. The parser's source-offset table disambiguates these brackets from
/// ordinary relational operators before this bounded expression scan runs.
fn explicit_generic_call_close(tokens: &[Token], callee_index: usize) -> Option<usize> {
    if !tokens
        .get(callee_index + 1)
        .is_some_and(|token| token.is("<"))
    {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(callee_index + 1) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return tokens
                        .get(index + 1)
                        .is_some_and(|token| token.is("("))
                        .then_some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits one top-level conditional expression into its condition and branch
/// expressions. Nested conditionals are accounted for before accepting their
/// matching colon, so `a ? b : c ? d : e` remains well formed.
fn conditional_expression_parts(tokens: &[Token]) -> Option<(&[Token], &[Token], &[Token])> {
    let mut depth = 0usize;
    let mut question_index = None;
    let mut nested_conditionals = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "?" if depth == 0 => {
                if question_index.is_none() {
                    question_index = Some(index);
                } else {
                    nested_conditionals += 1;
                }
            }
            ":" if depth == 0 && question_index.is_some() => {
                if nested_conditionals == 0 {
                    let question_index = question_index.expect("conditional question index exists");
                    return (question_index > 0
                        && index > question_index + 1
                        && index + 1 < tokens.len())
                    .then_some((
                        &tokens[..question_index],
                        &tokens[question_index + 1..index],
                        &tokens[index + 1..],
                    ));
                }
                nested_conditionals -= 1;
            }
            _ => {}
        }
    }
    None
}

fn infer_logical_expression(left: Type, right: Type) -> Type {
    if left == Type::Boolean && right == Type::Boolean {
        Type::Boolean
    } else {
        merge_conditional_branch_types(left, right)
    }
}

fn infer_additive_expression(operator: &Token, left: Type, right: Type) -> Type {
    if operator.is("+") && (left == Type::String || right == Type::String) {
        Type::String
    } else {
        infer_numeric_binary_expression(left, right)
    }
}

fn infer_numeric_binary_expression(left: Type, right: Type) -> Type {
    if left == Type::Number && right == Type::Number {
        Type::Number
    } else {
        Type::Unknown
    }
}

fn is_known_primitive_type(value: &Type) -> bool {
    matches!(
        value,
        Type::Boolean | Type::Number | Type::String | Type::Null | Type::Undefined
    )
}

fn is_strict_equality_primitive_type(value: &Type) -> bool {
    matches!(value, Type::Boolean | Type::Number | Type::String)
}

fn infer_nullish_coalescing_expression(left: Type, right: Type) -> Type {
    match exclude_nullish_type(left) {
        None => right,
        Some(left) => merge_conditional_branch_types(left, right),
    }
}

fn exclude_nullish_type(value: Type) -> Option<Type> {
    match value {
        Type::Null | Type::Undefined => None,
        Type::Union(values) => {
            let mut values = values
                .into_iter()
                .filter(|value| !matches!(value, Type::Null | Type::Undefined))
                .collect::<Vec<_>>();
            match values.len() {
                0 => None,
                1 => values.pop(),
                _ => Some(Type::Union(values)),
            }
        }
        value => Some(value),
    }
}

fn merge_conditional_branch_types(consequent: Type, alternate: Type) -> Type {
    if consequent == alternate {
        consequent
    } else {
        Type::Union(vec![consequent, alternate])
    }
}

#[cfg(test)]
mod tests;
