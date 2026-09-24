use cranpose::prelude::*;
use cranpose_core::rememberMutableStateOf;
use cranpose_coroflow::rememberHandle;
use cranpose_navigation::{rememberNavController, NavController, NavHost, NavOptions};
use cranpose_ui::widgets::{PaddingValues, Scaffold};

use crate::{
    data::tasks::TasksRepository,
    screens::{HomeScreen, SettingsScreen, TasksScreen},
    theme::{body_text_style, heading_text_style, Palette},
};

const TITLE: &str = "Cranpose Isolated Demo";

/// Where the app can go. `NavHost` gives each screen its own view model
/// store; add a variant, with fields for its arguments, for every new screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Tasks,
    Settings,
}

impl Screen {
    const ALL: [Screen; 3] = [Screen::Home, Screen::Tasks, Screen::Settings];

    fn label(self) -> &'static str {
        match self {
            Screen::Home => "Home",
            Screen::Tasks => "Tasks",
            Screen::Settings => "Settings",
        }
    }
}

pub(crate) fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title(TITLE)
        .with_size(900, 600)
        .with_fonts(crate::fonts::DEMO_FONTS)
        .with_fps_counter(true)
}

#[composable]
pub(crate) fn IsolatedDemoApp() {
    let nav = rememberNavController(Screen::Home);
    let dark_mode = rememberMutableStateOf(|| false);
    let tasks = rememberHandle(TasksRepository::default);
    let palette = Palette::for_mode(dark_mode.value());
    let current = nav.current_route().unwrap_or(Screen::Home);

    Scaffold(
        Modifier::empty()
            .fill_max_size()
            .background(palette.background),
        move || TopBar(palette, current.label()),
        move || BottomNav(palette, nav, current),
        move |padding: PaddingValues| {
            Box(
                padding.apply_to(Modifier::empty().fill_max_size()),
                BoxSpec::default(),
                move || {
                    NavHost(nav, move |screen| match screen {
                        Screen::Home => HomeScreen(palette),
                        Screen::Tasks => TasksScreen(palette, tasks),
                        Screen::Settings => SettingsScreen(palette, dark_mode),
                    });
                },
            );
        },
    );
}

#[composable]
fn TopBar(palette: Palette, active_label: &'static str) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(16.0)
            .background(palette.surface),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(TITLE, Modifier::empty(), heading_text_style(palette.text));
            Spacer(Size {
                width: 12.0,
                height: 0.0,
            });
            Text(
                active_label,
                Modifier::empty(),
                body_text_style(palette.muted_text),
            );
        },
    );
}

#[composable]
fn BottomNav(palette: Palette, nav: NavController<Screen>, current: Screen) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(palette.surface),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceEvenly),
        move || {
            for candidate in Screen::ALL {
                NavButton(palette, candidate, candidate == current, move || {
                    // Tabs replace each other above Home instead of stacking.
                    nav.navigate_with(
                        candidate,
                        NavOptions::new()
                            .pop_up_to(Screen::Home, false)
                            .launch_single_top(),
                    );
                });
            }
        },
    );
}

#[composable]
fn NavButton(palette: Palette, candidate: Screen, active: bool, on_click: impl Fn() + 'static) {
    let background = if active {
        palette.primary
    } else {
        palette.surface
    };
    let label_color = if active {
        palette.on_primary
    } else {
        palette.muted_text
    };

    Button(
        Modifier::empty()
            .padding(10.0)
            .background(background)
            .rounded_corners(8.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                candidate.label(),
                Modifier::empty(),
                body_text_style(label_color),
            );
        },
    );
}
