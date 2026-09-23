use cranpose_core::{remember, rememberMutableStateOf};
use cranpose_foundation::{
    text::TextFieldState, LiveRegionMode, ProgressBarRangeInfo, SemanticsSetProgress,
    SemanticsWidgetRole,
};
use cranpose_ui::{
    composable, BasicTextField, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Row, RowSpec, Text, TextStyle,
};

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
            Text("Accessibility robot", Modifier::empty(), robot_text_style());
            RobotNavigationControls();
            RobotOverlappingControls();
            Row(
                Modifier::empty().merge_descendants(),
                RowSpec::default(),
                move || {
                    Text("Account", Modifier::empty(), robot_text_style());
                    Text("Account", Modifier::empty(), robot_text_style());
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
                robot_text_style(),
            );
            BasicTextField(notes, field_modifier("Notes"), robot_text_style());
            Text(
                format!("Edited: {}", notes.text()),
                Modifier::empty(),
                robot_text_style(),
            );
            BasicTextField(
                password,
                field_modifier("Passphrase").password(),
                robot_text_style(),
            );
            Text(
                "Loading",
                Modifier::empty().height(48.0).semantics(|config| {
                    config.role = Some(SemanticsWidgetRole::ProgressBar);
                    config.progress = Some(ProgressBarRangeInfo::new(40.0, 0.0, 100.0, 0));
                }),
                robot_text_style(),
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
                robot_text_style(),
            );
            Text(
                format!("Volume value: {}", volume.get()),
                Modifier::empty(),
                robot_text_style(),
            );
            Text(
                "Decorative secret",
                Modifier::empty().hide_from_accessibility(),
                robot_text_style(),
            );
        },
    );
}

#[composable]
fn RobotDialog(
    title: &'static str,
    close_label: &'static str,
    visible: cranpose_core::MutableState<bool>,
    content: impl Fn() + 'static,
) {
    let content = std::rc::Rc::new(content);
    let notes = remember(|| TextFieldState::new("")).with(|state| *state);
    cranpose_ui::widgets::dialog::Dialog(
        cranpose_ui::widgets::dialog::DialogSpec::default(),
        move |_| visible.set(false),
        move || {
            let content = content.clone();
            Column(
                Modifier::empty()
                    .width(280.0)
                    .background(Color::WHITE)
                    .padding(16.0),
                ColumnSpec::default(),
                move || {
                    Text(title, Modifier::empty(), robot_text_style());
                    content();
                    BasicTextField(notes, field_modifier("Dialog notes"), robot_text_style());
                    RobotButton(close_label, true, move || visible.set(false));
                },
            );
        },
    );
}

#[composable]
fn RobotOverlappingControls() {
    let count = rememberMutableStateOf(|| 0i32);
    Row(Modifier::empty(), RowSpec::default(), move || {
        cranpose_ui::Box(
            Modifier::empty().size(cranpose_ui::Size::new(160.0, 48.0)),
            cranpose_ui::BoxSpec::default(),
            move || {
                for (label, increment) in [("Rear action", 1), ("Front action", 10)] {
                    Text(
                        label,
                        Modifier::empty()
                            .size(cranpose_ui::Size::new(160.0, 48.0))
                            .clickable(move |_| count.set(count.get() + increment)),
                        robot_text_style(),
                    );
                }
            },
        );
        Text(
            format!("Overlap count: {}", count.get()),
            Modifier::empty(),
            robot_text_style(),
        );
    });
}

fn robot_text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(Color::BLACK);
    style
}

fn field_modifier(name: &str) -> Modifier {
    Modifier::empty()
        .width(280.0)
        .height(48.0)
        .content_description(name)
}

#[composable]
fn RobotButton(label: &'static str, enabled: bool, action: impl FnMut() + 'static) {
    Button(
        Modifier::empty()
            .height(48.0)
            .padding_horizontal(8.0)
            .semantics(move |config| {
                config.enabled = enabled;
                config.state_description = (!enabled).then(|| "Disabled".to_owned());
            }),
        ButtonSpec::default(),
        action,
        move || {
            Text(label, Modifier::empty(), robot_text_style());
        },
    );
}

#[composable]
fn RobotNavigationControls() {
    let delivery = rememberMutableStateOf(|| 1usize);
    let preferences = rememberMutableStateOf(|| false);
    let confirmation = rememberMutableStateOf(|| false);
    Row(
        Modifier::empty().selectable_group(),
        RowSpec::default(),
        move || {
            for (index, label) in ["Standard", "Express", "Collection"]
                .into_iter()
                .enumerate()
            {
                Text(
                    label,
                    Modifier::empty()
                        .size(cranpose_ui::Size::new(100.0, 48.0))
                        .selectable(
                            delivery.get() == index,
                            Some(SemanticsWidgetRole::RadioButton),
                            move || delivery.set(index),
                        ),
                    robot_text_style(),
                );
            }
        },
    );
    RobotButton("Open preferences", true, move || preferences.set(true));
    if preferences.get() {
        RobotDialog("Preferences", "Close preferences", preferences, move || {
            RobotButton("Open confirmation", true, move || confirmation.set(true));
            if confirmation.get() {
                RobotDialog("Confirmation", "Close confirmation", confirmation, || {});
            }
        });
    }
}
