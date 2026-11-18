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
    FilterChip,
    Card,
    Menu,
    IconButton,
    Search,
    NavBar,
    ListRow,
    Stepper,
    Progress,
    Activity,
    Gauge,
    Disclosure,
    TextField,
    PageIndicator,
    Wheel,
    WheelSame,
    Time,
    Time12,
    PopUp,
    ColorWell,
    Unavailable,
    Group,
    Alert,
    Confirmation,
}

// The names shared with the native app's `REFERENCE_COMPONENT`.
const NAMES: [(&str, Control); 30] = [
    ("slider", Control::Slider),
    ("toggle", Control::Toggle),
    ("segmented", Control::Segmented),
    ("button", Control::Button),
    ("prominent-button", Control::ProminentButton),
    ("chip", Control::Chip),
    ("filter-chip", Control::FilterChip),
    ("card", Control::Card),
    ("menu", Control::Menu),
    ("icon-button", Control::IconButton),
    ("search", Control::Search),
    ("nav-bar", Control::NavBar),
    ("list-row", Control::ListRow),
    ("stepper", Control::Stepper),
    ("progress", Control::Progress),
    ("activity", Control::Activity),
    ("gauge", Control::Gauge),
    ("disclosure", Control::Disclosure),
    ("text-field", Control::TextField),
    ("page-control", Control::PageIndicator),
    ("wheel", Control::Wheel),
    ("wheel-same", Control::WheelSame),
    ("time", Control::Time),
    ("time-12", Control::Time12),
    ("pop-up", Control::PopUp),
    ("color-well", Control::ColorWell),
    ("unavailable", Control::Unavailable),
    ("control-group", Control::Group),
    ("alert", Control::Alert),
    ("confirmation", Control::Confirmation),
];

impl Control {
    pub(crate) fn parse(name: &str) -> anyhow::Result<Self> {
        NAMES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, control)| *control)
            .ok_or_else(|| anyhow::anyhow!("unknown reference control: {name}"))
    }
}

#[composable]
pub(crate) fn LiquidControlReference(
    control: Control,
    backdrop: super::liquid_tab_reference::ReferenceBackdrop,
    dark: bool,
    initial: f32,
) {
    let activations = rememberMutableStateOf(|| 0usize);
    LiquidTheme(
        LiquidThemeSpec::system_with_accent(
            if dark {
                SchemeMode::Dark
            } else {
                SchemeMode::Light
            },
            Color::from_rgb_u8(0, if dark { 145 } else { 136 }, 255),
        ),
        move || {
            let insets = local_safe_area_insets().current();
            Box(
                super::liquid_tab_reference::reference_background(backdrop, dark),
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
                        move || ControlContent(control, initial, activations),
                    );
                    Text(
                        format!("Activations: {}", activations.get()),
                        Modifier::empty().offset(150.0, 800.0),
                        liquid_typography().caption1,
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
fn ControlContent(control: Control, initial: f32, activations: cranpose::MutableState<usize>) {
    match control {
        Control::Slider | Control::Toggle | Control::Segmented | Control::FilterChip => {
            ReferenceSelection(control, initial, activations);
        }
        Control::Stepper
        | Control::Progress
        | Control::Activity
        | Control::Gauge
        | Control::Disclosure
        | Control::TextField
        | Control::PageIndicator => ReferenceStandard(control, initial),
        Control::Wheel
        | Control::WheelSame
        | Control::Time
        | Control::Time12
        | Control::PopUp
        | Control::ColorWell => ReferencePicker(control, initial),
        Control::Unavailable | Control::Group | Control::Alert => {
            ReferenceContainer(control);
        }
        Control::Confirmation => ReferenceConfirmation(initial as usize),
        _ => ReferenceSurface(control, initial, activations),
    }
}

// The standard controls, in the states the native reference app sets from
// the same `REFERENCE_VALUE`.
#[composable]
fn ReferenceStandard(control: Control, initial: f32) {
    let value = rememberMutableStateOf(move || initial);
    let expanded = rememberMutableStateOf(move || initial == 1.0);
    let page = rememberMutableStateOf(move || initial as usize);
    match control {
        Control::Stepper => LiquidStepper(
            Modifier::empty(),
            value.get(),
            0.0..=1.0,
            0.1,
            move |next| value.set(next),
        ),
        Control::Progress => {
            LiquidLinearProgressIndicator(Modifier::empty().width(300.0), value.get());
        }
        Control::Activity => LiquidActivityIndicator(
            Modifier::empty(),
            spinner_size(initial),
            Some(liquid_colors().accent),
        ),
        Control::Gauge => LiquidGauge(Modifier::empty().width(300.0), value.get(), "Level"),
        Control::Disclosure => LiquidDisclosureGroup(
            Modifier::empty().width(300.0),
            "Details",
            expanded.get(),
            move |next| expanded.set(next),
            || {
                Text(
                    "More information",
                    Modifier::empty().fill_max_width(),
                    centered_body(),
                );
            },
        ),
        Control::TextField => ReferenceTextField(initial == 1.0),
        Control::PageIndicator => {
            Box(
                Modifier::empty().width(300.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    LiquidPageControl(Modifier::empty(), 5, page.get(), move |next| page.set(next));
                },
            );
        }
        _ => {}
    }
}

// The fruits the native wheel offers, and the time its time picker shows.
const FRUITS: [&str; 7] = [
    "Apple", "Banana", "Cherry", "Grape", "Lemon", "Mango", "Orange",
];
const SAME: [&str; 7] = ["Grape"; 7];
// The native time picker's frame on this simulator, on either clock.
const TIME_WIDTH: f32 = 320.0;

// The confirmation dialog points at a source where the native app puts one:
// 300 by 44 points in the centre for value 0, a 44 point square near the
// top-left corner for 1 and near the bottom-right corner for 2, 24 points in
// from the sides and 100 from the safe area's edges.
#[composable]
fn ReferenceConfirmation(placement: usize) {
    // The box around the control sits halfway between the insets already.
    let insets = local_safe_area_insets().current();
    let edge = 100.0 + (insets.top + insets.bottom) * 0.5;
    let (alignment, width, horizontal, vertical) = match placement {
        1 => (Alignment::TOP_START, 44.0, 24.0, edge),
        2 => (Alignment::BOTTOM_END, 44.0, 24.0, edge),
        _ => (Alignment::CENTER, 300.0, 0.0, 0.0),
    };
    Box(
        Modifier::empty()
            .fill_max_size()
            .padding_each(horizontal, vertical, horizontal, vertical),
        BoxSpec::default().content_alignment(alignment),
        move || {
            LiquidConfirmationDialog(
                Modifier::empty().width(width).height(44.0),
                true,
                || {},
                "Delete Photo?",
                "This photo will be deleted from all your devices.",
                |scope| {
                    scope.action("Delete", LiquidAlertRole::Destructive, || {});
                    scope.action("Cancel", LiquidAlertRole::Cancel, || {});
                },
                || {},
            );
        },
    );
}

// The containers of other controls, as the native app shows them.
#[composable]
fn ReferenceContainer(control: Control) {
    if control == Control::Alert {
        LiquidAlertDialog(
            || {},
            "Delete Photo?",
            "This photo will be deleted from all your devices.",
            |scope| {
                scope.action("Delete", LiquidAlertRole::Destructive, || {});
                scope.action("Cancel", LiquidAlertRole::Cancel, || {});
            },
        );
        return;
    }
    if control == Control::Group {
        LiquidControlGroup(Modifier::empty().width(300.0), |scope| {
            scope.button("Previous", || {});
            scope.button("Next", || {});
        });
        return;
    }
    LiquidContentUnavailableView(
        Modifier::empty().width(320.0),
        "No Photos",
        "Photos you take appear here.",
        || {
            // SwiftUI's borderless button: the label alone.
            let mut style = liquid_typography().subheadline;
            style.span_style.color = Some(liquid_colors().accent);
            Text("Take Photo", Modifier::empty().clickable(|_| {}), style);
        },
    );
}

// The pickers, at the native app's `REFERENCE_VALUE`.
#[composable]
fn ReferencePicker(control: Control, initial: f32) {
    let selected = rememberMutableStateOf(move || initial as usize);
    match control {
        Control::Time | Control::Time12 => {
            let is_24_hour = matches!(control, Control::Time);
            let state = rememberLiquidTimePickerState(10, 42, Some(is_24_hour));
            LiquidTimePicker(state, Modifier::empty().width(TIME_WIDTH));
        }
        Control::PopUp => LiquidPopUpButton(
            Modifier::empty(),
            "Fruit",
            &FRUITS,
            selected.get(),
            move |next| selected.set(next),
        ),
        Control::ColorWell => LiquidColorWell(
            Modifier::empty(),
            "Color",
            well_color(initial, liquid_colors().is_dark),
            || {},
        ),
        Control::WheelSame => {
            LiquidWheelPicker(Modifier::empty().width(300.0), "Fruit", &SAME, 3, |_| {});
        }
        _ => LiquidWheelPicker(
            Modifier::empty().width(300.0),
            "Fruit",
            &FRUITS,
            selected.get(),
            move |next| selected.set(next),
        ),
    }
}

// SwiftUI's `.blue` for a value of 0 and `.red` for 1, as the native color
// well shows them.
fn well_color(initial: f32, dark: bool) -> Color {
    match (initial > 0.5, dark) {
        (false, false) => Color::from_rgb_u8(0, 136, 255),
        (false, true) => Color::from_rgb_u8(0, 145, 255),
        (true, false) => Color::from_rgb_u8(255, 56, 60),
        (true, true) => Color::from_rgb_u8(255, 66, 69),
    }
}

// A value above 0.5 selects the large spinner, as the native app does.
fn spinner_size(initial: f32) -> LiquidActivityIndicatorSize {
    if initial > 0.5 {
        LiquidActivityIndicatorSize::Large
    } else {
        LiquidActivityIndicatorSize::Medium
    }
}

// A value of 1 fills the field, as the native app does.
#[composable]
fn ReferenceTextField(filled: bool) {
    let text = if filled { "Ada" } else { "" };
    let state = cranpose::remember(|| cranpose_foundation::text::TextFieldState::new(text))
        .with(|state| *state);
    LiquidTextField(Modifier::empty().width(300.0), state, "Name");
}

fn centered_body() -> cranpose::text::TextStyle {
    let mut style = liquid_typography().body;
    style.span_style.color = Some(liquid_colors().label);
    style.paragraph_style.text_align = cranpose::text::TextAlign::Center;
    style
}

#[composable]
fn ReferenceSelection(control: Control, initial: f32, activations: cranpose::MutableState<usize>) {
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
        Control::FilterChip => LiquidChip(
            Modifier::empty(),
            checked.get(),
            move || {
                checked.set(!checked.get());
                activations.set(activations.get() + 1);
            },
            "Unread".into(),
        ),
        _ => unreachable!("selection controls are dispatched by ControlContent"),
    }
}

#[composable]
fn ReferenceSurface(control: Control, initial: f32, activations: cranpose::MutableState<usize>) {
    match control {
        Control::Button | Control::ProminentButton => {
            let spec = if matches!(control, Control::ProminentButton) {
                GlassButtonSpec::prominent()
            } else {
                GlassButtonSpec::glass()
            };
            ReferenceButton(
                "Continue",
                spec.with_size(GlassButtonSize::Large),
                move || {
                    activations.set(activations.get() + 1);
                },
            );
        }
        Control::Chip => LiquidActionChip(
            Modifier::empty(),
            false,
            move || {
                activations.set(activations.get() + 1);
            },
            "Unread".into(),
        ),
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
            move || activations.set(activations.get() + 1),
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
        GlassButtonLabel(label.into(), spec.clone());
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
