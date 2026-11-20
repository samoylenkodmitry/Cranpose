// The panel to the right of the counter, turned toward the viewer. It counts the presses too and
// shows what the frame effect reads of the headset. It stands for what two hands do at once: a handle
// under it carries it while the other hand works on the counter panel, a pad counts the right clicks
// and the middle presses, and a list scrolls under a controller's thumbstick.

use cranpose::{PointerButton, PointerEventKind, PointerInputScope, prelude::*};
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    Hand, InputSource, SpatialPanel, SubspaceModifier, local_entitlement_check,
    local_headset_properties,
};
use player_demo::sized;

use crate::{headset_readout::Readout, ray_target::RayTarget};

// The panel's size in dp, which are millimeters.
pub(crate) const SIZE: [f32; 2] = [400.0, 820.0];

// The place of the panel in its subspace, which the scripted controllers read to find the handle
// and the pad.
pub(crate) fn modifier() -> SubspaceModifier {
    SubspaceModifier::new()
        .size(SIZE[0], SIZE[1])
        .offset(640.0, 0.0, 100.0)
        .rotate(0.0, -30.0, 0.0)
        .movable()
}

// The pad and the list stand at fixed places, so a scripted controller finds them without
// measuring the text around them: their middles, in dp from the panel's top left corner. They stand
// between two things that stop a ray before the panel: the pattern surface, which covers the panel's
// top 230 dp, and the torus's volume, which stands in front of the panel from 590 dp down.
const MARGIN: f32 = 24.0;
const PAD_HEIGHT: f32 = 60.0;
const LIST_HEIGHT: f32 = 90.0;
const PAD_TOP: f32 = 240.0;
const LIST_TOP: f32 = 310.0;
const ROWS: u32 = 40;
const ROW_HEIGHT: f32 = 32.0;

#[cfg_attr(
    not(all(feature = "android", target_os = "android")),
    expect(
        dead_code,
        reason = "the scripted controllers of an Android build read it"
    )
)]
pub(crate) const PAD_AT: [f32; 2] = [SIZE[0] / 2.0, PAD_TOP + PAD_HEIGHT / 2.0];
#[cfg_attr(
    not(all(feature = "android", target_os = "android")),
    expect(
        dead_code,
        reason = "the scripted controllers of an Android build read it"
    )
)]
pub(crate) const LIST_AT: [f32; 2] = [SIZE[0] / 2.0, LIST_TOP + LIST_HEIGHT / 2.0];

// The panel: a counter, the readout of the headset, the pad and the list.
#[composable]
pub(crate) fn PressesPanel(
    presses: MutableState<u32>,
    readout: MutableState<Readout>,
    target: RayTarget,
) {
    SpatialPanel(modifier(), move || {
        Box(
            Modifier::empty()
                .fill_max_size()
                .background(Color(0.16, 0.13, 0.1, 0.94))
                .rounded_corners(20.0),
            BoxSpec::default(),
            move || {
                Column(
                    Modifier::empty()
                        .offset(0.0, LIST_TOP + LIST_HEIGHT)
                        .padding(MARGIN),
                    ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                    move || {
                        Text(
                            format!("{} presses", presses.get()),
                            Modifier::empty(),
                            sized(28.0, Some(FontWeight::BOLD)),
                        );
                        ReadoutLines(readout, target);
                    },
                );
                ClickPad();
                ScrollList();
            },
        );
    });
}

// The lines of the headset's readout and the app's lifecycle as the headset reports it: resumed
// with input focus, paused under the system menu, stopped while the session does not run.
// Outside a headset the frame effect never runs. The readout stays at its start then.
#[composable]
fn ReadoutLines(readout: MutableState<Readout>, target: RayTarget) {
    let Readout {
        head_cm,
        trigger_percent,
        pinch_percent,
        controllers,
        pointers,
        frames_per_second,
    } = readout.get();
    let [left, right] = pointers.map(|pointer| match pointer {
        Some(InputSource::Controller) => "controller",
        Some(InputSource::Hands) => "hands",
        _ => "none",
    });
    let head = head_cm.map_or_else(|| "not tracked".to_owned(), |cm| format!("{cm} cm"));
    let (system, eye) = local_headset_properties().current().map_or_else(
        || (String::new(), String::new()),
        |headset| {
            let [width, height] = headset.eye_image_sizes[0].recommended;
            (headset.system_name, format!("Eye image {width}x{height}"))
        },
    );
    let [left_buttons, right_buttons] = [
        controllers[0].line(Hand::Left),
        controllers[1].line(Hand::Right),
    ];
    for line in [
        system,
        eye,
        format!("Head y {head}"),
        format!("Right trigger {trigger_percent}%"),
        format!("Right pinch {pinch_percent}%"),
        left_buttons,
        right_buttons,
        format!("Pointers L/R: {left} / {right}"),
        format!("Ray meets {}", target.text()),
        format!("Frame effect {frames_per_second} Hz"),
        format!("Lifecycle {:?}", local_lifecycle_state().current()),
    ] {
        Text(line, Modifier::empty(), sized(18.0, None));
    }
    if let Some(check) = local_entitlement_check().current() {
        Text(
            format!("Entitlement {:?}", check.entitlement()),
            Modifier::empty(),
            sized(18.0, None),
        );
    }
}

// How many times each pointer button went down on the pad.
#[derive(Clone, Copy, Default, PartialEq)]
struct Clicks {
    secondary: u32,
    middle: u32,
}

// A pad that counts the presses of the secondary and the middle button, which a controller makes
// with its first face button and with its thumbstick's click.
#[composable]
fn ClickPad() {
    let clicks = rememberMutableStateOf(Clicks::default);
    let Clicks { secondary, middle } = clicks.get();
    Box(
        Modifier::empty()
            .offset(MARGIN, PAD_TOP)
            .size_points(SIZE[0] - 2.0 * MARGIN, PAD_HEIGHT)
            .background(Color(0.3, 0.22, 0.16, 1.0))
            .rounded_corners(12.0)
            .pointer_input((), move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::ButtonDown(PointerButton::Secondary) => {
                                    clicks.update(|clicks| clicks.secondary += 1);
                                }
                                PointerEventKind::ButtonDown(PointerButton::Middle) => {
                                    clicks.update(|clicks| clicks.middle += 1);
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                format!("Right {secondary}, middle {middle}"),
                Modifier::empty(),
                sized(20.0, Some(FontWeight::BOLD)),
            );
        },
    );
}

// A list of rows that scrolls under a controller's thumbstick or a drag, with the distance that it
// scrolled.
#[composable]
fn ScrollList() {
    let scroll = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
    Column(
        Modifier::empty()
            .offset(MARGIN, LIST_TOP)
            .size_points(SIZE[0] - 2.0 * MARGIN, LIST_HEIGHT)
            .background(Color(0.1, 0.16, 0.2, 1.0))
            .rounded_corners(12.0)
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            for row in 1..=ROWS {
                Text(
                    format!("Row {row}, scrolled {} dp", scroll.value().round()),
                    Modifier::empty()
                        .height(ROW_HEIGHT)
                        .padding_symmetric(12.0, 4.0),
                    sized(20.0, None),
                );
            }
        },
    );
}
