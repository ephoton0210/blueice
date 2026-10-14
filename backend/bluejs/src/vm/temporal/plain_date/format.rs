// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ISODateToString`'s date-only portion and `FormatCalendarAnnotation`, with
//! the `calendarName` option's `ShowCalendar` mode.

use super::super::epoch::CivilDate;

/// `ISODateToString`'s date-only portion, including `ToTemporalYearMonth`'s
/// six-digit signed extended-year form for a year outside `0..=9999`.
pub(crate) fn format_iso_date(date: CivilDate) -> String {
    let (year, month, day) = date;
    let year_text = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { "-" } else { "+" }, year.abs())
    };
    format!("{year_text}-{month:02}-{day:02}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShowCalendar {
    Auto,
    Always,
    Never,
    Critical,
}

pub(crate) fn parse_show_calendar(value: &str) -> Option<ShowCalendar> {
    Some(match value {
        "auto" => ShowCalendar::Auto,
        "always" => ShowCalendar::Always,
        "never" => ShowCalendar::Never,
        "critical" => ShowCalendar::Critical,
        _ => return None,
    })
}

/// `FormatCalendarAnnotation`: omits an `iso8601` calendar unless the option
/// forces it, and prefixes a critical `!` when requested.
pub(crate) fn format_calendar_annotation(calendar: &str, show: ShowCalendar) -> String {
    match show {
        ShowCalendar::Never => String::new(),
        ShowCalendar::Auto if calendar == "iso8601" => String::new(),
        ShowCalendar::Critical => format!("[!u-ca={calendar}]"),
        ShowCalendar::Auto | ShowCalendar::Always => format!("[u-ca={calendar}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_show_calendar, ShowCalendar};

    #[test]
    fn calendar_option_accepts_only_the_defined_modes() {
        assert_eq!(parse_show_calendar("auto"), Some(ShowCalendar::Auto));
        assert_eq!(parse_show_calendar("always"), Some(ShowCalendar::Always));
        assert_eq!(parse_show_calendar("never"), Some(ShowCalendar::Never));
        assert_eq!(
            parse_show_calendar("critical"),
            Some(ShowCalendar::Critical)
        );
        assert_eq!(parse_show_calendar("unknown"), None);
    }
}
