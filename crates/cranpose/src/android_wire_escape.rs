/// Appends `value` to `out` with the wire's delimiters, `%` and `extra`
/// written as `%` and their two hex digits, in one pass over the value.
pub(crate) fn push_escaped_wire_field(out: &mut String, value: &str, extra: char) {
    for character in value.chars() {
        match character {
            '%' => out.push_str("%25"),
            '\t' => out.push_str("%09"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            character if character == extra => {
                // A delimiter is ASCII, so two hex digits hold it.
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                let code = u32::from(character) as usize;
                out.push('%');
                out.push(char::from(HEX[(code >> 4) & 0xF]));
                out.push(char::from(HEX[code & 0xF]));
            }
            character => out.push(character),
        }
    }
}

pub(crate) fn unescape_wire_field(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    value
        .replace("%09", "\t")
        .replace("%0A", "\n")
        .replace("%0D", "\r")
        .replace("%25", "%")
}
