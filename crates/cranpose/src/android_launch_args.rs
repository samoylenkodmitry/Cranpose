use cranpose_services::{LaunchArgValue, LaunchArgs};

use crate::android_wire_escape::unescape_wire_field as unescape;

pub(crate) fn decode_launch_arguments(payload: &str) -> LaunchArgs {
    let mut lines = payload.split('\n');
    let debuggable = matches!(lines.next(), Some("1"));
    LaunchArgs::new(lines.filter_map(decode_record), debuggable)
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
#[path = "tests/android_launch_args_tests.rs"]
mod tests;
