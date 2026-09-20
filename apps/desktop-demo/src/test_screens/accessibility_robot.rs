use cranpose_core::{remember, rememberMutableStateOf};
use cranpose_foundation::{
    text::TextFieldState, LiveRegionMode, ProgressBarRangeInfo, SemanticsSetProgress,
    SemanticsWidgetRole,
};
use cranpose_ui::{
    composable, BasicTextField, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Row, RowSpec, Text, TextStyle,
};

#[allow(non_snake_case)]
#[composable]
pub fn AccessibilityRobotScreen() {
    let count = rememberMutableStateOf(|| 0i32);
    let volume = rememberMutableStateOf(|| 30.0f32);
    let notes = remember(|| TextFieldState::new("initial")).with(|state| *state);
    let password = remember(|| TextFieldState::new("robot-secret-value")).with(|state| *state);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color::WHITE)
            .padding(12.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Accessibility robot",
                Modifier::empty(),
                TextStyle::default(),
            );
            Row(
                Modifier::empty().merge_descendants(),
                RowSpec::default(),
                move || {
                    Text("Account", Modifier::empty(), TextStyle::default());
                    Text("Account", Modifier::empty(), TextStyle::default());
                    RobotButton("Remove", true, move || count.set(count.get() + 1));
                },
            );
            Row(Modifier::empty(), RowSpec::default(), move || {
                RobotButton("Increase", true, move || count.set(count.get() + 1));
                RobotButton("Decrease", true, move || count.set(count.get() - 1));
                RobotButton("Disabled action", false, move || {
                    count.set(count.get() + 100)
                });
            });
            Text(
                format!("Action count: {}", count.get()),
                Modifier::empty()
                    .semantics(|config| config.live_region = Some(LiveRegionMode::Polite)),
                TextStyle::default(),
            );
            BasicTextField(notes, field_modifier("Notes"), TextStyle::default());
            Text(
                format!("Edited: {}", notes.text()),
                Modifier::empty(),
                TextStyle::default(),
            );
            BasicTextField(
                password,
                field_modifier("Passphrase").password(),
                TextStyle::default(),
            );
            Text(
                "Loading",
                Modifier::empty().height(48.0).semantics(|config| {
                    config.role = Some(SemanticsWidgetRole::ProgressBar);
                    config.progress = Some(ProgressBarRangeInfo::new(40.0, 0.0, 100.0, 0));
                }),
                TextStyle::default(),
            );
            Text(
                "Volume",
                Modifier::empty().height(48.0).semantics(move |config| {
                    config.progress = Some(ProgressBarRangeInfo::new(volume.get(), 0.0, 100.0, 0));
                    config.set_progress = Some(SemanticsSetProgress::new(move |value| {
                        volume.set(value);
                        true
                    }));
                }),
                TextStyle::default(),
            );
            Text(
                format!("Volume value: {}", volume.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
            Text(
                "Decorative secret",
                Modifier::empty().hide_from_accessibility(),
                TextStyle::default(),
            );
        },
    );
}

fn field_modifier(name: &str) -> Modifier {
    Modifier::empty()
        .width(280.0)
        .height(48.0)
        .content_description(name)
}

#[allow(non_snake_case)]
#[composable]
fn RobotButton(label: &'static str, enabled: bool, action: impl FnMut() + 'static) {
    Button(
        Modifier::empty()
            .height(48.0)
            .padding_horizontal(8.0)
            .semantics(move |config| config.enabled = enabled),
        ButtonSpec::default(),
        action,
        move || {
            Text(label, Modifier::empty(), TextStyle::default());
        },
    );
}
