use cranpose::{
    composable,
    liquid::prelude::*,
    local_safe_area_insets, rememberMutableStateOf,
    widgets::{Box, BoxSpec, Text},
    Alignment, Color, Modifier,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Slider,
    Toggle,
    Segmented,
    Button,
    ProminentButton,
    Chip,
    Card,
    Menu,
    IconButton,
    Search,
    NavBar,
    ListRow,
}

impl Control {
    pub(crate) fn parse(name: &str) -> anyhow::Result<Self> {
        Ok(match name {
            "slider" => Self::Slider,
            "toggle" => Self::Toggle,
            "segmented" => Self::Segmented,
            "button" => Self::Button,
            "prominent-button" => Self::ProminentButton,
            "chip" => Self::Chip,
            "card" => Self::Card,
            "menu" => Self::Menu,
            "icon-button" => Self::IconButton,
            "search" => Self::Search,
            "nav-bar" => Self::NavBar,
            "list-row" => Self::ListRow,
            _ => anyhow::bail!("unknown reference control: {name}"),
        })
    }
}

#[composable]
pub(crate) fn LiquidControlReference(
    control: Control,
    checkerboard: bool,
    dark: bool,
    initial: f32,
    gray: Option<f32>,
) {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: if dark {
                SchemeMode::Dark
            } else {
                SchemeMode::Light
            },
            accent: Color::from_rgb_u8(0, if dark { 145 } else { 136 }, 255),
            ..Default::default()
        },
        move || {
            let insets = local_safe_area_insets().current();
            Box(
                gray.map_or_else(
                    || super::liquid_tab_reference::reference_background(checkerboard, dark),
                    |value| {
                        Modifier::empty()
                            .fill_max_size()
                            .background(Color::rgba(value, value, value, 1.0))
                    },
                ),
                BoxSpec::default(),
                move || {
                    Text(
                        "Reference control",
                        Modifier::empty().offset(
                            150.0,
                            if control == Control::NavBar {
                                780.0
                            } else {
                                insets.top + 24.0
                            },
                        ),
                        liquid_typography().caption1,
                    );
                    Box(
                        Modifier::empty().fill_max_size().offset(
                            0.0,
                            if control == Control::NavBar {
                                0.0
                            } else {
                                (insets.top - insets.bottom) * 0.5
                            },
                        ),
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || ControlContent(control, initial),
                    );
                    if std::env::var("REFERENCE_RECORDING").as_deref() == Ok("1") {
                        super::liquid_tab_reference::RecordingOverlay();
                    }
                },
            );
        },
    );
}

#[composable]
fn ControlContent(control: Control, initial: f32) {
    match control {
        Control::Slider | Control::Toggle | Control::Segmented | Control::Chip => {
            ReferenceSelection(control, initial);
        }
        _ => ReferenceSurface(control, initial),
    }
}

#[composable]
fn ReferenceSelection(control: Control, initial: f32) {
    let value = rememberMutableStateOf(move || initial);
    let selected = rememberMutableStateOf(move || initial as usize);
    let checked = rememberMutableStateOf(move || initial == 1.0);
    match control {
        Control::Slider => LiquidSlider(Modifier::empty().width(300.0), value.get(), move |next| {
            value.set(next);
        }),
        Control::Toggle => LiquidToggle(
            Modifier::empty().content_description("Enabled"),
            checked.get(),
            move |next| checked.set(next),
        ),
        Control::Segmented => LiquidSegmentedControl(
            Modifier::empty().width(300.0),
            selected.get(),
            move |next| selected.set(next),
            move |scope| {
                scope.segment("All");
                scope.segment("Unread");
                scope.segment("Saved");
            },
        ),
        Control::Chip => LiquidActionChip(
            Modifier::empty(),
            checked.get(),
            move || checked.set(!checked.get()),
            "Unread",
        ),
        _ => unreachable!("selection controls are dispatched by ControlContent"),
    }
}

#[composable]
fn ReferenceSurface(control: Control, initial: f32) {
    match control {
        Control::Button | Control::ProminentButton => {
            let spec = if matches!(control, Control::ProminentButton) {
                GlassButtonSpec::prominent()
            } else {
                GlassButtonSpec::glass()
            };
            ReferenceButton("Continue", spec.with_size(GlassButtonSize::Large), || {});
        }
        Control::Card => LiquidCard(Modifier::empty().size_points(300.0, 160.0), || {
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                || {
                    let mut style = liquid_typography().body;
                    style.span_style.color = Some(liquid_colors().label);
                    Text("Content", Modifier::empty(), style);
                },
            );
        }),
        Control::Menu => ReferenceMenu(initial == 1.0),
        Control::IconButton => GlassIconButton(
            Modifier::empty().content_description("Search"),
            GlassButtonSpec::glass(),
            44.0,
            || {},
            icons::SEARCH,
        ),
        Control::Search => {
            let state = cranpose::remember(|| cranpose_foundation::text::TextFieldState::new(""))
                .with(|state| *state);
            LiquidSearchField(
                Modifier::empty().width(300.0),
                state,
                LiquidSearchFieldSpec::default(),
            );
        }
        Control::NavBar => ReferenceNavBar(),
        Control::ListRow => LiquidListRow(
            Modifier::empty().width(300.0),
            LiquidListRowSpec::default().with_separator(true),
            || {},
            || {
                let mut style = liquid_typography().body;
                style.span_style.color = Some(liquid_colors().label);
                Text("Content", Modifier::empty(), style);
            },
        ),
        _ => unreachable!("surface controls are dispatched by ControlContent"),
    }
}

#[composable]
fn ReferenceButton(label: &'static str, spec: GlassButtonSpec, on_click: impl Fn() + 'static) {
    GlassButton(Modifier::empty(), spec.clone(), on_click, move || {
        GlassButtonLabel(label, spec.clone());
    });
}

#[composable]
fn ReferenceNavBar() {
    let scroll = cranpose::remember(|| cranpose::ScrollState::new(0.0)).with(|state| *state);
    let insets = local_safe_area_insets().current();
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            LiquidNavBar(
                Modifier::empty()
                    .fill_max_width()
                    .height(liquid_nav_bar_expanded_height())
                    .offset(0.0, insets.top),
                LiquidNavBarSpec::new("Library"),
                scroll,
                || {},
                || {},
            );
        },
    );
}

#[composable]
fn ReferenceMenu(grouped: bool) {
    let expanded = rememberMutableStateOf(|| false);
    LiquidDropdownMenu(
        Modifier::empty(),
        expanded.get(),
        LiquidDropdownMenuSpec::default(),
        move || expanded.set(false),
        move || {
            ReferenceButton("Options", GlassButtonSpec::glass(), move || {
                expanded.set(true);
            });
        },
        move |scope| {
            if grouped {
                scope.header("Document");
            }
            scope.item(LiquidMenuItem::new("Copy"), || {});
            scope.item(LiquidMenuItem::new("Share"), || {});
            if grouped {
                scope.separator();
                scope.header("Danger");
            }
            scope.item(LiquidMenuItem::new("Delete").destructive(), || {});
            if grouped {
                scope.item(LiquidMenuItem::new("Cancel"), || {});
            }
        },
    );
}

#[cfg(test)]
#[path = "tests/liquid_control_reference_tests.rs"]
mod tests;
