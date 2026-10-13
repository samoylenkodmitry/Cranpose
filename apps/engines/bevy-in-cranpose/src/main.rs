use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bevy::log::{error, warn};
use cranpose::{liquid::prelude::*, prelude::*, text::TextUnit, *};
use cranpose_bevy::BevyView;
use demo_scene::Controls;

fn main() -> Result<(), LaunchError> {
    AppLauncher::new()
        .with_title("Bevy inside Cranpose")
        .with_size(1320, 820)
        .with_gpu_requirements(cranpose_bevy::gpu_requirements())
        .with_developer_inspector(false)
        .try_run(Root)
}

fn text_style(size: f32, color: Color) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(color),
        font_size: TextUnit::Sp(size),
        ..Default::default()
    })
}

#[composable]
fn Root() {
    let controls = rememberMutableStateOf(Controls::default);
    let frame = rememberMutableStateOf(|| None::<ImageBitmap>);
    let fps = rememberMutableStateOf(|| 0u32);
    let target_size = remember(|| Rc::new(Cell::new((0u32, 0u32)))).with(Rc::clone);
    let view = remember(|| Rc::new(RefCell::new(None::<BevyView>))).with(Rc::clone);
    let gpu = gpu_context();
    let running = gpu.is_some();
    let loop_size = Rc::clone(&target_size);
    let counter = remember(|| Rc::new(Cell::new((0u64, 0u32)))).with(Rc::clone);
    FrameEffect((), running, move |now| {
        let Some(gpu) = gpu.as_ref() else {
            return;
        };
        let (width, height) = loop_size.get();
        if width == 0 || height == 0 {
            return;
        }
        let mut slot = view.borrow_mut();
        let bevy = match slot.as_mut() {
            Some(bevy) => {
                if let Err(error) = bevy.resize(width, height) {
                    warn!("Bevy view resize: {error}");
                }
                bevy
            }
            None => match BevyView::new(gpu, width, height, demo_scene::build) {
                Ok(bevy) => slot.insert(bevy),
                Err(error) => {
                    error!("Bevy view: {error}");
                    return;
                }
            },
        };
        *bevy.world_mut().resource_mut::<Controls>() = controls.value();
        frame.set(Some(bevy.update()));
        let (since, frames) = counter.get();
        if now.saturating_sub(since) >= 500_000_000 {
            if since != 0 {
                fps.set((u64::from(frames) * 1_000_000_000 / now.saturating_sub(since)) as u32);
            }
            counter.set((now, 0));
        } else {
            counter.set((since, frames + 1));
        }
    });

    LiquidTheme(LiquidThemeSpec::default(), move || {
        let target_size = Rc::clone(&target_size);
        Row(
            Modifier::empty()
                .fill_max_size()
                .background(Color::from_rgb_u8(12, 14, 22))
                .padding(20.0),
            RowSpec::default().horizontal_arrangement(LinearArrangement::SpacedBy(20.0)),
            move || {
                ControlPanel(controls, fps);
                Viewport(controls, frame, Rc::clone(&target_size));
            },
        );
    });
}

#[composable]
fn ControlPanel(controls: MutableState<Controls>, fps: MutableState<u32>) {
    LiquidCard(
        Modifier::empty().width(300.0).fill_max_height(),
        move || {
            Column(
                Modifier::empty().fill_max_size().padding(20.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
                move || {
                    Text(
                        "Bevy inside Cranpose",
                        Modifier::empty(),
                        text_style(22.0, Color::WHITE),
                    );
                    Text(
                        "Bevy 0.20 renders this scene on Cranpose's wgpu device. Cranpose shows the frame as an image, clips it and draws glass over it. No pixels are copied.",
                        Modifier::empty(),
                        text_style(13.0, Color::from_rgb_u8(190, 196, 210)),
                    );
                    Labeled("Sun", move || {
                        LiquidSlider(
                            Modifier::empty().fill_max_width(),
                            controls.value().sun,
                            move |value| controls.update(|c| c.sun = value),
                        );
                    });
                    Labeled("Camera distance", move || {
                        LiquidSlider(
                            Modifier::empty().fill_max_width(),
                            (controls.value().distance - 6.0) / 18.0,
                            move |value| controls.update(|c| c.distance = 6.0 + value * 18.0),
                        );
                    });
                    Labeled("Bloom", move || {
                        LiquidSlider(
                            Modifier::empty().fill_max_width(),
                            controls.value().bloom / 0.8,
                            move |value| controls.update(|c| c.bloom = value * 0.8),
                        );
                    });
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                "Animate",
                                Modifier::empty().weight(1.0),
                                text_style(15.0, Color::WHITE),
                            );
                            LiquidToggle(Modifier::empty(), controls.value().spin, move |on| {
                                controls.update(|c| c.spin = on)
                            });
                        },
                    );
                    Spacer(Modifier::empty().weight(1.0));
                    Text(
                        format!("Bevy frames: {} per second", fps.value()),
                        Modifier::empty(),
                        text_style(13.0, Color::from_rgb_u8(150, 220, 170)),
                    );
                    Text(
                        "Drag the scene to orbit.",
                        Modifier::empty(),
                        text_style(13.0, Color::from_rgb_u8(190, 196, 210)),
                    );
                },
            );
        },
    );
}

#[composable]
fn Labeled(label: &'static str, content: impl FnMut() + 'static) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Text(label, Modifier::empty(), text_style(15.0, Color::WHITE));
            content();
        },
    );
}

#[composable]
fn Viewport(
    controls: MutableState<Controls>,
    frame: MutableState<Option<ImageBitmap>>,
    target_size: Rc<Cell<(u32, u32)>>,
) {
    let shape = RoundedCornerShape::uniform(28.0);
    let last = remember(|| Rc::new(Cell::new(None::<Point>))).with(Rc::clone);
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
            .draw_behind(move |scope| {
                let size = scope.size();
                let density = current_density();
                target_size.set((
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
                async move {
                    scope
                        .await_pointer_event_scope(|events| async move {
                            loop {
                                let event = events.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => last.set(Some(event.position)),
                                    PointerEventKind::Move => {
                                        if let Some(previous) = last.get() {
                                            let dx = event.position.x - previous.x;
                                            let dy = event.position.y - previous.y;
                                            controls.update(|c| {
                                                c.yaw += dx * 0.008;
                                                c.pitch = (c.pitch + dy * 0.006).clamp(0.05, 1.5);
                                            });
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
                    .width(360.0),
                Glass::regular(),
                || {
                    Column(
                        Modifier::empty().padding_symmetric(18.0, 14.0),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                        || {
                            Text(
                                "Glass over a Bevy frame",
                                Modifier::empty(),
                                text_style(17.0, Color::WHITE),
                            );
                            Text(
                                "PBR, cascaded shadows, point lights, HDR bloom",
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
                move || controls.set(Controls::default()),
                || {
                    Text(
                        "Reset view",
                        Modifier::empty(),
                        text_style(15.0, Color::WHITE),
                    );
                },
            );
        },
    );
}
