// A root that composes again from a value the platform changed, as
// Android's night mode makes it (#2063), draws what a rebuild of the tree
// draws: every page that took the new value paints it in that update.

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_ui::BoxWithConstraints;

use super::*;

thread_local! {
    // What the platform last reported, read by the root without a state.
    static PLATFORM_DARK: Cell<bool> = const { Cell::new(false) };
    static DARK: CompositionLocal<bool> = compositionLocalOf(|| false);
}

fn dark() -> CompositionLocal<bool> {
    DARK.with(Clone::clone)
}

fn page_color(is_dark: bool) -> Color {
    if is_dark {
        Color(0.1, 0.1, 0.1, 1.0)
    } else {
        Color(0.95, 0.95, 0.95, 1.0)
    }
}

// A page that paints the theme its caller read, under text that reads it
// itself.
#[composable]
fn ThemedPage(is_dark: bool) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .height(40.0)
            .background(page_color(is_dark)),
        ColumnSpec::default(),
        || {
            let text_dark = dark().current();
            Text(
                if text_dark { "dark" } else { "light" },
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

// A tab whose body reads the theme for its page, as the demo's tabs do,
// keyed as the demo keys its active tab.
#[composable]
fn KeyedTab() {
    let tab = 1u32;
    key(tab, || ThemedPage(dark().current()));
}

#[composable]
fn ThemedApp() {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        || {
            KeyedTab();
            BoxWithConstraints(Modifier::empty().fill_max_width(), |_| {
                ThemedPage(dark().current());
            });
        },
    );
}

#[test]
fn a_root_composed_again_from_a_platform_value_draws_its_new_theme_at_once() {
    PLATFORM_DARK.with(|dark| dark.set(false));
    let mut counted = CountedShell::settled(location_key(file!(), line!(), column!()), || {
        let platform_dark = PLATFORM_DARK.with(Cell::get);
        CompositionLocalProvider([dark().provides(platform_dark)], ThemedApp);
    });
    assert_scene_matches_rebuild(&mut counted.shell, "the light frame");

    // As Android's configuration change does.
    PLATFORM_DARK.with(|dark| dark.set(true));
    counted.shell.request_root_render();
    counted.shell.update_without_frame();
    assert_eq!(drawn_texts(&counted.shell), ["dark", "dark"]);
    assert_scene_matches_rebuild(&mut counted.shell, "the frame after the switch");
}
