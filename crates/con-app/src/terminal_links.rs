//! Visible-row terminal link detection for GPUI-owned terminal renderers.
//!
//! macOS delegates this to embedded libghostty. Windows and Linux own
//! their terminal paint/input path, so they need a small detector for
//! modifier-clicking plain URLs in the visible grid. Keep this module
//! off the paint path: callers should use it only on mouse gestures.

#[cfg(any(target_os = "windows", target_os = "linux"))]
use con_ghostty::vt::ScreenSnapshot;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use gpui::Modifiers;

#[cfg(any(target_os = "windows", target_os = "linux", test))]
const URL_SCHEMES: &[&str] = &[
    "http://",
    "https://",
    "mailto:",
    "ftp://",
    "file:",
    "ssh:",
    "git://",
    "tel:",
    "magnet:",
    "ipfs://",
    "ipns://",
    "gemini://",
    "gopher://",
    "news:",
];

#[cfg(any(target_os = "windows", target_os = "linux", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalLink {
    pub(crate) url: String,
    pub(crate) row: u16,
    /// Inclusive start column.
    pub(crate) start_col: u16,
    /// Exclusive end column.
    pub(crate) end_col: u16,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn should_open_link(modifiers: &Modifiers) -> bool {
    modifiers.control
        && !modifiers.alt
        && !modifiers.shift
        && !modifiers.platform
        && !modifiers.function
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn link_at_snapshot(
    snapshot: &ScreenSnapshot,
    col: u16,
    row: u16,
) -> Option<TerminalLink> {
    if row >= snapshot.rows || col >= snapshot.cols || snapshot.cols == 0 {
        return None;
    }

    let row_start = usize::from(row) * usize::from(snapshot.cols);
    let row_end = row_start + usize::from(snapshot.cols);
    let cells = snapshot.cells.get(row_start..row_end)?;

    let mut line = String::with_capacity(cells.len());
    let mut col_byte_ranges = Vec::with_capacity(cells.len());
    for cell in cells {
        let start = line.len();
        let ch = match cell.codepoint {
            0 => ' ',
            codepoint => char::from_u32(codepoint).unwrap_or('\u{FFFD}'),
        };
        line.push(ch);
        col_byte_ranges.push((start, line.len()));
    }

    let col = usize::from(col);
    let (hover_start, hover_end) = *col_byte_ranges.get(col)?;
    link_at_line(&line, hover_start, hover_end).map(|mut link| {
        link.row = row;
        link.start_col = byte_to_col(&col_byte_ranges, link.start_col as usize) as u16;
        link.end_col = byte_to_col_end(&col_byte_ranges, link.end_col as usize) as u16;
        link
    })
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn link_at_line(line: &str, hover_start: usize, hover_end: usize) -> Option<TerminalLink> {
    let mut search_from = 0;
    while let Some((scheme_start, scheme_len)) = find_next_scheme(line, search_from) {
        let mut end = consume_url(line, scheme_start);
        end = trim_url_end(line, scheme_start, end);
        let has_payload = end > scheme_start + scheme_len;

        if has_payload
            && byte_range_intersects(scheme_start, end, hover_start, hover_end)
            && let Ok(start_col) = u16::try_from(scheme_start)
            && let Ok(end_col) = u16::try_from(end)
        {
            return Some(TerminalLink {
                url: line[scheme_start..end].to_string(),
                row: 0,
                start_col,
                end_col,
            });
        }

        search_from = end.max(scheme_start + scheme_len);
    }

    None
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn find_next_scheme(line: &str, start: usize) -> Option<(usize, usize)> {
    let mut index = start.min(line.len());
    while index < line.len() {
        if !line.is_char_boundary(index) {
            index += 1;
            continue;
        }

        for scheme in URL_SCHEMES {
            if starts_with_ascii_ci(line, index, scheme) && is_scheme_boundary(line, index) {
                return Some((index, scheme.len()));
            }
        }

        index += 1;
    }
    None
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn starts_with_ascii_ci(line: &str, index: usize, needle: &str) -> bool {
    line.as_bytes()
        .get(index..index + needle.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn is_scheme_boundary(line: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }

    line[..index]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn consume_url(line: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, ch) in line[start..].char_indices() {
        if is_url_terminator(ch) {
            break;
        }
        end = start + offset + ch.len_utf8();
    }
    end
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn is_url_terminator(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(ch, '"' | '\'' | '`' | '<' | '>' | '{' | '}' | '|' | '\\')
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn trim_url_end(line: &str, start: usize, mut end: usize) -> usize {
    loop {
        let Some(last) = line[start..end].chars().next_back() else {
            return end;
        };

        let should_trim = matches!(last, '.' | ',' | ';' | ':' | '!')
            || (last == ')'
                && count_char(line, start, end, ')') > count_char(line, start, end, '('))
            || (last == ']'
                && count_char(line, start, end, ']') > count_char(line, start, end, '['));

        if !should_trim {
            return end;
        }
        end -= last.len_utf8();
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn count_char(line: &str, start: usize, end: usize, needle: char) -> usize {
    line[start..end].chars().filter(|ch| *ch == needle).count()
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn byte_range_intersects(start: usize, end: usize, hover_start: usize, hover_end: usize) -> bool {
    let hover_end = hover_end.max(hover_start + 1);
    start < hover_end && end > hover_start
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn byte_to_col(col_byte_ranges: &[(usize, usize)], byte: usize) -> usize {
    col_byte_ranges
        .iter()
        .position(|(_, end)| *end > byte)
        .unwrap_or(col_byte_ranges.len().saturating_sub(1))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn byte_to_col_end(col_byte_ranges: &[(usize, usize)], byte_end: usize) -> usize {
    if byte_end == 0 {
        return 0;
    }
    byte_to_col(col_byte_ranges, byte_end - 1).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(line: &str, needle: &str) -> Option<String> {
        let index = line.find(needle).expect("needle in line");
        link_at_line(line, index, index + 1).map(|link| link.url)
    }

    #[test]
    fn detects_scheme_url() {
        assert_eq!(
            detect("visit https://example.com now", "example"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn trims_sentence_punctuation() {
        assert_eq!(
            detect("visit https://example.com, then continue.", "example"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            detect("visit https://example.com.", "example"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn trims_unbalanced_closing_paren() {
        assert_eq!(
            detect("open (https://example.com/path)", "example"),
            Some("https://example.com/path".to_string())
        );
    }

    #[test]
    fn keeps_balanced_url_parens() {
        assert_eq!(
            detect(
                "see https://en.wikipedia.org/wiki/Rust_(video_game)",
                "wikipedia"
            ),
            Some("https://en.wikipedia.org/wiki/Rust_(video_game)".to_string())
        );
    }

    #[test]
    fn handles_query_strings() {
        assert_eq!(
            detect("open https://example.com/search?q=rust&sort=desc", "search"),
            Some("https://example.com/search?q=rust&sort=desc".to_string())
        );
    }

    #[test]
    fn requires_a_scheme_boundary() {
        assert_eq!(detect("prefixhttps://example.com", "example"), None);
    }
}
