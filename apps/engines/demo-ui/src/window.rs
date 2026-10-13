use std::{
    cell::{Cell, OnceCell},
    rc::Rc,
};

use cranpose::{liquid::prelude::*, prelude::*};

use crate::{Controls, MUTED, SceneSliders, WALK_HINT, text_style};

#[derive(Clone, Copy, PartialEq)]
pub struct EngineInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub features: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
pub struct WindowStates {
    pub controls: MutableState<Controls>,
    pub frame: MutableState<Option<ImageBitmap>>,
    pub fps: MutableState<u32>,
}

#[derive(Clone, Default)]
pub struct TargetSize(pub Rc<Cell<(u32, u32)>>);

impl PartialEq for TargetSize {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[composable]
pub fn rememberWindowStates() -> WindowStates {
    remember(|| WindowStates {
        controls: mutableStateOf(Controls::default()),
        frame: mutableStateOf(None),
        fps: mutableStateOf(0),
    })
    .with(|states| *states)
}

#[derive(Clone, Default)]
pub struct WindowBridge {
    pub states: Rc<OnceCell<WindowStates>>,
    pub size: TargetSize,
}

impl PartialEq for WindowBridge {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.states, &other.states) && self.size == other.size
    }
}

#[composable]
pub fn HostedEngineWindow(engine: EngineInfo, bridge: WindowBridge) {
    let states = rememberWindowStates();
    bridge.states.get_or_init(|| states);
    EngineWindow(engine, states, bridge.size.clone());
}

#[composable]
pub fn EngineWindow(engine: EngineInfo, states: WindowStates, target_size: TargetSize) {
    LiquidTheme(crate::dark_theme(), move || {
        let target_size = target_size.clone();
        Row(
            Modifier::empty()
                .fill_max_size()
                .background(Color::from_rgb_u8(12, 14, 22))
                .padding(20.0),
            RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(20.0)),
            move || {
                ControlPanel(engine, states.controls, states.fps);
                Viewport(engine, states, target_size.clone());
            },
        );
    });
}

#[composable]
fn ControlPanel(engine: EngineInfo, controls: MutableState<Controls>, fps: MutableState<u32>) {
    LiquidCard(
        Modifier::empty().width(300.0).fill_max_height(),
        move || {
            Column(
                Modifier::empty().fill_max_size().padding(20.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
                move || {
                    Text(
                        format!("{} inside Cranpose", engine.name),
                        Modifier::empty(),
                        text_style(22.0, Color::WHITE),
                    );
                    Text(
                        engine.description,
                        Modifier::empty(),
                        text_style(13.0, MUTED),
                    );
                    SceneSliders(controls);
                    Spacer(Modifier::empty().weight(1.0));
                    Text(
                        format!("{} frames: {} per second", engine.name, fps.value()),
                        Modifier::empty(),
                        text_style(13.0, Color::from_rgb_u8(150, 220, 170)),
                    );
                    Text(
                        format!("Click the scene first. {WALK_HINT}"),
                        Modifier::empty(),
                        text_style(13.0, MUTED),
                    );
                },
            );
        },
    );
}

#[composable]
fn Viewport(engine: EngineInfo, states: WindowStates, target_size: TargetSize) {
    let WindowStates {
        controls, frame, ..
    } = states;
    let shape = RoundedCornerShape::uniform(28.0);
    let last = remember(|| Rc::new(Cell::new(None::<Point>))).with(Rc::clone);
    let focus = remember(FocusRequester::new).with(Clone::clone);
    let press_focus = focus.clone();
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .shadow_with(
                18.0,
                LayerShape::Rounded(shape),
                true,
                Color::BLACK,
                Color::BLACK,
            )
            .focus_requester(&focus)
            .focus_target()
            .on_key_event(move |event| walk_key(controls, event))
            .draw_behind(move |scope| {
                let size = scope.size();
                let density = current_density();
                target_size.0.set((
                    (size.width * density).round() as u32,
                    (size.height * density).round() as u32,
                ));
                if let Some(image) = frame.value() {
                    scope.draw_image_at_sampled(
                        Rect::from_size(size),
                        image,
                        1.0,
                        None,
                        ImageSampling::Linear,
                    );
                }
            })
            .pointer_input((), move |scope| {
                let last = Rc::clone(&last);
                let focus = press_focus.clone();
                async move {
                    scope
                        .await_pointer_event_scope(|events| async move {
                            loop {
                                let event = events.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        let _ = focus.request_focus();
                                        last.set(Some(event.position));
                                    }
                                    PointerEventKind::Move => {
                                        if let Some(previous) = last.get() {
                                            let (dx, dy) = (
                                                event.position.x - previous.x,
                                                event.position.y - previous.y,
                                            );
                                            controls.update(|c| c.look(dx, dy));
                                            last.set(Some(event.position));
                                            event.consume();
                                        }
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel => {
                                        last.set(None)
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::default(),
        move || {
            GlassSurface(
                Modifier::empty()
                    .align(Alignment::BOTTOM_START)
                    .padding(20.0)
                    .width(380.0),
                Glass::regular(),
                move || {
                    Column(
                        Modifier::empty().padding_symmetric(18.0, 14.0),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                        move || {
                            Text(
                                format!("Glass over a {} frame", engine.name),
                                Modifier::empty(),
                                text_style(17.0, Color::WHITE),
                            );
                            Text(
                                engine.features,
                                Modifier::empty(),
                                text_style(13.0, Color::from_rgb_u8(225, 230, 240)),
                            );
                        },
                    );
                },
            );
            GlassButton(
                Modifier::empty().align(Alignment::TOP_END).padding(20.0),
                GlassButtonSpec::glass(),
                move || controls.update(Controls::back_to_start),
                || {
                    Text(
                        "Back to start",
                        Modifier::empty(),
                        text_style(15.0, Color::WHITE),
                    );
                },
            );
        },
    );
}

fn walk_key(controls: MutableState<Controls>, event: &KeyEvent) -> bool {
    let mut walk = controls.value().walk;
    let handled = walk.key(event.key_code, event.event_type == KeyEventType::KeyDown);
    if handled {
        controls.update(|c| c.walk = walk);
    }
    handled
}
