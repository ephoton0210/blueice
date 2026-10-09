// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reuse BlueJS's private default-module binding and named evaluation rules.

use super::*;

// This is the binding emitted by the workspace BlueJS module parser. It cannot
// collide with a source identifier and preserves default-name inference and
// function initialization during module instantiation.
pub(super) const DEFAULT_BINDING: &str = "\0bluejs_module_default";

pub(super) fn anonymous_function(
    module: &Module,
    function: &FunctionDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    Ok(bluejs::Stmt::ModuleDefaultFunction {
        function: lower_function_value(
            module,
            None,
            &function.parameters,
            &function.body,
            function.async_function,
            function.generator,
        )?,
        binding: DEFAULT_BINDING.to_string(),
    })
}

pub(super) fn anonymous_class(
    module: &Module,
    class: &blueice_bluets::ClassDeclaration,
    define_class_fields: bool,
) -> Result<bluejs::Stmt, BridgeError> {
    let bluejs::Stmt::ClassDecl(mut class) = lower_class(module, class, define_class_fields)?
    else {
        unreachable!("structured class lowering produces a class declaration")
    };
    class.name = None;
    Ok(bluejs::Stmt::VarDecl(
        bluejs::DeclKind::Const,
        vec![bluejs::VarDeclarator {
            pattern: bluejs::Pattern::Identifier(DEFAULT_BINDING.to_string()),
            init: Some(bluejs::Expr::Class(class)),
        }],
    ))
}

pub(super) fn variable(
    module: &Module,
    variable: &VariableDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    let mut variable = variable.clone();
    if module.declarations.iter().any(|declaration| {
        matches!(declaration,
        Declaration::DefaultExport(export) if export.expression && export.name == variable.name)
    }) {
        variable.name = DEFAULT_BINDING.to_string();
    }
    lower_variable(module, &variable)
}
