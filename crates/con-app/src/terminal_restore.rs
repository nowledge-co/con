pub(crate) fn restored_terminal_output(lines: Option<&[String]>) -> Option<Vec<u8>> {
    con_ghostty::restored_terminal_output_text(lines?).map(String::into_bytes)
}
