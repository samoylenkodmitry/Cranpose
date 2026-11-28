use cranpose_services::{LaunchArgValue, LaunchArgs};

use super::wire_escape::unescape_wire_field as unescape;

pub(crate) fn decode_launch_arguments(payload: &str) -> LaunchArgs {
    let mut lines = payload.split('\n');
    let mut header = lines.next().unwrap_or_default().split('\t');
    let debuggable = matches!(header.next(), Some("1"));
    let component = header.next().map(unescape).filter(|name| !name.is_empty());
    let args = LaunchArgs::new(lines.filter_map(decode_record), debuggable);
    match component {
        Some(class_name) => args.with_component_class_name(class_name),
        None => args,
    }
}

fn decode_record(record: &str) -> Option<(String, LaunchArgValue)> {
    let mut fields = record.split('\t');
    let kind = fields.next()?;
    let name = unescape(fields.next()?);
    let raw = fields.next()?;
    if name.is_empty() {
        return None;
    }
    let value = match kind {
        "b" => LaunchArgValue::Bool(raw == "1"),
        "i" => LaunchArgValue::Int(raw.parse().ok()?),
        "l" => LaunchArgValue::Long(raw.parse().ok()?),
        "f" => LaunchArgValue::Float(raw.parse().ok()?),
        "s" => LaunchArgValue::Text(unescape(raw)),
        _ => return None,
    };
    Some((name, value))
}

#[cfg(test)]
#[path = "tests/launch_args_tests.rs"]
mod tests;
