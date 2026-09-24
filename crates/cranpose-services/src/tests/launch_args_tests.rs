use std::cell::RefCell as StdRefCell;

use super::*;
use crate::run_test_composition;

fn args(entries: &[(&str, LaunchArgValue)]) -> LaunchArgs {
    LaunchArgs::new(
        entries
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone())),
        false,
    )
}

fn command_line(tokens: &[&str]) -> LaunchArgs {
    launch_args_from_command_line(tokens.iter().map(|token| (*token).to_string()), false)
}

#[test]
fn typed_extras_read_back_in_the_type_they_arrived_in() {
    let args = args(&[
        ("ob_autoplay", LaunchArgValue::Bool(true)),
        ("ob_level", LaunchArgValue::Int(7)),
        ("ob_seed", LaunchArgValue::Long(9_000_000_000)),
        ("ob_time_scale", LaunchArgValue::Float(0.5)),
        ("ob_screen", LaunchArgValue::Text("lobby".to_string())),
    ]);

    assert_eq!(args.boolean("ob_autoplay"), Some(true));
    assert_eq!(args.int("ob_level"), Some(7));
    assert_eq!(args.long("ob_seed"), Some(9_000_000_000));
    assert_eq!(args.float("ob_time_scale"), Some(0.5));
    assert_eq!(args.string("ob_screen"), Some("lobby"));
}

#[test]
fn a_missing_argument_reads_as_none_for_every_type() {
    let args = args(&[]);

    assert_eq!(args.boolean("absent"), None);
    assert_eq!(args.int("absent"), None);
    assert_eq!(args.long("absent"), None);
    assert_eq!(args.float("absent"), None);
    assert_eq!(args.string("absent"), None);
    assert!(!args.contains("absent"));
    assert!(args.is_empty());
}

#[test]
fn text_arguments_parse_into_the_requested_number_type() {
    let args = args(&[
        ("level", LaunchArgValue::Text("7".to_string())),
        ("seed", LaunchArgValue::Text("9000000000".to_string())),
        ("scale", LaunchArgValue::Text("0.25".to_string())),
        ("flag", LaunchArgValue::Text("ON".to_string())),
    ]);

    assert_eq!(args.int("level"), Some(7));
    assert_eq!(args.long("seed"), Some(9_000_000_000));
    assert_eq!(args.float("scale"), Some(0.25));
    assert_eq!(args.boolean("flag"), Some(true));
    assert_eq!(args.int("seed"), None, "a long that does not fit an i32");
    assert_eq!(args.boolean("level"), None, "numbers are not truthy");
}

#[test]
fn integer_arguments_widen_but_do_not_become_text() {
    let args = args(&[("level", LaunchArgValue::Int(7))]);

    assert_eq!(args.long("level"), Some(7));
    assert_eq!(args.float("level"), Some(7.0));
    assert_eq!(args.string("level"), None);
}

#[test]
fn the_command_line_maps_flags_and_assignments_to_arguments() {
    let args = command_line(&[
        "--ob_debug",
        "--ob_level=7",
        "positional",
        "--ob_screen=lobby",
    ]);

    assert_eq!(args.boolean("ob_debug"), Some(true));
    assert_eq!(args.int("ob_level"), Some(7));
    assert_eq!(args.string("ob_screen"), Some("lobby"));
    assert_eq!(
        args.len(),
        3,
        "positional arguments are not launch arguments"
    );
}

#[test]
fn the_command_line_stops_at_a_bare_double_dash() {
    let args = command_line(&["--before", "--", "--after"]);

    assert!(args.contains("before"));
    assert!(!args.contains("after"));
}

#[test]
fn the_first_value_wins_when_a_name_repeats() {
    let args = args(&[
        ("level", LaunchArgValue::Int(1)),
        ("level", LaunchArgValue::Int(2)),
    ]);

    assert_eq!(args.int("level"), Some(1));
    assert_eq!(args.len(), 1);
}

#[test]
fn the_installed_platform_snapshot_takes_precedence() {
    clear_platform_launch_args();
    set_platform_launch_args(Rc::new(args(&[(
        "ob_autoplay",
        LaunchArgValue::Bool(true),
    )])));

    assert_eq!(launch_args().boolean("ob_autoplay"), Some(true));

    clear_platform_launch_args();
    assert_eq!(launch_args().boolean("ob_autoplay"), None);
}

#[test]
fn debuggable_is_reported_by_the_snapshot() {
    clear_platform_launch_args();
    set_platform_launch_args(Rc::new(LaunchArgs::new(std::iter::empty(), true)));
    assert!(is_debuggable());

    set_platform_launch_args(Rc::new(LaunchArgs::new(std::iter::empty(), false)));
    assert!(!is_debuggable());
    clear_platform_launch_args();
}

#[test]
fn provide_launch_args_reaches_composition() {
    let captured = Rc::new(StdRefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let provided = Rc::new(LaunchArgs::new(
                [("ob_level".to_string(), LaunchArgValue::Int(3))],
                true,
            ));
            ProvideLaunchArgs(provided, move || {
                *captured.borrow_mut() = Some((
                    local_launch_args().current().int("ob_level"),
                    isDebuggable(),
                ));
            });
        });
    }

    assert_eq!(*captured.borrow(), Some((Some(3), true)));
}
