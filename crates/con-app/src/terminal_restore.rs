pub(crate) fn restored_terminal_output(lines: Option<&[String]>) -> Option<Vec<u8>> {
    let lines = lines?;
    if lines.is_empty() {
        return None;
    }

    let mut output = String::new();
    for line in lines {
        for ch in line.chars() {
            if ch == '\t' || !ch.is_control() {
                output.push(ch);
            }
        }
        output.push_str("\r\n");
    }

    (!output.trim().is_empty()).then(|| output.into_bytes())
}
