use super::*;

fn checkered_icon() -> ImageBitmap {
    let pixels = [[255, 0, 0, 255], [0, 0, 255, 128]]
        .into_iter()
        .cycle()
        .take(4)
        .flatten()
        .collect();
    ImageBitmap::from_rgba8(2, 2, pixels).expect("a 2x2 RGBA picture is a valid bitmap")
}

#[test]
fn an_application_has_no_window_icon_until_it_names_one() {
    assert!(AppSettings::default().window_icon.is_none());
    assert!(AppLauncher::new().settings.window_icon.is_none());
}

#[test]
fn the_named_window_icon_reaches_the_settings_unchanged() {
    let icon = checkered_icon();
    let settings = AppLauncher::new().with_window_icon(icon.clone()).settings;
    let stored = settings
        .window_icon
        .expect("with_window_icon stores the picture");

    assert_eq!((stored.width(), stored.height()), (2, 2));
    assert_eq!(stored.pixels(), icon.pixels());
}

#[test]
fn a_later_window_icon_replaces_an_earlier_one() {
    let wide = ImageBitmap::from_rgba8(2, 1, vec![9; 8]).expect("a 2x1 bitmap");
    let settings = AppLauncher::new()
        .with_window_icon(checkered_icon())
        .with_window_icon(wide)
        .settings;
    let stored = settings.window_icon.expect("the second icon is kept");

    assert_eq!((stored.width(), stored.height()), (2, 1));
}
