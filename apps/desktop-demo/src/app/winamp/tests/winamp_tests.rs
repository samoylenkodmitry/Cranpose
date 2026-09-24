use super::*;

#[test]
fn time_digits_are_mapped_correctly() {
    assert_eq!(time_digits(0.0), [0, 0, 0, 0]);
    assert_eq!(time_digits(1.0), [0, 5, 0, 0]);
}

#[test]
fn slider_helpers_clamp_values() {
    assert_eq!(slider_frame(-1.0, 28), 0);
    assert_eq!(slider_frame(2.0, 28), 27);
    assert_eq!(slider_thumb_x(-1.0, 248.0, 29.0), 0.0);
    assert_eq!(slider_thumb_x(2.0, 248.0, 29.0), 219.0);
}

#[test]
fn vertical_slider_helpers_clamp_values() {
    assert_eq!(vertical_slider_thumb_y(-1.0, 63.0, 11.0), 52.0);
    assert_eq!(vertical_slider_thumb_y(2.0, 63.0, 11.0), 0.0);
    assert_eq!(vertical_slider_thumb_y_down(-1.0, 145.0, 18.0), 0.0);
    assert_eq!(vertical_slider_thumb_y_down(2.0, 145.0, 18.0), 127.0);
}

#[test]
fn winamp_debug_env_flags_are_not_process_cached() {
    let source = include_str!("../mod.rs");
    let once_lock_bool = ["Once", "Lock<bool>"].concat();
    let cached_env = ["get_or_init(|| ", "std::env::var_os"].concat();
    assert!(!source.contains(&once_lock_bool));
    assert!(!source.contains(&cached_env));
}
