use cranpose_core::{mutableStateOf, remember, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, space::Scene3d, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Button,
    ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Point, PointerEventKind,
    PointerInputScope, Row, RowSpec, Size, Text,
};

use super::{
    eye_view::{EyeView, Side},
    page::{headset_views, immersive_entry},
    scene::{rememberSceneState, text_style, Meshes, Scene, SceneState},
};

const LOOK_PER_PIXEL: f32 = 0.004;
const FLAT_VERTICAL_FOV: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewMode {
    Flat,
    Stereo,
}

// Where the desktop viewer looks: radians left and up.
#[derive(Clone, Copy)]
struct Look {
    yaw: MutableState<f32>,
    pitch: MutableState<f32>,
}

#[composable]
pub(crate) fn XrTab() {
    let mode = rememberMutableStateOf(|| ViewMode::Flat);
    let look = remember(|| Look {
        yaw: mutableStateOf(0.0),
        pitch: mutableStateOf(0.0),
    })
    .with(|look| *look);
    let scene = rememberSceneState();
    let entry = immersive_entry();
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.03, 0.04, 0.07, 1.0)),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty().fill_max_width().padding(12.0),
                RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    ModeButton("Flat", mode.get() == ViewMode::Flat, move || {
                        mode.set(ViewMode::Flat);
                    });
                    ModeButton("Stereo", mode.get() == ViewMode::Stereo, move || {
                        mode.set(ViewMode::Stereo);
                    });
                    match entry {
                        Some(enter) => ModeButton("Enter VR", false, enter),
                        None => {
                            Text(
                            "For VR, open this page in a headset's browser. Drag to look around.",
                            Modifier::empty().padding(8.0),
                            text_style(14.0, Color(0.6, 0.65, 0.72, 1.0)),
                        );
                        }
                    }
                },
            );
            BoxWithConstraints(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .pointer_input(
                        (),
                        move |scope| async move { drag_to_look(scope, look).await },
                    ),
                move |scope| {
                    let c = scope.constraints();
                    let aspect = if c.max_width.is_finite()
                        && c.max_height.is_finite()
                        && c.max_height > 0.0
                    {
                        c.max_width / c.max_height
                    } else {
                        1.6
                    };
                    let (yaw, pitch) = (look.yaw.get(), look.pitch.get());
                    match mode.get() {
                        ViewMode::Flat => EyeBox(
                            EyeView::middle(yaw, pitch, FLAT_VERTICAL_FOV, aspect),
                            None,
                            scene,
                            Modifier::empty().fill_max_size(),
                        ),
                        ViewMode::Stereo => StereoViews(EyeView::pair(yaw, pitch), scene),
                    }
                },
            );
        },
    );
}

// The whole window while a headset shows the scene: both eyes side by side, which the page
// copies into the headset's layer. The page's XR session paces the frames. Each eye reads the
// headset's pose when it is drawn.
#[composable]
pub(crate) fn ImmersiveView() {
    let scene = rememberSceneState();
    Box(
        Modifier::empty().fill_max_size().background(Color::BLACK),
        BoxSpec::default(),
        move || {
            StereoViews(EyeView::pair(0.0, 0.0), scene);
        },
    );
}

#[composable]
fn ModeButton(label: &'static str, selected: bool, on_click: impl Fn() + 'static) {
    let background = if selected {
        Color(0.15, 0.38, 0.75, 1.0)
    } else {
        Color(0.18, 0.2, 0.25, 1.0)
    };
    Button(
        Modifier::empty()
            .rounded_corners(8.0)
            .background(background)
            .padding(8.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(label, Modifier::empty(), text_style(15.0, Color::WHITE));
        },
    );
}

async fn drag_to_look(scope: PointerInputScope, look: Look) {
    scope
        .await_pointer_event_scope(|events| async move {
            let mut last: Option<Point> = None;
            loop {
                let event = events.await_pointer_event().await;
                match event.kind {
                    PointerEventKind::Down if !event.is_consumed() => {
                        last = Some(event.global_position);
                    }
                    PointerEventKind::Move => {
                        let Some(previous) = last.filter(|_| !event.is_consumed()) else {
                            continue;
                        };
                        let now = event.global_position;
                        look.yaw
                            .update(|yaw| *yaw += (now.x - previous.x) * LOOK_PER_PIXEL);
                        look.pitch.update(|pitch| {
                            *pitch =
                                (*pitch + (now.y - previous.y) * LOOK_PER_PIXEL).clamp(-1.3, 1.3);
                        });
                        last = Some(now);
                    }
                    PointerEventKind::Up | PointerEventKind::Cancel => last = None,
                    _ => {}
                }
            }
        })
        .await;
}

// Both eyes side by side: the left eye in the left half.
#[composable]
fn StereoViews([left, right]: [EyeView; 2], scene: SceneState) {
    Row(
        Modifier::empty().fill_max_size(),
        RowSpec::default(),
        move || {
            for (view, side) in [(left, Side::Left), (right, Side::Right)] {
                EyeBox(
                    view,
                    Some(side),
                    scene,
                    Modifier::empty().fill_max_height().weight(1.0),
                );
            }
        },
    );
}

// One eye's view of the scene. An eye with a side takes the headset's eye of that side while the
// page sets them.
#[composable]
fn EyeBox(view: EyeView, side: Option<Side>, scene: SceneState, modifier: Modifier) {
    let meshes = remember(Meshes::new).with(Clone::clone);
    BoxWithConstraints(modifier, move |scope| {
        let c = scope.constraints();
        let finite = |v: f32| if v.is_finite() { v.max(1.0) } else { 1.0 };
        let size = Size {
            width: finite(c.max_width),
            height: finite(c.max_height),
        };
        let meshes = meshes.clone();
        Box(
            Modifier::empty().fill_max_size().scene3d(move || {
                // The page sets the newest pose just before the frame: reading it here, when
                // the scene is drawn, draws the scene again without a new composition.
                let eye = side
                    .and_then(|side| headset_views().map(|eyes| eyes[side as usize]))
                    .unwrap_or(view);
                Scene3d {
                    camera: eye.camera(eye.layout(size)),
                    light: None,
                    eye: None,
                }
            }),
            BoxSpec::default(),
            move || Scene(scene, meshes.clone()),
        );
    });
}
