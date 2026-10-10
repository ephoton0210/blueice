// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reparse owned decorator output before applying existing class and ES5 transforms.

use super::super::{class_lowering, targets, EmittedJavaScript, Module};
use crate::compiler::{CompilerOptions, Project};
use crate::diagnostic::Diagnostic;

pub(in crate::emitter) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    project: &Project,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.experimental_decorators
        || super::preserves_proposals(options)
        || (options.target >= crate::EcmaTarget::Es2022 && options.defines_class_fields())
        || !super::has_standard_lowering(module)
    {
        return Ok(emitted);
    }
    let owned = crate::parser::parse_emitted_module(
        &module.id,
        &emitted.javascript,
        &options.limits.parser,
    )
    .map_err(|mut errors| errors.remove(0))?;
    let mut edits = owned.edits.clone();
    class_lowering::lower_generated_class_members(&owned, options, &mut edits)?;
    emitted = targets::mapped_edits(emitted, edits);
    if options.target == crate::EcmaTarget::Es5 {
        let owned = crate::parser::parse_emitted_module(
            &module.id,
            &emitted.javascript,
            &options.limits.parser,
        )
        .map_err(|mut errors| errors.remove(0))?;
        let mut owned_project = project.clone();
        owned_project
            .modules
            .insert(module.id.clone(), owned.clone());
        let mut edits = owned.edits.clone();
        let lexical = targets::es5::prepare(&owned, &owned_project, options, &mut edits)?;
        emitted = targets::mapped_edits(emitted, edits);
        emitted = targets::es5::lower(emitted, &owned, options, &lexical)?;
    }
    Ok(emitted)
}
