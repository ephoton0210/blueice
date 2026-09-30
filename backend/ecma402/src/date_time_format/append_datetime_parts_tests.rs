// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{append_datetime_parts, DateTimePart};

fn part(kind: &str, value: &str) -> DateTimePart {
    DateTimePart {
        kind: kind.into(),
        value: value.into(),
    }
}

#[test]
fn substitutes_cldr_fields_and_preserves_typed_boundaries() {
    let parts = append_datetime_parts(
        "{0} ({2}: {1})",
        "hour",
        vec![part("year", "1970")],
        vec![part("hour", "00")],
    )
    .unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| (&part.kind[..], &part.value[..]))
            .collect::<Vec<_>>(),
        vec![
            ("year", "1970"),
            ("literal", " (hour: "),
            ("hour", "00"),
            ("literal", ")"),
        ]
    );
}

#[test]
fn handles_escaped_apostrophes_and_rejects_malformed_patterns() {
    let parts = append_datetime_parts(
        "{0} ''{1}''",
        "unused",
        vec![part("year", "1970")],
        vec![part("hour", "00")],
    )
    .unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|part| part.value.as_str())
            .collect::<String>(),
        "1970 '00'"
    );
    assert!(append_datetime_parts(
        "{0} '{1}",
        "hour",
        vec![part("year", "1970")],
        vec![part("hour", "00")],
    )
    .is_none());
    assert!(append_datetime_parts(
        "{0} only",
        "hour",
        vec![part("year", "1970")],
        vec![part("hour", "00")],
    )
    .is_none());
}
