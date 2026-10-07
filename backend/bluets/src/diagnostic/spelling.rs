// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded suggestions from already-authorized diagnostic names.

pub(crate) fn suggestion<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let source = name.to_ascii_lowercase();
    let mut matches = candidates
        .map(|candidate| {
            let mut previous = (0..=source.len()).collect::<Vec<_>>();
            for (row, character) in candidate.to_ascii_lowercase().bytes().enumerate() {
                let mut current = vec![row + 1];
                for (column, expected) in source.bytes().enumerate() {
                    current.push(
                        (previous[column + 1] + 1)
                            .min(current[column] + 1)
                            .min(previous[column] + usize::from(character != expected)),
                    );
                }
                previous = current;
            }
            (previous[source.len()], candidate)
        })
        .filter(|(distance, _)| *distance <= 2)
        .collect::<Vec<_>>();
    matches.sort_unstable();
    let (distance, candidate) = *matches.first()?;
    matches
        .get(1)
        .is_none_or(|(next, _)| *next != distance)
        .then_some(candidate)
}

/// Literal alternatives use declaration order to break equally close matches.
/// The bounded UTF-16 edit table avoids locale-dependent case conversions.
pub(crate) fn literal_suggestion<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let source = name.encode_utf16().collect::<Vec<_>>();
    let mut best = source.len() * 2 / 5 + 1;
    let mut result = None;
    for candidate in candidates {
        let target = candidate.encode_utf16().collect::<Vec<_>>();
        if target.len().abs_diff(source.len()) > (source.len() * 34 / 100).max(2)
            || (target.len() < 3 && !candidate.eq_ignore_ascii_case(name))
            || candidate == name
        {
            continue;
        }
        let mut row = (0..=target.len()).collect::<Vec<_>>();
        for (index, character) in source.iter().enumerate() {
            let mut next = vec![index + 1];
            for (column, expected) in target.iter().enumerate() {
                let cost = if character == expected { 0 } else { 2 };
                next.push(
                    (row[column + 1] + 1)
                        .min(next[column] + 1)
                        .min(row[column] + cost),
                );
            }
            row = next;
        }
        let distance = row[target.len()];
        if distance < best {
            best = distance;
            result = Some(candidate);
        }
    }
    result
}
