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
