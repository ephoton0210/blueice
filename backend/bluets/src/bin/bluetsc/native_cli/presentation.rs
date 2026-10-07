// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Human source context uses bytes retained by this authorized invocation.
use super::*;
use blueice_bluets::{TypeScriptDiagnostic, TypeScriptPosition};

pub(super) fn format(
    diagnostics: &[Diagnostic],
    sources: &BTreeMap<String, String>,
    pretty: bool,
) -> String {
    let mut result = String::new();
    for diagnostic in diagnostics {
        let Some(counterpart) = &diagnostic.typescript else {
            result.push_str(&format!(
                "{}:{}:{}: {}: {}\n",
                diagnostic.span.module,
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.code,
                diagnostic.message
            ));
            continue;
        };
        let message = &counterpart.message;
        let file = &counterpart.span.module;
        if pretty {
            if let Some(position) = &counterpart.position {
                result.push_str(&format!(
                    "{} - \x1b[91merror\x1b[0m\x1b[90m TS{}: \x1b[0m{}\n\n",
                    location(file, position),
                    counterpart.code,
                    message
                ));
                if let Some(source) = sources.get(file) {
                    result.push_str(&context(source, position, "", 91));
                }
                result.push('\n');
                for related in &counterpart.related_information {
                    if let Some(position) = &related.position {
                        result
                            .push_str(&format!("  {}\n", location(&related.span.module, position)));
                        if let Some(source) = sources.get(&related.span.module) {
                            result.push_str(&context(source, position, "    ", 96));
                        } else if let Some(line) = TypeScriptDiagnostic::pinned_library_source_line(
                            &related.span.module,
                            position.line,
                        ) {
                            result.push_str(&context_line(
                                line,
                                position.line,
                                position.column.saturating_sub(1),
                                position.length,
                                position.line.to_string().len(),
                                "    ",
                                96,
                            ));
                        }
                        result.push_str(&format!("    {}\n\n", related.message));
                    }
                }
            } else {
                result.push_str(&format!(
                    "\x1b[91merror\x1b[0m\x1b[90m TS{}: \x1b[0m{message}\n",
                    counterpart.code
                ));
            }
        } else if let Some(position) = &counterpart.position {
            result.push_str(&format!(
                "{file}({},{}): error TS{}: {message}\n",
                position.line, position.column, counterpart.code
            ));
        } else {
            result.push_str(&format!("error TS{}: {message}\n", counterpart.code));
        }
    }
    if pretty && !diagnostics.is_empty() {
        let located = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic.typescript.as_ref().and_then(|ts| {
                    ts.position
                        .as_ref()
                        .map(|position| (ts.span.module.as_str(), position.line))
                })
            })
            .collect::<Vec<_>>();
        let count = diagnostics.len();
        if let Some(&(file, line)) = located.first() {
            if located.iter().all(|(name, _)| *name == file) {
                let phrase = if count == 1 {
                    format!("Found 1 error in {file}")
                } else {
                    format!("Found {count} errors in the same file, starting at: {file}")
                };
                result.push_str(&format!("\n{phrase}\x1b[90m:{line}\x1b[0m\n\n"));
            } else {
                result.push_str(&format!(
                    "\nFound {count} errors in {} files.\n\n",
                    located
                        .iter()
                        .map(|(file, _)| file)
                        .collect::<BTreeSet<_>>()
                        .len()
                ));
                let mut files = BTreeMap::<&str, (usize, usize)>::new();
                for (file, line) in located {
                    let entry = files.entry(file).or_insert((0, line));
                    entry.0 += 1;
                    entry.1 = entry.1.min(line);
                }
                result.push_str("Errors  Files\n");
                for (file, (count, line)) in files {
                    result.push_str(&format!("{count:6}  {file}\x1b[90m:{line}\x1b[0m\n"));
                }
            }
        } else {
            result.push_str(&format!(
                "\nFound {count} error{}.\n\n",
                if count == 1 { "" } else { "s" }
            ));
        }
    }
    result
}

fn location(file: &str, position: &TypeScriptPosition) -> String {
    format!(
        "\x1b[96m{file}\x1b[0m:\x1b[93m{}\x1b[0m:\x1b[93m{}\x1b[0m",
        position.line, position.column
    )
}
fn context(source: &str, position: &TypeScriptPosition, indent: &str, color: u8) -> String {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        let newline = match ch {
            '\r' if chars.peek().is_some_and(|(_, ch)| *ch == '\n') => {
                chars.next();
                2
            }
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => 1,
            _ => continue,
        };
        lines.push((&source[start..offset], newline));
        start = offset + ch.len_utf8() + usize::from(newline == 2);
    }
    lines.push((&source[start..], 0));
    let first = position.line.saturating_sub(1);
    if first >= lines.len() {
        return String::new();
    }
    let column = position.column.saturating_sub(1);
    let mut last = first;
    let mut end_column = column;
    let mut remaining = position.length;
    loop {
        let (line, newline) = lines[last];
        let available = line.encode_utf16().count().saturating_sub(end_column);
        if remaining <= available {
            end_column += remaining;
            break;
        }
        end_column += available;
        remaining -= available;
        if remaining <= newline || last + 1 == lines.len() {
            break;
        }
        remaining -= newline;
        last += 1;
        end_column = 0;
    }
    let elide = last - first > 3;
    let width = (last + 1).to_string().len().max(if elide { 3 } else { 1 });
    let mut result = String::new();
    for (index, (line, _)) in lines.iter().enumerate().take(last + 1).skip(first) {
        if elide && index > first + 1 && index < last - 1 {
            if index == first + 2 {
                result.push_str(&format!("{indent}\x1b[7m...\x1b[0m \n"));
            }
            continue;
        }
        let start = if index == first { column } else { 0 };
        let end = if index == last {
            end_column
        } else {
            line.encode_utf16().count()
        };
        result.push_str(&context_line(
            line,
            index + 1,
            start,
            end.saturating_sub(start),
            width,
            indent,
            color,
        ));
    }
    result
}
fn context_line(
    line: &str,
    number: usize,
    column: usize,
    length: usize,
    width: usize,
    indent: &str,
    color: u8,
) -> String {
    let text = line.replace('\t', " ");
    format!(
        "{indent}\x1b[7m{number:width$}\x1b[0m {text}\n{indent}\x1b[7m{}\x1b[0m \x1b[{color}m{}{}\x1b[0m\n",
        " ".repeat(width), " ".repeat(column), "~".repeat(length)
    )
}
