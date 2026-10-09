use cranpose::{AppLauncher, Robot};
use desktop_app::app::{DemoTab, TEST_ACTIVE_TAB_STATE};

/// The robot app hook that switches the demo to a tab.
pub const SET_TAB: &str = "set-tab";

pub fn launch(title: &str, width: u32, height: u32) -> AppLauncher {
    AppLauncher::new()
        .with_title(title)
        .with_size(width, height)
        .with_headless(true)
}

pub fn counter_demo() {
    desktop_app::app::combined_app_with_initial_tab(Some(desktop_app::app::DemoTab::Counter));
}

/// A robot app hook that answers only [`SET_TAB`]. Runners install it and
/// reach a tab with [`switch_tab`] instead of clicking its button, because tab
/// buttons move whenever a tab is added.
pub fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != SET_TAB {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    set_tab(&argument)
}

/// Switches the demo to the tab whose slug or alias is `name`, as
/// [`DemoTab::from_startup_name`] reads it. Hooks that answer more names than
/// [`set_tab_hook`] call it for [`SET_TAB`].
pub fn set_tab(name: &str) -> Result<Option<String>, String> {
    let tab =
        DemoTab::from_startup_name(name).ok_or_else(|| format!("unknown demo tab '{name}'"))?;
    TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| format!("active tab state was not installed before selecting {tab:?}"))?
        .set(tab);
    Ok(None)
}

/// Switches the running demo to `tab` through the installed [`SET_TAB`] hook.
pub fn switch_tab(robot: &Robot, tab: &str) {
    if let Err(err) = robot.invoke_app_hook(SET_TAB, tab) {
        panic!("failed to select tab '{tab}': {err}");
    }
}
