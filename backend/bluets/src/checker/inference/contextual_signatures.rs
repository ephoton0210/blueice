// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextual parameter shapes are not positional runtime call arguments.

use super::*;

pub(in crate::checker) fn infer_signature_context(
    source: &FunctionSignature,
    target: &FunctionSignature,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> InferenceResult {
    let (parameters, identities) = candidate_parameters(source);
    let mut collector = collect::Collector {
        parameters: &parameters,
        identities: &identities,
        aliases,
        budget,
        candidates: BTreeMap::new(),
        active: HashSet::new(),
        dependencies: BTreeMap::new(),
        evidence: 0,
    };
    // Each target parameter supplies a covariant input to instantiation; the
    // final relation still checks the whole resulting function contravariantly.
    collector.collect_parameters(&source.parameters, &target.parameters, true);
    let candidates = std::mem::take(&mut collector.candidates);
    drop(collector);
    finish_candidates(
        source,
        candidates,
        aliases,
        budget,
        (target.return_type != Type::Void).then_some(&target.return_type),
    )
}
