// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Whole scalar runs preserve contextual casing; malformed units stay opaque.
use super::CaseMapping;

pub(super) fn map(units: &[u16], operation: CaseMapping) -> Vec<u16> {
    if matches!(
        operation,
        CaseMapping::Capitalize | CaseMapping::Uncapitalize
    ) {
        let Some((&first, tail)) = units.split_first() else {
            return Vec::new();
        };
        let mut result = match char::from_u32(u32::from(first)) {
            Some(value) => converted(&value.to_string(), operation)
                .encode_utf16()
                .collect(),
            None => vec![first],
        };
        result.extend_from_slice(tail);
        return result;
    }
    let mut result = Vec::new();
    let mut run = String::new();
    for value in char::decode_utf16(units.iter().copied()) {
        match value {
            Ok(value) => run.push(value),
            Err(error) => {
                result.extend(converted(&run, operation).encode_utf16());
                run.clear();
                result.push(error.unpaired_surrogate());
            }
        }
    }
    result.extend(converted(&run, operation).encode_utf16());
    result
}

fn converted(value: &str, operation: CaseMapping) -> String {
    match operation {
        CaseMapping::Upper | CaseMapping::Capitalize => value.to_uppercase(),
        CaseMapping::Lower | CaseMapping::Uncapitalize => value.to_lowercase(),
    }
}
