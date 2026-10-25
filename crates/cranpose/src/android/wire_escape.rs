/// `value` with the wire's escapes undone: `%09`, `%0A` and `%0D` back to a
/// tab, newline and carriage return, and `%25` to `%`, last.
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
