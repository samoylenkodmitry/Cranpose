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
mod tests {
    use super::*;

    #[test]
    fn decoding_recovers_every_extra_type() {
        let args = decode_launch_arguments(concat!(
            "1\n",
            "b\tob_autoplay\t1\n",
            "b\tob_unlock\t0\n",
            "i\tob_level\t7\n",
            "l\tob_seed\t9000000000\n",
            "f\tob_time_scale\t0.5\n",
            "s\tob_screen\tlobby"
        ));

        assert!(args.is_debuggable());
        assert_eq!(args.boolean("ob_autoplay"), Some(true));
        assert_eq!(args.boolean("ob_unlock"), Some(false));
        assert_eq!(args.int("ob_level"), Some(7));
        assert_eq!(args.long("ob_seed"), Some(9_000_000_000));
        assert_eq!(args.float("ob_time_scale"), Some(0.5));
        assert_eq!(args.string("ob_screen"), Some("lobby"));
    }

    #[test]
    fn decoding_reports_a_release_build_as_not_debuggable() {
        let args = decode_launch_arguments("0\nb\tob_debug\t1");

        assert!(!args.is_debuggable());
        assert_eq!(
            args.boolean("ob_debug"),
            Some(true),
            "extras still arrive; gating them is the app's decision"
        );
    }

    #[test]
    fn decoding_restores_escaped_names_and_values() {
        let args = decode_launch_arguments("1\ns\ttab%09name\tone%0Atwo%25three");

        assert_eq!(args.string("tab\tname"), Some("one\ntwo%three"));
    }

    #[test]
    fn decoding_skips_records_it_cannot_type() {
        let args = decode_launch_arguments(concat!(
            "1\n",
            "x\tparcelable\t?\n",
            "i\tbroken\tnot-a-number\n",
            "i\ttruncated\n",
            "s\t\tunnamed\n",
            "i\tob_level\t7"
        ));

        assert_eq!(args.names().collect::<Vec<_>>(), vec!["ob_level"]);
        assert_eq!(args.int("ob_level"), Some(7));
    }

    #[test]
    fn decoding_an_empty_payload_yields_no_arguments() {
        let args = decode_launch_arguments("");

        assert!(args.is_empty());
        assert!(!args.is_debuggable());
    }

    #[test]
    fn decoding_a_launch_without_extras_keeps_the_debuggable_flag() {
        let args = decode_launch_arguments("1");

        assert!(args.is_empty());
        assert!(args.is_debuggable());
    }
}
