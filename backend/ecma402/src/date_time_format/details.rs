// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn time_zone_database() -> &'static TimeZoneDatabase {
    static DATABASE: OnceLock<TimeZoneDatabase> = OnceLock::new();
    DATABASE.get_or_init(TimeZoneDatabase::bundled)
}

/// Parses the restricted ISO offset grammar used by `IsTimeZoneOffsetString`.
///
/// Offsets admit only an ASCII sign followed by `HH`, `HHMM`, or `HH:MM`.
/// They are bounded at 23:59, and negative zero is normalized to positive
/// zero for `resolvedOptions().timeZone`.
pub(super) fn parse_time_zone_offset(identifier: &str) -> Option<(String, i32)> {
    let bytes = identifier.as_bytes();
    let (&sign, rest) = bytes.split_first()?;
    let sign = match sign {
        b'+' => 1_i32,
        b'-' => -1_i32,
        _ => return None,
    };
    let (hour, minute) = match rest {
        [hour_tens, hour_ones] => (ascii_decimal(*hour_tens, *hour_ones)?, 0),
        [hour_tens, hour_ones, minute_tens, minute_ones] => (
            ascii_decimal(*hour_tens, *hour_ones)?,
            ascii_decimal(*minute_tens, *minute_ones)?,
        ),
        [hour_tens, hour_ones, b':', minute_tens, minute_ones] => (
            ascii_decimal(*hour_tens, *hour_ones)?,
            ascii_decimal(*minute_tens, *minute_ones)?,
        ),
        _ => return None,
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    let seconds = sign * (i32::from(hour) * 3_600 + i32::from(minute) * 60);
    let normalized_seconds = if seconds == 0 { 0 } else { seconds };
    let normalized_sign = if normalized_seconds < 0 { '-' } else { '+' };
    let absolute = normalized_seconds.unsigned_abs();
    Some((
        format!(
            "{normalized_sign}{:02}:{:02}",
            absolute / 3_600,
            (absolute % 3_600) / 60
        ),
        normalized_seconds,
    ))
}

pub(super) fn ascii_decimal(tens: u8, ones: u8) -> Option<u8> {
    tens.is_ascii_digit()
        .then_some((tens - b'0') * 10)
        .zip(ones.is_ascii_digit().then_some(ones - b'0'))
        .map(|(tens, ones)| tens + ones)
}

pub(super) fn shared_range_part(part: &DateTimePart) -> DateTimeRangePart {
    DateTimeRangePart {
        kind: part.kind.clone(),
        value: part.value.clone(),
        source: DateTimeRangePartSource::Shared,
    }
}

/// Converts ICU4X's interval-side annotations to ECMA-402's serialized-part
/// ownership. ICU marks the side that emitted a segment of its interval
/// pattern, while ECMA-402 marks a field `shared` when one serialized field
/// represents equal start and end values. A field repeated in the output
/// remains endpoint-owned, so a default en-US range keeps both years distinct.
pub(super) fn normalize_range_part_sources(
    parts: &mut [DateTimeRangePart],
    start: &[DateTimePart],
    end: &[DateTimePart],
) {
    for index in 0..parts.len() {
        let part = &parts[index];
        if part.kind == "literal" {
            continue;
        }
        let emitted_once = parts
            .iter()
            .filter(|candidate| candidate.kind == part.kind && candidate.value == part.value)
            .count()
            == 1;
        let matches_both_endpoints = start
            .iter()
            .any(|candidate| candidate.kind == part.kind && candidate.value == part.value)
            && end
                .iter()
                .any(|candidate| candidate.kind == part.kind && candidate.value == part.value);
        if emitted_once && matches_both_endpoints {
            parts[index].source = DateTimeRangePartSource::Shared;
        }
    }

    for index in 0..parts.len() {
        if parts[index].kind != "literal" {
            continue;
        }
        let previous = parts[..index]
            .iter()
            .rev()
            .find(|part| part.kind != "literal")
            .map(|part| part.source);
        let next = parts[index + 1..]
            .iter()
            .find(|part| part.kind != "literal")
            .map(|part| part.source);
        if previous == Some(DateTimeRangePartSource::Shared)
            || next == Some(DateTimeRangePartSource::Shared)
            || (previous == Some(DateTimeRangePartSource::StartRange)
                && next == Some(DateTimeRangePartSource::EndRange))
        {
            parts[index].source = DateTimeRangePartSource::Shared;
        }
    }
}

/// ICU4X currently emits a fractional second as part of the `second` field.
/// ECMA-402 exposes the decimal marker and fractional digits as independent
/// `formatToParts` fields, so split that ICU field before range ownership is
/// assigned or unrequested fields are filtered.
pub(super) fn fractional_second_segments(
    value: &str,
    digits: Option<u8>,
) -> Option<(&str, &str, &str)> {
    let digits = usize::from(digits?);
    let positions = value
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.len() <= digits {
        return None;
    }
    let fractional_start = positions[positions.len() - digits];
    let separator_start = positions[positions.len() - digits - 1];
    let second = &value[..separator_start];
    let separator = &value[separator_start..fractional_start];
    let fractional = &value[fractional_start..];
    (!second.is_empty()
        && !separator.is_empty()
        && separator.chars().all(|character| !character.is_numeric())
        && fractional.chars().all(char::is_numeric))
    .then_some((second, separator, fractional))
}

pub(super) fn split_fractional_seconds(
    parts: Vec<DateTimePart>,
    digits: Option<u8>,
) -> Vec<DateTimePart> {
    let mut result = Vec::with_capacity(parts.len() + usize::from(digits.unwrap_or(0)) * 2);
    for part in parts {
        if part.kind == "second" {
            if let Some((second, separator, fractional)) =
                fractional_second_segments(&part.value, digits)
            {
                result.extend([
                    DateTimePart {
                        kind: "second".into(),
                        value: second.into(),
                    },
                    DateTimePart {
                        kind: "literal".into(),
                        value: separator.into(),
                    },
                    DateTimePart {
                        kind: "fractionalSecond".into(),
                        value: fractional.into(),
                    },
                ]);
                continue;
            }
        }
        result.push(part);
    }
    result
}

pub(super) fn split_range_fractional_seconds(
    parts: Vec<DateTimeRangePart>,
    digits: Option<u8>,
) -> Vec<DateTimeRangePart> {
    let mut result = Vec::with_capacity(parts.len() + usize::from(digits.unwrap_or(0)) * 2);
    for part in parts {
        if part.kind == "second" {
            if let Some((second, separator, fractional)) =
                fractional_second_segments(&part.value, digits)
            {
                result.extend([
                    DateTimeRangePart {
                        kind: "second".into(),
                        value: second.into(),
                        source: part.source,
                    },
                    DateTimeRangePart {
                        kind: "literal".into(),
                        value: separator.into(),
                        source: part.source,
                    },
                    DateTimeRangePart {
                        kind: "fractionalSecond".into(),
                        value: fractional.into(),
                        source: part.source,
                    },
                ]);
                continue;
            }
        }
        result.push(part);
    }
    result
}

#[derive(Default)]
pub(super) struct PartWriter {
    string: String,
    parts: Vec<(usize, usize, Part)>,
}

impl fmt::Write for PartWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.string.write_str(value)
    }
}

impl PartsWrite for PartWriter {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> fmt::Result,
    ) -> fmt::Result {
        let start = self.string.len();
        write(self)?;
        let end = self.string.len();
        if start < end {
            self.parts.push((start, end, part));
        }
        Ok(())
    }
}

impl PartWriter {
    pub(super) fn into_parts(mut self) -> Vec<DateTimePart> {
        self.parts.sort_unstable_by_key(|(start, end, part)| {
            (
                *start,
                *end,
                if part.category == "datetime" { 0 } else { 1 },
            )
        });
        let mut result = Vec::new();
        let mut cursor = 0;
        for (start, end, part) in self.parts {
            if part.category != "datetime" || start < cursor {
                continue;
            }
            if cursor < start {
                result.push(DateTimePart {
                    kind: "literal".into(),
                    value: self.string[cursor..start].into(),
                });
            }
            result.push(DateTimePart {
                kind: part.value.into(),
                value: self.string[start..end].into(),
            });
            cursor = end;
        }
        if cursor < self.string.len() {
            result.push(DateTimePart {
                kind: "literal".into(),
                value: self.string[cursor..].into(),
            });
        }
        result
    }

    pub(super) fn into_range_parts(mut self) -> Vec<DateTimeRangePart> {
        self.parts.sort_unstable_by_key(|(start, end, part)| {
            (
                *start,
                *end,
                if part.category == DATE_RANGE_PART_SOURCE_CATEGORY {
                    0
                } else {
                    1
                },
            )
        });
        let fields = self
            .parts
            .iter()
            .filter(|(_, _, part)| part.category == "datetime")
            .copied()
            .collect::<Vec<_>>();
        let mut sources = self
            .parts
            .iter()
            .filter_map(|(start, end, part)| {
                (part.category == DATE_RANGE_PART_SOURCE_CATEGORY)
                    .then(|| range_part_source(part.value).map(|source| (*start, *end, source)))
                    .flatten()
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|(start, end, _)| (*start, *end));

        let mut result = Vec::new();
        let mut cursor = 0;
        for (start, end, source) in sources {
            if end <= cursor {
                continue;
            }
            if cursor < start {
                push_range_literal(
                    &mut result,
                    &self.string[cursor..start],
                    DateTimeRangePartSource::Shared,
                );
            }
            let mut within_source = start.max(cursor);
            for (field_start, field_end, field) in &fields {
                if *field_start < within_source || *field_end > end {
                    continue;
                }
                push_range_literal(
                    &mut result,
                    &self.string[within_source..*field_start],
                    source,
                );
                result.push(DateTimeRangePart {
                    kind: field.value.into(),
                    value: self.string[*field_start..*field_end].into(),
                    source,
                });
                within_source = *field_end;
            }
            push_range_literal(&mut result, &self.string[within_source..end], source);
            cursor = end;
        }
        push_range_literal(
            &mut result,
            &self.string[cursor..],
            DateTimeRangePartSource::Shared,
        );
        result
    }
}

pub(super) fn range_part_source(value: &str) -> Option<DateTimeRangePartSource> {
    match value {
        "shared" => Some(DateTimeRangePartSource::Shared),
        "startRange" => Some(DateTimeRangePartSource::StartRange),
        "endRange" => Some(DateTimeRangePartSource::EndRange),
        _ => None,
    }
}

pub(super) fn push_range_literal(
    parts: &mut Vec<DateTimeRangePart>,
    value: &str,
    source: DateTimeRangePartSource,
) {
    if !value.is_empty() {
        // Nested ICU annotations can split one contiguous pattern literal.
        // Joining equal-source literal bytes preserves both the text and its
        // authoritative owner; unlike the old post-pass, it never changes a
        // source according to neighboring fields or separator contents.
        if let Some(previous) = parts.last_mut() {
            if previous.kind == "literal" && previous.source == source {
                previous.value.push_str(value);
                return;
            }
        }
        parts.push(DateTimeRangePart {
            kind: "literal".into(),
            value: value.into(),
            source,
        });
    }
}
