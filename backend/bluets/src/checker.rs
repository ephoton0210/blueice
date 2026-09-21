// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Name binding and deterministic, deliberately bounded type checking.

use crate::compiler::{is_declaration_module, Project};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    Declaration, FunctionBodyItem, FunctionDeclaration, FunctionElseBranch, FunctionIfStatement,
    InterfaceDeclaration, Module, Parameter, TypeField, TypeParameter,
};
use crate::syntax::{Token, TokenKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub use crate::parser::Type;

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
}

#[derive(Debug, Clone)]
struct FunctionSignature {
    parameters: Vec<Parameter>,
    type_parameters: Vec<TypeParameter>,
    return_type: Type,
}

struct TypeExpansionBudget {
    remaining: usize,
    exhausted: bool,
}

impl TypeExpansionBudget {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            exhausted: false,
        }
    }

    fn consume(&mut self) -> bool {
        if let Some(remaining) = self.remaining.checked_sub(1) {
            self.remaining = remaining;
            true
        } else {
            self.exhausted = true;
            false
        }
    }
}

/// Rechecks the requested modules while retaining checker output for modules
/// that the incremental project graph proved unaffected. The caller must only
/// supply a previous project checked under the same compiler policy and must
/// include every reverse dependency of a changed module in `rechecked`.
pub(crate) fn check_incremental(
    project: &Project,
    enforce_types: bool,
    previous: Option<&CheckedProject>,
    rechecked: &BTreeSet<String>,
    max_type_expansions: usize,
) -> (CheckedProject, Vec<Diagnostic>) {
    let mut diagnostics = project::declaration_module_diagnostics(project);
    let exported_types = project::exported_types(project);
    let mut checked_modules = BTreeMap::new();

    for (module_id, module) in &project.modules {
        if !rechecked.contains(module_id) {
            if let Some(previous) = previous.and_then(|previous| previous.modules.get(module_id)) {
                checked_modules.insert(module_id.clone(), previous.clone());
                continue;
            }
        }
        let mut checker = module::ModuleChecker::new(
            project,
            module,
            &exported_types,
            enforce_types,
            max_type_expansions,
        );
        checker.bind();
        if enforce_types {
            checker.check_types();
        }
        diagnostics.extend(checker.diagnostics);
        checked_modules.insert(
            module_id.clone(),
            CheckedModule {
                module: module.clone(),
                symbols: checker.symbols,
            },
        );
    }
    (
        CheckedProject {
            modules: checked_modules,
        },
        diagnostics,
    )
}

mod module;
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

fn infer_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
) -> BTreeMap<String, Type> {
    let mut substitutions = BTreeMap::new();
    let type_parameters = signature
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    for (index, actual) in actuals.iter().enumerate() {
        let Some(parameter) = function_parameter_for_argument(signature, index) else {
            break;
        };
        let annotation = if parameter.rest {
            rest_parameter_element_annotation(parameter)
        } else {
            parameter.annotation.as_ref()
        };
        let Some(annotation) = annotation else {
            continue;
        };
        infer_type_arguments(annotation, actual, &type_parameters, &mut substitutions);
    }
    for parameter in &signature.type_parameters {
        let default = parameter
            .default
            .as_ref()
            .map(|value| substitute_type(value, &substitutions))
            .unwrap_or(Type::Unknown);
        substitutions
            .entry(parameter.name.clone())
            .or_insert(default);
    }
    substitutions
}

fn function_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
) -> Option<BTreeMap<String, Type>> {
    match explicit_type_arguments {
        Some(arguments) => complete_type_arguments(&signature.type_parameters, arguments)
            .map(|arguments| type_parameter_substitutions(&signature.type_parameters, arguments)),
        None => Some(infer_call_substitutions(signature, actuals)),
    }
}

fn function_signature_matches(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    if !function_signature_accepts_argument_count(signature, actuals.len()) {
        return Ok(false);
    }
    let Some(substitutions) =
        function_call_substitutions(signature, actuals, explicit_type_arguments)
    else {
        return Ok(false);
    };
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
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
    let close = tokens.iter().position(|token| token.is(")"))?;
    if close + 1 != tokens.len() {
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

fn infer_type_arguments(
    template: &Type,
    actual: &Type,
    type_parameters: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Type>,
) {
    match (template, actual) {
        (Type::Named { name, arguments }, actual)
            if arguments.is_empty() && type_parameters.contains(name) =>
        {
            if let Some(previous) = substitutions.get(name) {
                if previous != actual {
                    substitutions.insert(name.clone(), Type::Unknown);
                }
            } else {
                substitutions.insert(name.clone(), actual.clone());
            }
        }
        (Type::Array(template), Type::Array(actual)) => {
            infer_type_arguments(template, actual, type_parameters, substitutions);
        }
        (Type::Tuple(templates), Type::Tuple(actuals)) if templates.len() == actuals.len() => {
            for (template, actual) in templates.iter().zip(actuals) {
                infer_type_arguments(template, actual, type_parameters, substitutions);
            }
        }
        (Type::Record(templates), Type::Record(actuals)) => {
            for template in templates {
                if let Some(actual) = actuals.iter().find(|actual| actual.name == template.name) {
                    infer_type_arguments(
                        &template.value,
                        &actual.value,
                        type_parameters,
                        substitutions,
                    );
                }
            }
        }
        _ => {}
    }
}

enum PropertyType {
    Found(Type),
    Missing,
    /// The initial checker has no property semantics for this expression, so
    /// retain its conservative `unknown` behavior rather than rejecting a
    /// potentially valid JavaScript property access.
    Indeterminate,
    Exhausted,
}

fn property_type(
    value: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> PropertyType {
    match value {
        Type::Record(fields) => fields
            .iter()
            .find(|field| field.name == property)
            .map(|field| {
                if field.optional {
                    PropertyType::Found(Type::Union(vec![field.value.clone(), Type::Undefined]))
                } else {
                    PropertyType::Found(field.value.clone())
                }
            })
            .unwrap_or(PropertyType::Missing),
        Type::Named { .. } => {
            match instantiate_named(value, aliases, visited, budget, "property") {
                Some(value) => property_type(&value, property, aliases, visited, budget),
                None if budget.exhausted => PropertyType::Exhausted,
                None => PropertyType::Indeterminate,
            }
        }
        Type::Intersection(parts) => {
            let mut indeterminate = false;
            for part in parts {
                match property_type(part, property, aliases, visited, budget) {
                    PropertyType::Found(value) => return PropertyType::Found(value),
                    PropertyType::Missing => {}
                    PropertyType::Indeterminate => indeterminate = true,
                    PropertyType::Exhausted => return PropertyType::Exhausted,
                }
            }
            if indeterminate {
                PropertyType::Indeterminate
            } else {
                PropertyType::Missing
            }
        }
        Type::Any | Type::Unknown => PropertyType::Indeterminate,
        _ => PropertyType::Missing,
    }
}

fn infer_array(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let mut values = Vec::new();
    let mut start = 1usize;
    let mut depth = 0usize;
    for index in 1..tokens.len() {
        match tokens[index].text.as_str() {
            "[" | "(" | "{" => depth += 1,
            "]" | ")" | "}" if depth > 0 => depth -= 1,
            "," if depth == 0 => {
                if start < index {
                    values.push(infer_array_element(&tokens[start..index], scope));
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start + 1 < tokens.len() {
        values.push(infer_array_element(&tokens[start..tokens.len() - 1], scope));
    }
    let Some(first) = values.first().cloned() else {
        return Type::Array(Box::new(Type::Unknown));
    };
    if values.iter().all(|value| value == &first) {
        Type::Array(Box::new(first))
    } else {
        Type::Array(Box::new(Type::Union(values)))
    }
}

/// Infers an array literal against an explicit tuple annotation without
/// changing ordinary array-literal inference. A tuple spread is expanded only
/// when its source is itself known to be a tuple, preserving fixed arity.
fn infer_contextual_tuple_literal(
    tokens: &[Token],
    scope: &BTreeMap<String, Type>,
) -> Option<Type> {
    if tokens.first().is_none_or(|token| !token.is("["))
        || tokens.last().is_none_or(|token| !token.is("]"))
    {
        return None;
    }
    let mut values = Vec::new();
    let mut start = 1usize;
    let mut depth = 0usize;
    for index in 1..tokens.len() {
        match tokens[index].text.as_str() {
            "[" | "(" | "{" => depth += 1,
            "]" | ")" | "}" if depth > 0 => depth -= 1,
            "," if depth == 0 => {
                push_contextual_tuple_element(&tokens[start..index], scope, &mut values)?;
                start = index + 1;
            }
            _ => {}
        }
    }
    if start + 1 < tokens.len() {
        push_contextual_tuple_element(&tokens[start..tokens.len() - 1], scope, &mut values)?;
    }
    Some(Type::Tuple(values))
}

fn push_contextual_tuple_element(
    tokens: &[Token],
    scope: &BTreeMap<String, Type>,
    values: &mut Vec<Type>,
) -> Option<()> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.first().is_some_and(|token| token.is("...")) {
        let Type::Tuple(spread) = infer_simple(&tokens[1..], scope) else {
            return None;
        };
        values.extend(spread);
    } else {
        values.push(infer_array_element(tokens, scope));
    }
    Some(())
}

fn infer_array_element(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    if tokens.first().is_some_and(|token| token.is("...")) {
        return match infer_simple(&tokens[1..], scope) {
            Type::Array(element) => *element,
            _ => Type::Unknown,
        };
    }
    infer_simple(tokens, scope)
}

fn infer_record(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let mut fields = Vec::new();
    let mut index = 1usize;
    while index < tokens.len() && !tokens[index].is("}") {
        if tokens[index].is("...") {
            let Some(source) = tokens.get(index + 1) else {
                return Type::Unknown;
            };
            let Some(separator) = tokens.get(index + 2) else {
                return Type::Unknown;
            };
            if source.kind != TokenKind::Identifier || !matches!(separator.text.as_str(), "," | "}")
            {
                return Type::Unknown;
            }
            let Type::Record(spread) = scope.get(&source.text).cloned().unwrap_or(Type::Unknown)
            else {
                return Type::Unknown;
            };
            for field in spread {
                insert_inferred_record_field(&mut fields, field);
            }
            index += if separator.is(",") { 3 } else { 2 };
            continue;
        }
        let name = tokens[index].text.clone();
        let (value, value_end) = if tokens.get(index + 1).is_some_and(|token| token.is(":")) {
            let value_start = index + 2;
            let mut value_end = value_start;
            while value_end < tokens.len()
                && !tokens[value_end].is(",")
                && !tokens[value_end].is("}")
            {
                value_end += 1;
            }
            (
                infer_simple(&tokens[value_start..value_end], scope),
                value_end,
            )
        } else if tokens
            .get(index + 1)
            .is_some_and(|token| token.is(",") || token.is("}"))
        {
            (
                scope.get(&name).cloned().unwrap_or(Type::Unknown),
                index + 1,
            )
        } else {
            return Type::Unknown;
        };
        insert_inferred_record_field(
            &mut fields,
            TypeField {
                name,
                optional: false,
                value,
                span: SourceSpan::new(
                    "<inferred>",
                    tokens[index].start,
                    tokens[value_end.saturating_sub(1)].end,
                ),
            },
        );
        index = value_end.saturating_add(1);
    }
    Type::Record(fields)
}

fn insert_inferred_record_field(fields: &mut Vec<TypeField>, field: TypeField) {
    if let Some(existing) = fields
        .iter_mut()
        .find(|existing| existing.name == field.name)
    {
        *existing = field;
    } else {
        fields.push(field);
    }
}

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

fn infer_boolean_logical_expression(left: Type, right: Type) -> Type {
    if left == Type::Boolean && right == Type::Boolean {
        Type::Boolean
    } else {
        Type::Unknown
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

fn is_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if actual == expected {
        return true;
    }
    if let Some(expanded) = instantiate_named(actual, aliases, visited, budget, "actual") {
        return is_assignable(&expanded, expected, aliases, visited, budget);
    }
    if let Some(expanded) = instantiate_named(expected, aliases, visited, budget, "expected") {
        return is_assignable(actual, &expanded, aliases, visited, budget);
    }
    if let Type::Union(options) = actual {
        return options
            .iter()
            .all(|option| is_assignable(option, expected, aliases, &mut visited.clone(), budget));
    }
    if let Type::Union(options) = expected {
        return options
            .iter()
            .any(|option| is_assignable(actual, option, aliases, &mut visited.clone(), budget));
    }
    if let Type::Intersection(parts) = expected {
        return parts
            .iter()
            .all(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget));
    }
    if let (Type::Intersection(_), Type::Record(expected_fields)) = (actual, expected) {
        return expected_fields.iter().all(|expected_field| {
            match property_type(actual, &expected_field.name, aliases, visited, budget) {
                PropertyType::Found(actual) => is_assignable(
                    &actual,
                    &expected_field.value,
                    aliases,
                    &mut visited.clone(),
                    budget,
                ),
                PropertyType::Missing => expected_field.optional,
                PropertyType::Indeterminate => true,
                PropertyType::Exhausted => false,
            }
        });
    }
    if let Type::Intersection(parts) = actual {
        return parts
            .iter()
            .any(|part| is_assignable(part, expected, aliases, &mut visited.clone(), budget));
    }
    match (actual, expected) {
        (Type::Literal(value), Type::String) => value.starts_with('\'') || value.starts_with('\"'),
        (Type::Literal(value), Type::Number) => value.parse::<f64>().is_ok(),
        (Type::Literal(value), Type::Boolean) => matches!(value.as_str(), "true" | "false"),
        (Type::Array(actual), Type::Array(expected)) => {
            is_assignable(actual, expected, aliases, visited, budget)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => {
            actual.iter().zip(expected).all(|(actual, expected)| {
                is_assignable(actual, expected, aliases, &mut visited.clone(), budget)
            })
        }
        (Type::Record(actual), Type::Record(expected)) => expected.iter().all(|expected_field| {
            actual
                .iter()
                .find(|actual_field| actual_field.name == expected_field.name)
                .map(|actual_field| {
                    (actual_field.optional == expected_field.optional || expected_field.optional)
                        && is_assignable(
                            &actual_field.value,
                            &expected_field.value,
                            aliases,
                            &mut visited.clone(),
                            budget,
                        )
                })
                .unwrap_or(expected_field.optional)
        }),
        _ => actual == expected,
    }
}

fn instantiate_named(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    side: &str,
) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    let definition = aliases.get(name)?;
    let arguments = complete_type_arguments(&definition.parameters, arguments)?;
    let key = format!("{side}:{}", type_identity(value));
    if !visited.insert(key) {
        return None;
    }
    if !budget.consume() {
        return None;
    }
    let substitutions = definition
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect();
    Some(substitute_type(&definition.value, &substitutions))
}

/// Resolves omitted trailing generic arguments through their declaration-site
/// defaults. The parser/checker reports invalid argument counts and constraint
/// violations separately; this helper is also used by structural expansion,
/// where `None` simply means the named type cannot be expanded safely.
fn complete_type_arguments(parameters: &[TypeParameter], supplied: &[Type]) -> Option<Vec<Type>> {
    if supplied.len() > parameters.len() {
        return None;
    }
    let mut substitutions = BTreeMap::new();
    let mut arguments = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        let value = supplied.get(index).cloned().or_else(|| {
            parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &substitutions))
        })?;
        substitutions.insert(parameter.name.clone(), value.clone());
        arguments.push(value);
    }
    Some(arguments)
}

fn substitute_type(value: &Type, substitutions: &BTreeMap<String, Type>) -> Type {
    match value {
        Type::Named { name, arguments } if arguments.is_empty() => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| value.clone()),
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_type(argument, substitutions))
                .collect(),
        },
        Type::Array(value) => Type::Array(Box::new(substitute_type(value, substitutions))),
        Type::Tuple(values) => Type::Tuple(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| TypeField {
                    name: field.name.clone(),
                    optional: field.optional,
                    value: substitute_type(&field.value, substitutions),
                    span: field.span.clone(),
                })
                .collect(),
        ),
        Type::Union(values) => Type::Union(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Intersection(values) => Type::Intersection(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn type_identity(value: &Type) -> String {
    match value {
        Type::Named { name, arguments } => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(value) => format!("{}[]", type_identity(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("|"),
        Type::Intersection(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("&"),
        _ => type_label(value),
    }
}

pub(crate) fn type_label(value: &Type) -> String {
    match value {
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Undefined => "undefined".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Number => "number".to_string(),
        Type::String => "string".to_string(),
        Type::Literal(value) => value.clone(),
        Type::Named { name, .. } => name.clone(),
        Type::Array(value) => format!("{}[]", type_label(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values.iter().map(type_label).collect::<Vec<_>>().join(", ")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Intersection(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" & "),
    }
}

#[cfg(test)]
mod tests;
