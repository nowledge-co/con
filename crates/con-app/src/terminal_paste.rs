use std::path::PathBuf;

use gpui::{ClipboardEntry, ClipboardItem, ExternalPaths};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TerminalPastePayload {
    Text(String),
    ForwardCtrlV,
}

pub fn payload_from_clipboard(item: &ClipboardItem) -> Option<TerminalPastePayload> {
    let paths = external_paths_from_entries(item.entries());
    if !paths.is_empty() {
        return quoted_paths_text(&paths).map(TerminalPastePayload::Text);
    }

    if item
        .entries()
        .iter()
        .any(|entry| matches!(entry, ClipboardEntry::Image(image) if !image.bytes.is_empty()))
    {
        return Some(TerminalPastePayload::ForwardCtrlV);
    }

    let text = text_from_entries(item.entries());
    if let Some(paths) = paths_from_uri_list(&text)
        && !paths.is_empty()
    {
        return quoted_paths_text(&paths).map(TerminalPastePayload::Text);
    }

    if !text.is_empty() {
        return Some(TerminalPastePayload::Text(text));
    }

    None
}

pub fn payload_from_external_paths(paths: &ExternalPaths) -> Option<TerminalPastePayload> {
    quoted_paths_text(paths.paths()).map(TerminalPastePayload::Text)
}

fn external_paths_from_entries(entries: &[ClipboardEntry]) -> Vec<PathBuf> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths()),
            _ => None,
        })
        .flatten()
        .cloned()
        .collect()
}

fn text_from_entries(entries: &[ClipboardEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::String(string) => Some(string.text.as_str()),
            _ => None,
        })
        .collect()
}

fn paths_from_uri_list(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(path) = path_from_file_uri(line) else {
            return None;
        };
        paths.push(path);
    }

    if paths.is_empty() { None } else { Some(paths) }
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest
        .strip_prefix("localhost/")
        .map(|path| format!("/{path}"))
        .unwrap_or_else(|| rest.to_string());

    if !path.starts_with('/') {
        return None;
    }

    Some(PathBuf::from(percent_decode(&path)?))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = *bytes.get(index + 1)?;
            let lo = *bytes.get(index + 2)?;
            decoded.push((hex_value(hi)? << 4) | hex_value(lo)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn quoted_paths_text(paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }

    let mut text = String::new();
    for path in paths {
        text.push(' ');
        text.push_str(&format!("{path:?}"));
    }
    text.push(' ');
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{ClipboardEntry, ClipboardItem, ClipboardString, ExternalPaths, Image, ImageFormat};

    use super::{TerminalPastePayload, payload_from_clipboard, quoted_paths_text};

    #[test]
    fn quoted_paths_are_space_padded() {
        let paths = vec![
            PathBuf::from("plain.txt"),
            PathBuf::from("name with spaces.png"),
        ];

        assert_eq!(
            quoted_paths_text(&paths),
            Some(" \"plain.txt\" \"name with spaces.png\" ".to_string())
        );
    }

    #[test]
    fn clipboard_text_becomes_text_payload() {
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::String(ClipboardString::new(
                "echo hello".to_string(),
            ))],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::Text("echo hello".to_string()))
        );
    }

    #[test]
    fn clipboard_paths_win_over_lossy_text_fallback() {
        let item = ClipboardItem {
            entries: vec![
                ClipboardEntry::String(ClipboardString::new(
                    "lossy platform path text".to_string(),
                )),
                ClipboardEntry::ExternalPaths(ExternalPaths(
                    vec![PathBuf::from("one.txt"), PathBuf::from("two words.txt")].into(),
                )),
            ],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::Text(
                " \"one.txt\" \"two words.txt\" ".to_string()
            ))
        );
    }

    #[test]
    fn clipboard_image_wins_over_text_representation() {
        let image = Image::from_bytes(ImageFormat::Png, vec![137, 80, 78, 71]);
        let item = ClipboardItem {
            entries: vec![
                ClipboardEntry::String(ClipboardString::new(
                    "https://example.com/image.png".to_string(),
                )),
                ClipboardEntry::Image(image),
            ],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::ForwardCtrlV)
        );
    }

    #[test]
    fn copied_image_and_code_files_paste_as_paths() {
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                vec![PathBuf::from("diagram.png"), PathBuf::from("main.rs")].into(),
            ))],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::Text(
                " \"diagram.png\" \"main.rs\" ".to_string()
            ))
        );
    }

    #[test]
    fn linux_file_uri_clipboard_pastes_as_paths() {
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::String(ClipboardString::new(
                "# copied files\nfile:///home/me/diagram%20one.png\nfile:///home/me/main.rs\n"
                    .to_string(),
            ))],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::Text(
                " \"/home/me/diagram one.png\" \"/home/me/main.rs\" ".to_string()
            ))
        );
    }

    #[test]
    fn ordinary_file_url_text_stays_text_when_mixed_with_words() {
        let text = "see file:///home/me/main.rs";
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::String(ClipboardString::new(
                text.to_string(),
            ))],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::Text(text.to_string()))
        );
    }

    #[test]
    fn image_only_clipboard_forwards_native_paste_to_tui() {
        let image = Image::from_bytes(ImageFormat::Png, vec![137, 80, 78, 71]);
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::Image(image)],
        };

        assert_eq!(
            payload_from_clipboard(&item),
            Some(TerminalPastePayload::ForwardCtrlV)
        );
    }

    #[test]
    fn empty_image_clipboard_is_ignored() {
        let item = ClipboardItem {
            entries: vec![ClipboardEntry::Image(Image::empty())],
        };

        assert_eq!(payload_from_clipboard(&item), None);
    }
}
