// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordered module lowering and shared generated-import name ownership.

use super::{
    class_lowering, classes, commonjs, decorators, enums, jsx, legacy_decorators, namespaces,
    output_options,
};
use crate::compiler::{CompilerOptions, Project};
use crate::diagnostic::Diagnostic;
use crate::parser::{Module, TextEdit};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn lower(
    module: &Module,
    project: &Project,
    exported_enums: &BTreeMap<String, BTreeMap<String, crate::checker::ExportedEnum>>,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    wrapper: crate::ModuleKind,
) -> Result<(), Diagnostic> {
    let signatures = classes::overload_signature_erasures(module);
    // Whole-member erasure owns its modifier/annotation edits as well.
    edits.retain(|edit| {
        !signatures
            .iter()
            .any(|signature| edit.start >= signature.start && edit.end <= signature.end)
    });
    edits.extend(signatures);
    output_options::erase_type_comments(module, edits);
    output_options::erase_opening_comments(module, edits);
    // CommonJS name rewriting goes first so that the lowerings below, which copy
    // or move source text (a static initializer, a decorator expression), take
    // the rewritten text with them.
    let mut generated_module_names = BTreeSet::new();
    let references = if options.module_kind == crate::compiler::ModuleKind::CommonJs {
        commonjs::lower_commonjs(
            module,
            project,
            options,
            edits,
            wrapper,
            &mut generated_module_names,
        )?
    } else {
        BTreeMap::new()
    };
    legacy_decorators::prepare_super(module, options, edits)?;
    class_lowering::lower_class_members(module, options, edits, wrapper)?;
    enums::lower_enums(module, project, exported_enums, options, edits)?;
    namespaces::lower_namespaces(module, project, options, edits)?;
    if options.experimental_decorators {
        legacy_decorators::lower_legacy_decorators(
            module,
            project,
            options,
            &references,
            wrapper == crate::ModuleKind::System,
            edits,
        )?;
    } else {
        decorators::lower_decorators(module, options, edits, wrapper)?;
    }
    jsx::lower_jsx(module, options, &references, &generated_module_names, edits)?;
    Ok(())
}
