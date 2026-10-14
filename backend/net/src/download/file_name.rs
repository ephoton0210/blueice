// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where a downloaded file's name comes from, and making it safe to use
//! as one. A server-supplied `Content-Disposition` or a URL path is
//! attacker-influenced text (`phase-10-download-manager/PLAN.md`'s
//! "Destination policy"), so nothing from either reaches the file system
//! without going through [`sanitize`].

/// The longest file name, in bytes, that [`sanitize`] will return -- well
/// under the 255 bytes most file systems allow, leaving room for the
/// `.blueice-part.json` suffix the engine appends.
pub const MAX_FILE_NAME_BYTES: usize = 200;

/// The longest extension (including its dot) that survives truncating an
/// over-long name.
const MAX_KEPT_EXTENSION_BYTES: usize = 16;

/// Splits a header value at top-level `;`, ignoring those inside a
/// quoted string.
fn split_params(header: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut in_quotes, mut escaped, mut start) = (false, false, 0);
    for (i, c) in header.char_indices() {
        if escaped {
            escaped = false;
        } else if in_quotes && c == '\\' {
            escaped = true;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ';' && !in_quotes {
            parts.push(&header[start..i]);
            start = i + 1;
        }
    }
    parts.push(&header[start..]);
    parts
}

/// A parameter value: either a quoted string (with `\"` and `\\`
/// unescaped) or a bare token.
fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value.strip_prefix('"') else {
        return value.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '"' => break,
            other => out.push(other),
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

/// RFC 8187 `charset'language'percent-encoded` -- only the two charsets
/// the RFC requires recipients to support.
fn decode_extended(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_uppercase();
    let _language = parts.next()?;
    let bytes = percent_decode(parts.next()?)?;
    match charset.as_str() {
        "UTF-8" => String::from_utf8(bytes).ok(),
        "ISO-8859-1" => Some(bytes.into_iter().map(char::from).collect()),
        _ => None,
    }
}

/// The file name a `Content-Disposition` header asks for. `filename*`
/// (RFC 8187) wins over plain `filename` regardless of order; an
/// extended value this build can't decode falls back to the plain one.
/// An empty name is no name. **Not sanitized** -- see [`sanitize`].
pub fn content_disposition_file_name(header: &str) -> Option<String> {
    let (mut plain, mut extended) = (None, None);
    for param in split_params(header) {
        let Some((name, value)) = param.split_once('=') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "filename*" => extended = decode_extended(&unquote(value)),
            "filename" => plain = Some(unquote(value)),
            _ => {}
        }
    }
    extended
        .filter(|n| !n.is_empty())
        .or(plain.filter(|n| !n.is_empty()))
}

/// The last path segment of `url`, percent-decoded, or `None` when the
/// URL has no file segment (`/`, `/dir/`, a bare host, a query only).
/// **Not sanitized** -- see [`sanitize`].
pub fn file_name_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.next_back()?;
    if segment.is_empty() {
        return None;
    }
    Some(
        percent_decode(segment)
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| segment.to_string()),
    )
}

fn is_reserved_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    match upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    {
        Some(n) => matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"),
        None => false,
    }
}

/// Truncates on a character boundary to at most `max` bytes.
fn truncate_to(s: &str, max: usize) -> &str {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn limit_length(name: String) -> String {
    if name.len() <= MAX_FILE_NAME_BYTES {
        return name;
    }
    let extension_start = name
        .rfind('.')
        .filter(|&i| i > 0 && name.len() - i <= MAX_KEPT_EXTENSION_BYTES);
    match extension_start {
        Some(i) => {
            let (stem, extension) = name.split_at(i);
            let stem = truncate_to(stem, MAX_FILE_NAME_BYTES - extension.len())
                .trim_end_matches(['.', ' ']);
            format!("{stem}{extension}")
        }
        None => truncate_to(&name, MAX_FILE_NAME_BYTES)
            .trim_end_matches(['.', ' '])
            .to_string(),
    }
}

/// Formatting characters that do not occupy a trustworthy visible position
/// in a filename.  In particular, bidi overrides can make an executable
/// suffix render as a document suffix, and zero-width/tag characters can make
/// two names look identical in the downloads page or MCP output.
fn is_invisible_or_bidi(c: char) -> bool {
    matches!(c,
        '\u{061c}'
            | '\u{070f}'
            | '\u{115f}' | '\u{1160}'
            | '\u{17b4}' | '\u{17b5}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{2800}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
            | '\u{ffa0}'
            | '\u{fff0}'..='\u{fff8}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0000}'..='\u{e0fff}'
    )
}

/// Turns untrusted text into a single, safe file-name component: takes
/// the last non-empty path component (either separator), drops control
/// characters, replaces `<>:"|?*` with `_`, trims spaces and dots from
/// both ends (so never a hidden file, `.`, or `..`), prefixes a Windows
/// reserved device name with `_`, bounds the length while keeping a short
/// extension, and falls back to `download` if nothing is left.
pub fn sanitize(name: &str) -> String {
    let last = name
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_or_bidi(*c))
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['.', ' ']);
    if trimmed.is_empty() {
        return "download".to_string();
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    let named = if is_reserved_device_name(stem) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    };
    let limited = limit_length(named);
    let limited = limited.trim_matches(['.', ' ']);
    if limited.is_empty() {
        "download".to_string()
    } else {
        limited.to_string()
    }
}

/// The name to save a download under: the server's `Content-Disposition`
/// if it names one, else the URL's last path segment, else `download` --
/// always [`sanitize`]d.
pub fn choose_file_name(content_disposition: Option<&str>, url: &str) -> String {
    let raw = content_disposition
        .and_then(content_disposition_file_name)
        .or_else(|| file_name_from_url(url));
    sanitize(&raw.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_quoted_and_bare_filenames() {
        assert_eq!(
            content_disposition_file_name(r#"attachment; filename="report.pdf""#).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            content_disposition_file_name("attachment; filename=report.pdf").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            content_disposition_file_name("attachment;filename=report.pdf;size=4").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            content_disposition_file_name(r#"ATTACHMENT; FILENAME="Report.PDF""#).as_deref(),
            Some("Report.PDF")
        );
    }

    #[test]
    fn content_disposition_quoted_values_keep_semicolons_and_escaped_quotes() {
        assert_eq!(
            content_disposition_file_name(r#"attachment; filename="a;b.txt""#).as_deref(),
            Some("a;b.txt")
        );
        assert_eq!(
            content_disposition_file_name(r#"attachment; filename="a\"b.txt""#).as_deref(),
            Some(r#"a"b.txt"#)
        );
    }

    #[test]
    fn content_disposition_extended_filename_is_percent_decoded_and_preferred() {
        assert_eq!(
            content_disposition_file_name("attachment; filename*=UTF-8''na%C3%AFve%20file.txt")
                .as_deref(),
            Some("naïve file.txt")
        );
        assert_eq!(
            content_disposition_file_name(
                r#"attachment; filename="fallback.txt"; filename*=UTF-8''real%20name.txt"#
            )
            .as_deref(),
            Some("real name.txt"),
            "filename* wins over filename regardless of order"
        );
        assert_eq!(
            content_disposition_file_name(
                r#"attachment; filename*=UTF-8''real.txt; filename="fallback.txt""#
            )
            .as_deref(),
            Some("real.txt")
        );
    }

    #[test]
    fn content_disposition_extended_filename_supports_latin1_and_ignores_other_charsets() {
        assert_eq!(
            content_disposition_file_name("attachment; filename*=ISO-8859-1''caf%E9.txt")
                .as_deref(),
            Some("café.txt")
        );
        assert_eq!(
            content_disposition_file_name("attachment; filename*=KOI8-R''%C1.txt"),
            None
        );
        assert_eq!(
            content_disposition_file_name(
                r#"attachment; filename*=KOI8-R''%C1.txt; filename="ok.txt""#
            )
            .as_deref(),
            Some("ok.txt"),
            "an unusable extended value falls back to the plain one"
        );
    }

    #[test]
    fn content_disposition_invalid_percent_encoding_is_ignored() {
        assert_eq!(
            content_disposition_file_name("attachment; filename*=UTF-8''%ZZ.txt"),
            None
        );
        assert_eq!(
            content_disposition_file_name("attachment; filename*=UTF-8''%C3.txt"),
            None,
            "a truncated UTF-8 sequence is not valid UTF-8"
        );
    }

    #[test]
    fn content_disposition_without_a_filename_is_none() {
        assert_eq!(content_disposition_file_name("inline"), None);
        assert_eq!(content_disposition_file_name("attachment"), None);
        assert_eq!(content_disposition_file_name(""), None);
        assert_eq!(
            content_disposition_file_name(r#"attachment; filename="""#),
            None,
            "an empty name is no name"
        );
    }

    #[test]
    fn a_file_name_comes_from_the_last_path_segment_of_the_url() {
        assert_eq!(
            file_name_from_url("https://example.com/a/b/file.tar.gz?x=1#frag").as_deref(),
            Some("file.tar.gz")
        );
        assert_eq!(
            file_name_from_url("http://127.0.0.1:8080/big.iso").as_deref(),
            Some("big.iso")
        );
        assert_eq!(
            file_name_from_url("https://example.com/my%20file.txt").as_deref(),
            Some("my file.txt")
        );
    }

    #[test]
    fn a_url_with_no_file_segment_yields_none() {
        assert_eq!(file_name_from_url("https://example.com/"), None);
        assert_eq!(file_name_from_url("https://example.com"), None);
        assert_eq!(file_name_from_url("https://example.com/dir/"), None);
        assert_eq!(
            file_name_from_url("https://example.com/?file=x.bin"),
            None,
            "a query string is not a path"
        );
        assert_eq!(
            file_name_from_url("https://example.com?path=/a/b"),
            None,
            "a slash in a query is not a path segment"
        );
        assert_eq!(file_name_from_url("not a url"), None);
    }

    #[test]
    fn sanitize_leaves_ordinary_names_alone() {
        for name in [
            "report.pdf",
            "archive.tar.gz",
            "naïve file (1).txt",
            "日本語.txt",
            "no_extension",
        ] {
            assert_eq!(sanitize(name), name);
        }
    }

    #[test]
    fn sanitize_strips_directory_components() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("a/b\\c.txt"), "c.txt");
        assert_eq!(sanitize("/absolute/path.bin"), "path.bin");
        assert_eq!(sanitize("C:\\Windows\\system32\\cmd.exe"), "cmd.exe");
    }

    #[test]
    fn sanitize_replaces_characters_no_file_system_agrees_on_and_drops_control_characters() {
        assert_eq!(sanitize("a:b?.txt"), "a_b_.txt");
        assert_eq!(sanitize("bad<>\"|*name.txt"), "bad_____name.txt");
        assert_eq!(sanitize("a\u{0}b\nc\td.txt"), "abcd.txt");
    }

    #[test]
    fn sanitize_removes_bidi_zero_width_and_unicode_tag_characters() {
        assert_eq!(
            sanitize("invoice\u{202e}fdp.exe"),
            "invoicefdp.exe",
            "a bidi override cannot disguise the extension"
        );
        assert_eq!(
            sanitize("pay\u{200b}load\u{2060}.exe"),
            "payload.exe",
            "zero-width formatting cannot make a second visual name"
        );
        assert_eq!(
            sanitize("safe\u{e0001}\u{e007f}.txt"),
            "safe.txt",
            "Unicode tag characters are invisible too"
        );
    }

    #[test]
    fn truncating_a_name_without_an_extension_cannot_leave_a_windows_unsafe_suffix() {
        let name = format!("{} .", "x".repeat(MAX_FILE_NAME_BYTES));
        let sanitized = sanitize(&name);
        assert!(sanitized.len() <= MAX_FILE_NAME_BYTES);
        assert!(!sanitized.ends_with([' ', '.']), "{sanitized:?}");
    }

    #[test]
    fn sanitize_never_produces_a_hidden_or_dot_only_name() {
        assert_eq!(sanitize(".hidden"), "hidden");
        assert_eq!(sanitize("..."), "download");
        assert_eq!(sanitize(".."), "download");
        assert_eq!(sanitize("."), "download");
        assert_eq!(sanitize("  spaced.txt  "), "spaced.txt");
        assert_eq!(sanitize("trail.dots..."), "trail.dots");
    }

    #[test]
    fn sanitize_defuses_windows_reserved_device_names() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("LPT9.log"), "_LPT9.log");
        assert_eq!(
            sanitize("console.txt"),
            "console.txt",
            "only the exact device names are reserved"
        );
        assert_eq!(sanitize("com10"), "com10");
    }

    #[test]
    fn sanitize_falls_back_to_a_default_for_an_empty_name() {
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize("   "), "download");
        assert_eq!(sanitize("///"), "download");
        assert_eq!(sanitize("\u{0}\u{1}"), "download");
    }

    #[test]
    fn sanitize_bounds_the_length_and_keeps_the_extension() {
        let long = format!("{}.txt", "a".repeat(300));
        let out = sanitize(&long);
        assert!(out.len() <= MAX_FILE_NAME_BYTES, "{}", out.len());
        assert!(out.ends_with(".txt"));

        let wide = format!("{}.bin", "日".repeat(200));
        let out = sanitize(&wide);
        assert!(out.len() <= MAX_FILE_NAME_BYTES);
        assert!(out.ends_with(".bin"));
        assert!(
            out.is_char_boundary(out.len()),
            "truncation must not split a multi-byte character"
        );
        assert!(out.chars().all(|c| c == '日' || ".bin".contains(c)));
    }

    #[test]
    fn sanitize_bounds_a_long_name_with_no_usable_extension() {
        let out = sanitize(&"b".repeat(500));
        assert_eq!(out.len(), MAX_FILE_NAME_BYTES);
        let out = sanitize(&format!("{}.{}", "c".repeat(100), "d".repeat(300)));
        assert!(
            out.len() <= MAX_FILE_NAME_BYTES,
            "an absurdly long extension is truncated, not preserved"
        );
    }

    #[test]
    fn choose_file_name_prefers_the_server_then_the_url_then_a_default() {
        assert_eq!(
            choose_file_name(
                Some(r#"attachment; filename="server.bin""#),
                "https://example.com/url.bin"
            ),
            "server.bin"
        );
        assert_eq!(
            choose_file_name(None, "https://example.com/url.bin"),
            "url.bin"
        );
        assert_eq!(
            choose_file_name(Some("inline"), "https://example.com/url.bin"),
            "url.bin"
        );
        assert_eq!(choose_file_name(None, "https://example.com/"), "download");
    }

    #[test]
    fn choose_file_name_sanitizes_whichever_source_it_uses() {
        assert_eq!(
            choose_file_name(
                Some(r#"attachment; filename="../../evil.sh""#),
                "https://example.com/x"
            ),
            "evil.sh"
        );
        assert_eq!(
            choose_file_name(None, "https://example.com/..%2F..%2Fpasswd"),
            "passwd"
        );
    }
}
