// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fail-closed strict artifact and manifest audit before output staging.

use super::*;
use blueice_bluets::{BuildArtifact, EmittedRuntimeSite};

const HELPER_ALIAS: &str = "__bluetsValidateStringV1";

pub(super) fn verify(
    metadata: &BuildMetadata,
    artifacts: &BTreeMap<String, BuildArtifact>,
    declaration_modules: &BTreeMap<String, String>,
) -> io::Result<()> {
    if metadata.runtime_policy != RuntimePolicy::StrictRuntime.as_str() {
        if !metadata.strict_boundaries.is_empty()
            || !metadata.strict_artifacts.is_empty()
            || artifacts
                .values()
                .any(|artifact| artifact.strict_runtime.is_some())
        {
            return Err(invalid("non-strict output carries strict boundary claims"));
        }
        return Ok(());
    }
    if metadata.language_version != blueice_bluets::LANGUAGE_VERSION
        || !matches!(metadata.target, "es2020" | "es2022")
        || metadata.strict_boundaries.is_empty()
        || metadata.strict_artifacts.is_empty()
        || artifacts.is_empty()
        || metadata.entries.len() != 1
        || metadata.declaration_modules != declaration_modules.keys().cloned().collect::<Vec<_>>()
    {
        return Err(invalid(
            "strict output lacks a supported target or complete boundary inventory",
        ));
    }
    if metadata.strict_artifacts != strict_artifact_inventory(artifacts) {
        return Err(invalid(
            "strict artifact bytes differ from manifest digests",
        ));
    }
    let mut remaining = BTreeMap::new();
    for boundary in &metadata.strict_boundaries {
        if boundary.helper_version != RUNTIME_HELPER_V1_VERSION
            || remaining.insert(&boundary.contract_id, boundary).is_some()
        {
            return Err(invalid(
                "strict manifest has a duplicate or incompatible boundary",
            ));
        }
    }
    let mut output_fingerprint = None;
    let mut entry_found = false;
    for (module_id, artifact) in artifacts {
        if !canonical_module_id(module_id) || artifact.module_id != *module_id {
            return Err(invalid(
                "strict artifact has a noncanonical module identity",
            ));
        }
        let output_id = Path::new(module_id).with_extension("js");
        if metadata.entries[0] == output_path(&output_id) {
            entry_found = true;
        }
        if artifact.source_map.is_some() != metadata.source_map
            || artifact.declaration.is_some() != metadata.declaration
        {
            return Err(invalid(
                "strict artifact does not match requested output modes",
            ));
        }
        if let Some(map) = &artifact.source_map {
            let file_name = output_id.file_name().and_then(|name| name.to_str());
            if file_name != Some(map.file.as_str())
                || map.sources.len() != 1
                || map.sources[0] != *module_id
                || map.sources_content.len() != 1
            {
                return Err(invalid("strict source map does not identify its module"));
            }
        }
        match &output_fingerprint {
            Some(fingerprint) if fingerprint != &artifact.fingerprint => {
                return Err(invalid(
                    "strict artifacts have different project fingerprints",
                ));
            }
            None => output_fingerprint = Some(artifact.fingerprint.clone()),
            _ => {}
        }
        let Some(record) = &artifact.strict_runtime else {
            return Err(invalid("strict artifact has no emitted helper record"));
        };
        if record.helper_version != RUNTIME_HELPER_V1_VERSION
            || record.boundaries.is_empty()
            || record.helper_import.generated_start != 0
            || record.helper_import.source_span != SourceSpan::new(module_id, 0, 0)
        {
            return Err(invalid("strict artifact has an incomplete helper import"));
        }
        let helper_path = if module_id.contains('/') {
            format!(
                "{}{}",
                "../".repeat(module_id.split('/').count() - 1),
                RUNTIME_HELPER_V1_FILE
            )
        } else {
            format!("./{RUNTIME_HELPER_V1_FILE}")
        };
        let expected_import =
            format!("import {{ validateStringV1 as {HELPER_ALIAS} }} from '{helper_path}';\n");
        if record.helper_import.expected_text != expected_import
            || !artifact.javascript.starts_with(&expected_import)
        {
            return Err(invalid(
                "strict helper import differs from the published helper",
            ));
        }
        let mut expected_positions = Vec::new();
        for boundary in &record.boundaries {
            let Some(owner) = remaining.remove(&boundary.contract_id) else {
                return Err(invalid("strict artifact has an unowned boundary"));
            };
            if owner.module != *module_id
                || owner.function != boundary.function
                || owner.source_start != boundary.source_span.start
                || owner.source_end != boundary.source_span.end
                || owner.max_string_bytes != boundary.max_string_bytes
                || boundary.source_span.module != *module_id
                || boundary.ingress.is_empty()
                || boundary.ingress.len() > 16
            {
                return Err(invalid(
                    "strict artifact boundary differs from owner metadata",
                ));
            }
            for site in boundary
                .ingress
                .iter()
                .chain(std::iter::once(&boundary.egress))
            {
                verify_site(
                    artifact,
                    module_id,
                    site,
                    boundary.source_span.start,
                    boundary.source_span.end,
                    boundary.max_string_bytes,
                )?;
                expected_positions.push(site.generated_start);
            }
        }
        let marker = format!("{HELPER_ALIAS}(");
        let actual_positions = artifact
            .javascript
            .match_indices(&marker)
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        if expected_positions != actual_positions {
            return Err(invalid(
                "strict helper calls do not match recorded crossings",
            ));
        }
    }
    if !remaining.is_empty() || !entry_found {
        return Err(invalid(
            "strict manifest has a boundary or entry without output",
        ));
    }
    let fingerprint =
        output_fingerprint.ok_or_else(|| invalid("strict output has no fingerprint"))?;
    if metadata.fingerprint != fingerprint_entries(&[fingerprint]) {
        return Err(invalid(
            "strict manifest fingerprint differs from emitted output",
        ));
    }
    Ok(())
}

fn verify_site(
    artifact: &BuildArtifact,
    module_id: &str,
    site: &EmittedRuntimeSite,
    function_start: usize,
    function_end: usize,
    budget: usize,
) -> io::Result<()> {
    let marker = format!("{HELPER_ALIAS}(");
    if site.source_span.module != module_id
        || site.source_span.start < function_start
        || site.source_span.end > function_end
        || site.source_span.start >= site.source_span.end
        || !site.expected_text.starts_with(&marker)
        || !site.expected_text.ends_with(&format!(", {budget})"))
        || !artifact
            .javascript
            .get(site.generated_start..)
            .is_some_and(|tail| tail.starts_with(&site.expected_text))
    {
        return Err(invalid("strict helper call differs from its recorded site"));
    }
    Ok(())
}

fn canonical_module_id(module_id: &str) -> bool {
    module_id.ends_with(".ts")
        && !module_id.ends_with(".d.ts")
        && module_id.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(':')
                && !part.contains('\\')
        })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
