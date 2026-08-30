pub(crate) fn escape_wire_field(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
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
