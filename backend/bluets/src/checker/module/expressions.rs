// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Expression inference and direct-runtime checks for one module.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(super) fn infer_expression(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        // Each chained member call recursively infers its receiver. Keep
        // that recursion under the compiler's existing expansion envelope.
        if tokens.iter().filter(|token| token.is(".")).count() > self.max_type_expansions {
            return Type::Unknown;
        }
        let tokens = strip_outer_parentheses(tokens);
        if tokens.len() > 1 && tokens.last().is_some_and(|token| token.is("!")) {
            let inferred = self.infer_expression(&tokens[..tokens.len() - 1], scope);
            return match inferred {
                Type::Union(options) => {
                    let mut retained = options
                        .into_iter()
                        .filter(|option| !matches!(option, Type::Null | Type::Undefined))
                        .collect::<Vec<_>>();
                    match retained.len() {
                        0 => Type::Never,
                        1 => retained.pop().expect("one retained non-null type"),
                        _ => Type::Union(retained),
                    }
                }
                Type::Null | Type::Undefined => Type::Never,
                value => value,
            };
        }
        if let Some(call) = member_call_parts(tokens) {
            let base = self.infer_expression(call.receiver, scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            if let PropertyType::Found(Type::Function { result, .. }) = property_type(
                &base,
                &call.member.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                return *result;
            }
            return Type::Unknown;
        }
        if let Some(call) =
            direct_call_parts(tokens).filter(|call| split_call_arguments(call.arguments).is_some())
        {
            if let Some(signatures) = self.functions.get(&call.callee.text) {
                let explicit = call.generic.then(|| {
                    self.module
                        .generic_call_type_arguments
                        .get(&call.callee.start)
                        .expect("parsed generic call has recorded type arguments")
                        .as_slice()
                });
                return self.infer_function_call(signatures, call.arguments, scope, explicit);
            }
            return scope
                .get(&call.callee.text)
                .cloned()
                .unwrap_or(Type::Unknown);
        }
        if let Some((_, consequent, alternate)) = conditional_expression_parts(tokens) {
            return merge_conditional_branch_types(
                self.infer_expression(consequent, scope),
                self.infer_expression(alternate, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["??"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_nullish_coalescing_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["||"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["&&"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        for operators in [&["|"][..], &["^"][..], &["&"][..]] {
            if let Some((left, _, right)) = top_level_binary_parts(tokens, operators, |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            }) {
                return infer_numeric_binary_expression(
                    self.infer_expression(left, scope),
                    self.infer_expression(right, scope),
                );
            }
        }
        if top_level_binary_parts(
            tokens,
            &[
                "===",
                "!==",
                "==",
                "!=",
                "<",
                ">",
                "<=",
                ">=",
                "in",
                "instanceof",
            ],
            |start| self.module.generic_call_type_arguments.contains_key(&start),
        )
        .is_some()
        {
            return Type::Boolean;
        }
        if let Some((left, _, right)) = top_level_shift_parts(tokens, |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            })
        {
            return infer_additive_expression(
                operator,
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["*", "/", "%"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["**"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        let Some(first) = tokens.first() else {
            return Type::Undefined;
        };
        if first.is("typeof") {
            return Type::String;
        }
        if first.is("void") {
            return Type::Undefined;
        }
        if matches!(first.text.as_str(), "!") {
            return Type::Boolean;
        }
        if matches!(first.text.as_str(), "+" | "-" | "~") {
            return Type::Number;
        }
        if first.kind == TokenKind::String || first.kind == TokenKind::Template {
            return Type::String;
        }
        if first.kind == TokenKind::Number {
            return Type::Number;
        }
        match first.text.as_str() {
            "true" | "false" => Type::Boolean,
            "null" => Type::Null,
            "undefined" => Type::Undefined,
            "[" => infer_array(tokens, scope),
            "{" => infer_record(tokens, scope),
            _ if first.kind == TokenKind::Identifier => {
                if tokens.len() == 1 {
                    if let Some(signature) =
                        self.functions.get(&first.text).and_then(|set| set.first())
                    {
                        if signature.type_parameters.is_empty() {
                            return Type::Function {
                                parameters: signature.parameters.clone(),
                                result: Box::new(signature.return_type.clone()),
                            };
                        }
                    }
                }
                if tokens.get(1).is_some_and(|token| token.is("."))
                    && tokens
                        .get(2)
                        .is_some_and(|token| token.kind == TokenKind::Identifier)
                {
                    let base = scope.get(&first.text).cloned().unwrap_or(Type::Unknown);
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    return match property_type(
                        &base,
                        &tokens[2].text,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        PropertyType::Found(value) => value,
                        PropertyType::Missing
                        | PropertyType::Indeterminate
                        | PropertyType::Exhausted => Type::Unknown,
                    };
                }
                scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        }
    }

    pub(super) fn infer_function_call(
        &self,
        signatures: &[FunctionSignature],
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        explicit_type_arguments: Option<&[Type]>,
    ) -> Type {
        let Some(arguments) = split_call_arguments(tokens) else {
            return Type::Unknown;
        };
        let Ok(actuals) = self.expanded_call_argument_types(&arguments, scope) else {
            return Type::Unknown;
        };
        let Ok(Some(signature)) =
            self.select_function_signature(signatures, &actuals, explicit_type_arguments)
        else {
            return Type::Unknown;
        };
        let substitutions =
            function_call_substitutions(signature, &actuals, explicit_type_arguments)
                .expect("selected function signature has valid substitutions");
        substitute_type(&signature.return_type, &substitutions)
    }

    pub(super) fn check_function_call(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let Some(call) = direct_call_parts(tokens) else {
            return;
        };
        let Some(signatures) = self.functions.get(&call.callee.text).cloned() else {
            if self.require_declared_global_calls && !scope.contains_key(&call.callee.text) {
                self.type_error(
                    span,
                    format!(
                        "function {} is not declared by this page profile",
                        call.callee.text
                    ),
                    DiagnosticCode::UnknownName,
                );
            }
            return;
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        for argument in &arguments {
            let argument = if argument.first().is_some_and(|token| token.is("...")) {
                &argument[1..]
            } else {
                argument
            };
            self.check_direct_property_access(argument, scope, span);
        }
        let actuals = match self.expanded_call_argument_types(&arguments, scope) {
            Ok(actuals) => actuals,
            Err(()) => {
                self.type_error(
                    span,
                    format!(
                        "a spread argument for function {} must have a fixed-length tuple type",
                        call.callee.text
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
        };
        let explicit = call.generic.then(|| {
            self.module
                .generic_call_type_arguments
                .get(&call.callee.start)
                .expect("parsed generic call has recorded type arguments")
                .as_slice()
        });
        let selected = match self.select_function_signature(&signatures, &actuals, explicit) {
            Ok(selected) => selected.cloned(),
            Err(()) => {
                self.type_error(
                    span,
                    format!(
                        "overload selection exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
        };
        if signatures.len() > 1 && selected.is_none() {
            self.type_error(
                span,
                format!(
                    "no overload of function {} accepts the supplied argument types",
                    call.callee.text
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let Some(signature) = selected.or_else(|| signatures.first().cloned()) else {
            return;
        };
        let required = function_signature_required_arguments(&signature);
        if !function_signature_accepts_argument_count(&signature, actuals.len()) {
            let expected = if signature
                .parameters
                .last()
                .is_some_and(|parameter| parameter.rest)
            {
                format!("at least {required}")
            } else {
                format!("{required} to {}", signature.parameters.len())
            };
            self.type_error(
                span,
                format!(
                    "function {} expects {} argument(s), got {}",
                    call.callee.text,
                    expected,
                    actuals.len()
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let substitutions = if let Some(explicit) = explicit {
            let Some(substitutions) =
                self.check_explicit_function_type_arguments(&signature, explicit, span)
            else {
                return;
            };
            substitutions
        } else {
            let substitutions = infer_call_substitutions(&signature, &actuals);
            self.check_call_type_parameter_constraints(
                &signature,
                &substitutions,
                span,
                "inferred type",
            );
            substitutions
        };
        for (index, actual) in actuals.iter().enumerate() {
            let parameter = function_parameter_for_argument(&signature, index)
                .expect("an accepted function call has a parameter for every argument");
            let expected = call_parameter_expected_type(parameter, &substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                        index + 1,
                        type_label(actual),
                        parameter.name,
                        type_label(&expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn check_member_calls_in_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let mut open = Vec::new();
        let mut closes = vec![None; tokens.len()];
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                open.push(index);
            } else if token.is(")") {
                if let Some(start) = open.pop() {
                    closes[start] = Some(index);
                }
            }
        }
        for start in 0..tokens.len().saturating_sub(3) {
            if tokens[start].kind != TokenKind::Identifier
                || !tokens[start + 1].is(".")
                || tokens[start + 2].kind != TokenKind::Identifier
                || !tokens[start + 3].is("(")
            {
                continue;
            }
            if let Some(end) = closes[start + 3] {
                self.check_member_call(&tokens[start..=end], scope, span);
            }
        }
        // A method on a call result has no identifier immediately before its
        // final dot (for example `document.getElementById('x')!.appendChild(y)`).
        // The direct-call scan above checks the inner call; check the complete
        // chain once for the outer receiver and its arguments.
        if member_call_parts(tokens).is_some_and(|call| call.receiver.len() > 1) {
            self.check_member_call(tokens, scope, span);
        }
    }

    fn check_member_call(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let tokens = if tokens.len() > 1 && tokens.last().is_some_and(|token| token.is("!")) {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        let Some(call) = member_call_parts(tokens) else {
            return;
        };
        if self.require_declared_global_calls
            && call.receiver.first().is_some_and(|base| {
                base.kind == TokenKind::Identifier && !scope.contains_key(&base.text)
            })
        {
            let base = &call.receiver[0];
            self.type_error(
                span,
                format!("object {} is not declared by this page profile", base.text),
                DiagnosticCode::UnknownName,
            );
            return;
        }
        let base = self.infer_expression(call.receiver, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let member_type = property_type(
            &base,
            &call.member.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        let (parameters, _) = match member_type {
            PropertyType::Found(Type::Function { parameters, result }) => (parameters, result),
            PropertyType::Found(_) => {
                self.type_error(
                    span,
                    format!("property `{}` is not callable", call.member.text),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
            PropertyType::Missing
                if matches!(
                    base,
                    Type::String | Type::Number | Type::Boolean | Type::Array(_) | Type::Tuple(_)
                ) =>
            {
                // BlueTS does not yet model the JavaScript built-in method
                // catalogs. Preserve their runtime semantics while still
                // rejecting missing members on declared host interfaces.
                return;
            }
            PropertyType::Missing => {
                self.type_error(
                    span,
                    format!(
                        "property `{}` does not exist on type `{}`",
                        call.member.text,
                        type_label(&base)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                return;
            }
            PropertyType::Exhausted => {
                self.type_error(
                    span,
                    format!(
                        "property lookup exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
            PropertyType::Indeterminate => return,
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        for argument in &arguments {
            self.check_function_call(argument, scope, span);
        }
        let Ok(actuals) = self.expanded_call_argument_types(&arguments, scope) else {
            self.type_error(
                span,
                format!(
                    "a spread argument for method {} must have a fixed-length tuple type",
                    call.member.text
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        let required = parameters
            .iter()
            .filter(|parameter| !parameter.optional)
            .count();
        if actuals.len() < required || actuals.len() > parameters.len() {
            self.type_error(
                span,
                format!(
                    "method {} expects {required} to {} argument(s), got {}",
                    call.member.text,
                    parameters.len(),
                    actuals.len()
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        for (index, actual) in actuals.iter().enumerate() {
            let expected = parameters[index]
                .annotation
                .as_ref()
                .expect("method signature parameters have annotations");
            if !self.is_assignable_bounded(actual, expected, span) {
                self.type_error(
                    span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to method parameter `{}` of type `{}`",
                        index + 1,
                        type_label(actual),
                        parameters[index].name,
                        type_label(expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn expanded_call_argument_types(
        &self,
        arguments: &[&[Token]],
        scope: &BTreeMap<String, Type>,
    ) -> Result<Vec<Type>, ()> {
        let mut actuals = Vec::new();
        for argument in arguments {
            if argument.first().is_some_and(|token| token.is("...")) {
                let Type::Tuple(values) = self.infer_expression(&argument[1..], scope) else {
                    return Err(());
                };
                actuals.extend(values);
            } else {
                actuals.push(match *argument {
                    [literal] if literal.kind == TokenKind::String => {
                        Type::Literal(literal.text.clone())
                    }
                    _ => self.infer_expression(argument, scope),
                });
            }
        }
        Ok(actuals)
    }

    pub(super) fn select_function_signature<'b>(
        &self,
        signatures: &'b [FunctionSignature],
        actuals: &[Type],
        explicit_type_arguments: Option<&[Type]>,
    ) -> Result<Option<&'b FunctionSignature>, ()> {
        for signature in signatures {
            if function_signature_matches(
                signature,
                actuals,
                explicit_type_arguments,
                &self.types,
                self.max_type_expansions,
            )? {
                return Ok(Some(signature));
            }
        }
        Ok(None)
    }

    pub(super) fn is_assignable_bounded(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: &SourceSpan,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let assignable = is_assignable(
            actual,
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            self.type_error(
                span,
                format!(
                    "type comparison exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
            true
        } else {
            assignable
        }
    }

    pub(super) fn check_call_type_parameter_constraints(
        &mut self,
        signature: &FunctionSignature,
        substitutions: &BTreeMap<String, Type>,
        span: &SourceSpan,
        actual_description: &str,
    ) {
        for parameter in &signature.type_parameters {
            let Some(constraint) = &parameter.constraint else {
                continue;
            };
            let actual = substitutions
                .get(&parameter.name)
                .expect("function substitutions contain every type parameter");
            let expected = substitute_type(constraint, substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "{actual_description} `{}` does not satisfy constraint `{}` for `{}`",
                        type_label(actual),
                        type_label(&expected),
                        parameter.name,
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn check_explicit_function_type_arguments(
        &mut self,
        signature: &FunctionSignature,
        arguments: &[Type],
        span: &SourceSpan,
    ) -> Option<BTreeMap<String, Type>> {
        let required = signature
            .type_parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > signature.type_parameters.len() {
            self.type_error(
                span,
                format!(
                    "function type arguments require {required} to {} argument(s), got {}",
                    signature.type_parameters.len(),
                    arguments.len(),
                ),
                DiagnosticCode::TypeMismatch,
            );
            return None;
        }
        for argument in arguments {
            self.check_type(argument, span);
        }
        let completed = complete_type_arguments(&signature.type_parameters, arguments)?;
        let substitutions = type_parameter_substitutions(&signature.type_parameters, completed);
        self.check_call_type_parameter_constraints(
            signature,
            &substitutions,
            span,
            "type argument",
        );
        Some(substitutions)
    }

    pub(super) fn check_direct_property_access(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let [base, dot, property] = tokens else {
            return;
        };
        if base.kind != TokenKind::Identifier
            || !dot.is(".")
            || property.kind != TokenKind::Identifier
        {
            return;
        }
        let value = scope.get(&base.text).cloned().unwrap_or(Type::Unknown);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        match property_type(
            &value,
            &property.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found(_) | PropertyType::Indeterminate => {}
            PropertyType::Missing => self.type_error(
                span,
                format!(
                    "property `{}` does not exist on type `{}`",
                    property.text,
                    type_label(&value)
                ),
                DiagnosticCode::TypeMismatch,
            ),
            PropertyType::Exhausted => self.type_error(
                span,
                format!(
                    "property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }

    pub(super) fn check_member_assignment(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let [base, dot, property, assign, value @ ..] = strip_outer_parentheses(tokens) else {
            return;
        };
        if base.kind != TokenKind::Identifier
            || !dot.is(".")
            || property.kind != TokenKind::Identifier
            || !assign.is("=")
            || value.is_empty()
        {
            return;
        }
        let owner = scope.get(&base.text).cloned().unwrap_or(Type::Unknown);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        match property_type(
            &owner,
            &property.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found(expected) => {
                let actual = self.infer_expression(value, scope);
                if !self.is_assignable_bounded(&actual, &expected, span) {
                    self.type_error(
                        span,
                        format!(
                            "assignment has type `{}`, which is not assignable to property `{}` of type `{}`",
                            type_label(&actual),
                            property.text,
                            type_label(&expected)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            PropertyType::Missing => self.type_error(
                span,
                format!(
                    "property `{}` does not exist on type `{}`",
                    property.text,
                    type_label(&owner)
                ),
                DiagnosticCode::TypeMismatch,
            ),
            PropertyType::Exhausted => self.type_error(
                span,
                format!(
                    "property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
            PropertyType::Indeterminate => {}
        }
    }

    /// Checks only arithmetic forms whose operand types are already known
    /// primitive values. Unknown, `any`, union and structural forms remain
    /// outside this deliberately bounded compatibility rule.
    pub(super) fn check_arithmetic_operators(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let generic_call = |start| self.module.generic_call_type_arguments.contains_key(&start);
        if let Some((condition, consequent, alternate)) = conditional_expression_parts(tokens) {
            self.check_arithmetic_operators(condition, scope, span);
            self.check_arithmetic_operators(consequent, scope, span);
            self.check_arithmetic_operators(alternate, scope, span);
            return;
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["??"], generic_call) {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            return;
        }
        for operators in [&["||"][..], &["&&"][..]] {
            if let Some((left, _, right)) = top_level_binary_parts(tokens, operators, generic_call)
            {
                self.check_arithmetic_operators(left, scope, span);
                self.check_arithmetic_operators(right, scope, span);
                return;
            }
        }
        for operators in [&["|"][..], &["^"][..], &["&"][..]] {
            if let Some((left, operator, right)) =
                top_level_binary_parts(tokens, operators, generic_call)
            {
                self.check_arithmetic_operators(left, scope, span);
                self.check_arithmetic_operators(right, scope, span);
                self.check_known_numeric_operands(
                    &operator.text,
                    &self.infer_expression(left, scope),
                    &self.infer_expression(right, scope),
                    span,
                );
                return;
            }
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["===", "!=="], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_disjoint_strict_equality(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, _, right)) =
            top_level_binary_parts(tokens, &["==", "!=", "<", ">", "<=", ">="], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            return;
        }
        if let Some((left, operator, right)) = top_level_shift_parts(tokens, generic_call) {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_numeric_operands(
                operator.text(),
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["*", "/", "%"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) = top_level_binary_parts(tokens, &["**"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
        }
    }

    pub(super) fn check_known_arithmetic_operands(
        &mut self,
        operator: &Token,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if !is_known_primitive_type(left) || !is_known_primitive_type(right) {
            return;
        }
        let accepted = (operator.is("+") && (left == &Type::String || right == &Type::String))
            || (left == &Type::Number && right == &Type::Number);
        if !accepted {
            self.type_error(
                span,
                format!(
                    "operator `{}` cannot be applied to types `{}` and `{}`",
                    operator.text,
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    pub(super) fn check_known_disjoint_strict_equality(
        &mut self,
        operator: &Token,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if is_strict_equality_primitive_type(left)
            && is_strict_equality_primitive_type(right)
            && left != right
        {
            self.type_error(
                span,
                format!(
                    "operator `{}` compares disjoint types `{}` and `{}`",
                    operator.text,
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    pub(super) fn check_known_numeric_operands(
        &mut self,
        operator: &str,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if !is_known_primitive_type(left) || !is_known_primitive_type(right) {
            return;
        }
        if left != &Type::Number || right != &Type::Number {
            self.type_error(
                span,
                format!(
                    "operator `{operator}` cannot be applied to types `{}` and `{}`",
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}
