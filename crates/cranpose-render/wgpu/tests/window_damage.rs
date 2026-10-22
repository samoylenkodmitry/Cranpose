//! A frame displayed through the renderer's own composition target, into a
//! swapchain image that holds a frame drawn before, or into an image that
//! comes cleared through the renderer's copy of the frame, draws only the
//! window pixels that changed since that frame, and equals the frame drawn
//! whole byte for byte. One small thing changes per frame, beside and under glass,
//! through layers, effects and shadows, over a page of still cards; the
//! damage each frame reports holds every pixel the whole frames change, and
//! a frame that moves the whole page draws the whole window.

use std::{cell::RefCell, rc::Rc, sync::MutexGuard};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_liquid::{Glass, GlassSurface};
use cranpose_render_common::{
    Renderer,
    graph::{DrawCommandId, DrawRunNode, PrimitivePhase, RenderGraph, RenderNode},
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::{FrameDamage, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Box, BoxSpec, Color, Column, ColumnSpec, ContentScale, GraphicsLayer, Image,
    LayerShape, Modifier, RenderEffect, Row, RowSpec, Text, TextStyle, composable,
    text::{SpanStyle, TextUnit},
};
use cranpose_ui_graphics::{
    Brush, CornerRadii, Dp, DrawPrimitive, DrawScope, DrawStyle, ImageBitmap, Path, Point, Rect,
    RoundedCornerShape, Stroke,
};

use crate::support::{self, rect_modifier};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 400;
const STEPS: u32 = 6;
/// Device pixels per logical one: whole, and a fraction that leaves edges
/// between pixels.
const SCALES: [f32; 2] = [1.0, 1.5];
/// Where the changing thing sits: over the cards and under the glass.
const SPOT: [f32; 4] = [120.0, 110.0, 24.0, 24.0];
const GLASS: [f32; 4] = [60.0, 80.0, 200.0, 100.0];
const UPPER_GLASS: [f32; 4] = [140.0, 140.0, 140.0, 90.0];
/// A spinner beside the glass, over the cards.
const SPINNER: [f32; 4] = [180.0, 10.0, 24.0, 24.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    Shape,
    Arc,
    Gradient,
    Star,
    Label,
    Picture,
    TurnedLayer,
    FadedLayer,
    UnderBlur,
    UnderBlurUnderBlur,
    UnderLiquidGlass,
    BlurredContent,
    Shadow,
    Appearing,
    MovedPage,
    /// A spinner turning by drawing its arc again, beside two liquid glass
    /// panes.
    Spinner,
    /// A spinner turning by its layer, beside two liquid glass panes.
    TurnedSpinner,
    /// A shape changing colour, under a blur pane that shows at
    /// [`GLASS_STEP`] only.
    GlassOnce,
    /// A spinner turning by drawing its arc again over the background
    /// alone: a page that costs less drawn whole than copied.
    BareSpinner,
}

/// The frames a target draws whole before one can draw a part: the first,
/// with nothing to compare with; one the back-off skips; and the next,
/// summarized as the frame the one after compares with.
const START_FRAMES: u32 = 3;

/// The frames a swapchain draws whole before one draws a part: its images
/// first hold no frame, then the target's start frames, until the image of
/// the last of those comes back.
const SWAPCHAIN_START_FRAMES: u32 = 2 * SWAPCHAIN_IMAGES as u32 + START_FRAMES - 1;

/// The frames in a row that change a small part of the window before the
/// renderer keeps a copy of the frame for images that come cleared.
const COPY_AFTER: u32 = 3;

/// The frames images that come cleared take before they first summarize.
const COPY_HOLD_OFF: u32 = 64;

/// The frames images that come cleared take whole before one takes a part:
/// the frames before they summarize, the one summarized as the frame the
/// next compares with, then the small changes before the renderer keeps a
/// copy, the last of which it copies.
const CLEARED_START_FRAMES: u32 = COPY_HOLD_OFF + 1 + COPY_AFTER;

/// The step at which the pane of [`Scene::GlassOnce`] shows: after a
/// swapchain frame drew a part of the window.
const GLASS_STEP: u32 = SWAPCHAIN_START_FRAMES + 1;

fn spot() -> Modifier {
    rect_modifier(SPOT)
}

fn shade(step: u32) -> Color {
    Color(0.2 + 0.12 * (step % 6) as f32, 0.75, 0.3, 1.0)
}

fn arc(scope: &mut dyn DrawScope, start: f32) {
    let size = scope.size();
    scope.draw_arc(
        Brush::solid(Color(0.3, 0.8, 1.0, 1.0)),
        Point::new(size.width * 0.5, size.height * 0.5),
        9.0,
        start,
        270.0,
        Stroke::new(3.0),
    );
}

fn star(center: Point, outer: f32) -> Path {
    let mut star = Path::new();
    for point in 0..10 {
        let angle = point as f32 * std::f32::consts::PI / 5.0;
        let radius = if point % 2 == 0 { outer } else { outer * 0.4 };
        let at = Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        if point == 0 {
            star.move_to(at);
        } else {
            star.line_to(at);
        }
    }
    star.close();
    star
}

fn label_style(size: f32) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::WHITE),
        font_size: TextUnit::Sp(size),
        ..Default::default()
    })
}

thread_local! {
    static PICTURES: [ImageBitmap; 2] = [[230, 80, 40, 255], [40, 90, 230, 255]]
        .map(|rgba| ImageBitmap::from_rgba8(2, 2, rgba.repeat(4)).expect("a solid image"));
}

/// A card of the still page: a gradient round rect under a border, a star
/// path, a row of dots and a label.
#[composable]
fn Card(index: usize) {
    let hue = (index % 7) as f32 / 7.0;
    Box(
        Modifier::empty()
            .size_points(60.0, 52.0)
            .padding(2.0)
            .rounded_corners(8.0)
            .border(
                1.0,
                Color(1.0, 1.0, 1.0, 0.35),
                RoundedCornerShape::uniform(8.0),
            )
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient(vec![
                        Color(0.2 + hue * 0.6, 0.3, 0.8 - hue * 0.5, 1.0),
                        Color(0.1, 0.5 + hue * 0.3, 0.4, 1.0),
                    ]),
                    CornerRadii::uniform(8.0),
                );
                let center = Point::new(size.width * 0.5, size.height * 0.5);
                scope.draw_path(
                    &star(center, 12.0),
                    Brush::solid(Color(1.0, 0.9, 0.4, 0.8)),
                    DrawStyle::Fill,
                );
                for dot in 0..6 {
                    scope.draw_circle(
                        Brush::solid(Color(1.0, 1.0, 1.0, 0.5)),
                        Point::new(6.0 + dot as f32 * 7.0, size.height - 6.0),
                        2.0,
                    );
                }
            }),
        BoxSpec::default(),
        move || {
            Text(
                format!("#{index}"),
                Modifier::empty().padding(3.0),
                label_style(9.0),
            );
        },
    );
}

#[composable]
fn Cards(scene: Scene, step: MutableState<u32>) {
    let shift = if scene == Scene::MovedPage {
        step.get() as f32
    } else {
        0.0
    };
    Column(
        Modifier::empty().offset(4.0, 4.0 - shift),
        ColumnSpec::default(),
        || {
            for row in 0..7 {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    for column in 0..5 {
                        Card(row * 5 + column);
                    }
                });
            }
        },
    );
}

fn blur_pane(rect: [f32; 4]) -> Modifier {
    rect_modifier(rect)
        .backdrop_effect(RenderEffect::blur(6.0))
        .background(Color(1.0, 1.0, 1.0, 0.15))
        .rounded_corners(16.0)
}

/// An arc in `rect` drawn again `degrees` further a step.
fn arc_drawn_again(rect: [f32; 4], step: MutableState<u32>, degrees: f32) -> Modifier {
    rect_modifier(rect).draw_behind(move |scope| arc(scope, step.get() as f32 * degrees))
}

/// An arc in `rect` whose layer turns `degrees` a step.
fn arc_turned(rect: [f32; 4], step: MutableState<u32>, degrees: f32) -> Modifier {
    rect_modifier(rect)
        .graphics_layer(move || GraphicsLayer {
            rotation_z: step.get() as f32 * degrees,
            ..Default::default()
        })
        .draw_behind(|scope| arc(scope, 0.0))
}

/// A white box at the spot whose blurred shadow moves down a pixel a step.
fn shadowed(step: MutableState<u32>) -> Modifier {
    spot()
        .drop_shadow(
            LayerShape::Rounded(RoundedCornerShape::uniform(6.0)),
            move |shadow| {
                shadow.radius = 8.0;
                shadow.offset.y = (step.get() % 3) as f32;
                shadow.color = Color(0.0, 0.0, 0.0, 0.7);
            },
        )
        .background(Color::WHITE)
        .rounded_corners(6.0)
}

fn gradient(scope: &mut dyn DrawScope, step: u32) {
    scope.draw_round_rect(
        Brush::linear_gradient(vec![shade(step), Color(0.1, 0.2, 0.6, 1.0)]),
        CornerRadii::uniform(6.0),
    );
}

fn growing_star(scope: &mut dyn DrawScope, step: u32) {
    scope.draw_path(
        &star(Point::new(12.0, 12.0), 8.0 + (step % 3) as f32),
        Brush::solid(Color(1.0, 0.4, 0.4, 1.0)),
        DrawStyle::Fill,
    );
}

/// The modifier of the box `scene` changes at a step; `None` for a scene
/// that changes no box.
fn changing_box(scene: Scene, step: MutableState<u32>) -> Option<Modifier> {
    Some(match scene {
        Scene::Shape
        | Scene::UnderBlur
        | Scene::UnderBlurUnderBlur
        | Scene::UnderLiquidGlass
        | Scene::GlassOnce => spot().background(shade(step.get())),
        Scene::Arc => arc_drawn_again(SPOT, step, 30.0),
        Scene::Gradient => spot().draw_behind(move |scope| gradient(scope, step.get())),
        Scene::Star => spot().draw_behind(move |scope| growing_star(scope, step.get())),
        Scene::TurnedLayer => arc_turned(SPOT, step, 15.0),
        Scene::FadedLayer => spot().graphics_layer(|| GraphicsLayer {
            alpha: 0.6,
            ..Default::default()
        }),
        Scene::BlurredContent => spot().blur(Dp(3.0)),
        Scene::Shadow => shadowed(step),
        Scene::Appearing if step.get() % 2 == 1 => spot().background(Color(0.9, 0.2, 0.2, 1.0)),
        Scene::Spinner | Scene::BareSpinner => arc_drawn_again(SPINNER, step, 12.0),
        Scene::TurnedSpinner => arc_turned(SPINNER, step, 12.0),
        Scene::Label | Scene::Picture | Scene::Appearing | Scene::MovedPage => return None,
    })
}

#[composable]
fn Changing(scene: Scene, step: MutableState<u32>) {
    match scene {
        Scene::Label => {
            Text(
                format!("{}", step.get() * 7),
                rect_modifier([SPOT[0], SPOT[1], 60.0, 20.0]),
                label_style(14.0),
            );
        }
        Scene::Picture => {
            let picture = PICTURES.with(|pictures| pictures[(step.get() % 2) as usize].clone());
            Image(
                picture,
                None,
                spot(),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                None,
            );
        }
        _ => {
            let shaded = matches!(scene, Scene::FadedLayer | Scene::BlurredContent);
            if let Some(modifier) = changing_box(scene, step) {
                Box(modifier, BoxSpec::default(), move || {
                    if shaded {
                        Box(
                            Modifier::empty()
                                .size_points(SPOT[2], SPOT[3])
                                .background(shade(step.get())),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            }
        }
    }
}

#[composable]
fn Page(scene: Scene, step: MutableState<u32>) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.06, 0.06, 0.1, 1.0)),
        BoxSpec::default(),
        move || {
            if scene != Scene::BareSpinner {
                Cards(scene, step);
            }
            Changing(scene, step);
            match scene {
                Scene::UnderBlur => {
                    Box(blur_pane(GLASS), BoxSpec::default(), || {});
                }
                Scene::GlassOnce if step.get() == GLASS_STEP => {
                    Box(blur_pane(GLASS), BoxSpec::default(), || {});
                }
                Scene::UnderBlurUnderBlur => {
                    Box(blur_pane(GLASS), BoxSpec::default(), || {});
                    Box(blur_pane(UPPER_GLASS), BoxSpec::default(), || {});
                }
                Scene::UnderLiquidGlass => {
                    GlassSurface(rect_modifier(GLASS), Glass::regular(), || {});
                }
                Scene::Spinner | Scene::TurnedSpinner => {
                    GlassSurface(rect_modifier(GLASS), Glass::regular(), || {});
                    GlassSurface(rect_modifier(UPPER_GLASS), Glass::regular(), || {});
                }
                _ => {}
            }
        },
    );
}

/// An image the renderer can draw and copy from, but not copy into: frames
/// compose in the renderer's own target, which keeps the frame before.
const COMPOSED: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::COPY_SRC);
/// An image the renderer draws straight into unless the frame reads the page
/// through glass.
const UNSAMPLED: wgpu::TextureUsages = COMPOSED.union(wgpu::TextureUsages::COPY_DST);
/// An image every frame draws straight into, whole.
const DIRECT: wgpu::TextureUsages = UNSAMPLED.union(wgpu::TextureUsages::TEXTURE_BINDING);
/// The images of a swapchain.
const SWAPCHAIN_IMAGES: usize = 3;

/// What a frame draws over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keeper {
    /// The renderer's own composition target, which keeps the frame before.
    Composition,
    /// The images of a swapchain of these usages in turn, each holding the
    /// frame drawn into it last, as a surface that keeps presented images
    /// hands them out.
    Swapchain(wgpu::TextureUsages),
    /// An image that comes cleared, as a WebGPU canvas hands them out: the
    /// renderer keeps the frame in its own target and copies it in.
    Cleared,
}

const KEEPERS: [Keeper; 3] = [
    Keeper::Composition,
    Keeper::Swapchain(DIRECT),
    Keeper::Cleared,
];

impl Keeper {
    /// The frames drawn whole before one draws a part.
    fn start_frames(self) -> u32 {
        match self {
            Keeper::Composition => START_FRAMES,
            Keeper::Swapchain(_) => SWAPCHAIN_START_FRAMES,
            Keeper::Cleared => CLEARED_START_FRAMES,
        }
    }
}

/// How a frame is drawn into an image.
#[derive(Clone, Copy)]
enum Draw {
    /// Into a texture the caller hands in.
    Untracked,
    /// Into the swapchain image of this index, holding the frame drawn
    /// into it last when the flag says so.
    Swapchain(u32, bool),
    /// Into an image that comes cleared.
    Cleared,
}

/// The colour an image that comes cleared holds before a frame draws into
/// it: a frame that leaves a pixel undrawn shows it.
const CLEARED_TO: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// A presentable image and its view.
struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// A page of a scene drawn as a window shows it, into presentable images.
struct Harness {
    shell: AppShell<WgpuRenderer>,
    step: Rc<RefCell<Option<MutableState<u32>>>>,
    keeper: Keeper,
    composed: Image,
    swapchain: [Image; SWAPCHAIN_IMAGES],
    /// The frames drawn into the swapchain.
    presented: usize,
    /// The frame, by its place among the frames drawn into the swapchain,
    /// whose image comes without the frame it held.
    unkept: Option<usize>,
    direct: Image,
}

/// A frame drawn over the frame its keeper holds, and whole.
struct Frame {
    /// The statistics of the frame drawn over the frame its keeper holds.
    stats: RenderStatsSnapshot,
    kept: Vec<u8>,
    direct: Vec<u8>,
}

impl Harness {
    /// The page of `scene` at `scale` device pixels per logical one.
    fn new(renderer: WgpuRenderer, scene: Scene, scale: f32, keeper: Keeper) -> Self {
        let device = renderer.try_device().expect("device").clone();
        let image = |usage| {
            let (texture, view) = support::render_target(
                &device,
                WIDTH,
                HEIGHT,
                wgpu::TextureFormat::Rgba8Unorm,
                usage,
            );
            Image { texture, view }
        };
        let swapchain_usage = match keeper {
            Keeper::Composition | Keeper::Cleared => DIRECT,
            Keeper::Swapchain(usage) => usage,
        };
        let swapchain = std::array::from_fn(|_| image(swapchain_usage));
        let (composed, direct) = (image(COMPOSED), image(DIRECT));
        let step: Rc<RefCell<Option<MutableState<u32>>>> = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&step);
        let mut shell = AppShell::new(renderer, location_key(file!(), line!(), column!()), {
            move || {
                let state = cranpose_core::rememberMutableStateOf(|| 0u32);
                *slot.borrow_mut() = Some(state);
                Page(scene, state);
            }
        });
        shell.set_density(scale);
        shell.set_buffer_size(WIDTH, HEIGHT);
        shell.set_viewport(WIDTH as f32 / scale, HEIGHT as f32 / scale);
        shell.update();
        Self {
            shell,
            step,
            keeper,
            composed,
            swapchain,
            presented: 0,
            unkept: None,
            direct,
        }
    }

    /// The frame of `step` drawn over the frame its keeper holds, and then
    /// into the direct image, whole.
    fn frame(&mut self, step: u32) -> Frame {
        let state = self
            .step
            .borrow()
            .as_ref()
            .copied()
            .expect("the page remembers its step");
        self.shell.debug_enter_app_context(|| state.set(step));
        self.shell.update();
        let (kept, stats) = match self.keeper {
            Keeper::Composition => draw(&mut self.shell, &self.composed, Draw::Untracked),
            Keeper::Swapchain(_) => {
                let index = self.presented % SWAPCHAIN_IMAGES;
                let kept =
                    self.presented >= SWAPCHAIN_IMAGES && self.unkept != Some(self.presented);
                self.presented += 1;
                let image = Draw::Swapchain(index as u32, kept);
                draw(&mut self.shell, &self.swapchain[index], image)
            }
            Keeper::Cleared => draw(&mut self.shell, &self.swapchain[0], Draw::Cleared),
        };
        let (direct, direct_stats) = draw(&mut self.shell, &self.direct, Draw::Untracked);
        assert_eq!(
            direct_stats.damage,
            FrameDamage::Untracked,
            "a frame drawn straight into the image keeps no frame"
        );
        Frame {
            stats,
            kept,
            direct,
        }
    }
}

/// Draws the shell's scene into `image` as `how` says: its pixels and the
/// frame's statistics.
fn draw(
    shell: &mut AppShell<WgpuRenderer>,
    image: &Image,
    how: Draw,
) -> (Vec<u8>, RenderStatsSnapshot) {
    let renderer = shell.renderer();
    let device = renderer.try_device().expect("device").clone();
    let queue = renderer.try_queue_for_tests().expect("queue").clone();
    match how {
        Draw::Untracked => renderer.render(&image.texture, &image.view, WIDTH, HEIGHT),
        Draw::Swapchain(index, kept) => {
            renderer.render_swapchain_image_for_tests(&image.texture, &image.view, index, kept)
        }
        Draw::Cleared => {
            clear(&device, &queue, image);
            renderer.render_cleared_image_for_tests(&image.texture, &image.view)
        }
    }
    .expect("the frame renders");
    assert_eq!(renderer.device_error_count_for_tests(), 0);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    (
        support::read_texture(&device, &queue, &image.texture),
        stats,
    )
}

/// Fills `image` with [`CLEARED_TO`], as a surface hands out an image that
/// holds none of the frames before.
fn clear(device: &wgpu::Device, queue: &wgpu::Queue, image: &Image) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cleared image"),
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("cleared image"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &image.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(CLEARED_TO),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    queue.submit([encoder.finish()]);
}

fn holds(damage: FrameDamage, x: usize, y: usize) -> bool {
    match damage {
        FrameDamage::Untracked => false,
        FrameDamage::Skipped | FrameDamage::Full => true,
        FrameDamage::Rect {
            x: left,
            y: top,
            width,
            height,
        } => {
            let (left, top) = (left as usize, top as usize);
            x >= left && y >= top && x < left + width as usize && y < top + height as usize
        }
    }
}

/// Asserts every pixel that differs between `before` and `after` lies
/// within `damage`.
fn assert_covers(label: &str, damage: FrameDamage, before: &[u8], after: &[u8]) {
    let outside: Vec<_> = support::differing_pixels(WIDTH, before, after)
        .into_iter()
        .filter(|&(x, y, _, _)| !holds(damage, x, y))
        .collect();
    assert!(
        outside.is_empty(),
        "{label}: {} changed pixels lie outside the damage {damage:?}: {}",
        outside.len(),
        support::describe_differing(&outside)
    );
}

fn around(rect: [f32; 4], margin: f32) -> [f32; 4] {
    [
        rect[0] - margin,
        rect[1] - margin,
        rect[2] + margin * 2.0,
        rect[3] + margin * 2.0,
    ]
}

impl Scene {
    /// The window pixels a step may change: the spot, what draws from it,
    /// and the panes over it.
    fn reach(self) -> [f32; 4] {
        match self {
            Scene::UnderBlur | Scene::UnderLiquidGlass => around(GLASS, 32.0),
            Scene::UnderBlurUnderBlur => {
                let right = (UPPER_GLASS[0] + UPPER_GLASS[2]).max(GLASS[0] + GLASS[2]);
                let bottom = (UPPER_GLASS[1] + UPPER_GLASS[3]).max(GLASS[1] + GLASS[3]);
                around(
                    [GLASS[0], GLASS[1], right - GLASS[0], bottom - GLASS[1]],
                    32.0,
                )
            }
            Scene::Label => around([SPOT[0], SPOT[1], 60.0, 20.0], 8.0),
            Scene::Shadow | Scene::BlurredContent => around(SPOT, 32.0),
            // A turned square reaches past its box by a fifth of its side.
            Scene::TurnedLayer => around(SPOT, 12.0),
            Scene::Spinner | Scene::TurnedSpinner => around(SPINNER, 12.0),
            _ => around(SPOT, 8.0),
        }
    }
}

/// Asserts `damage` is a rect within `reach`.
fn assert_within(label: &str, damage: FrameDamage, reach: [f32; 4]) {
    let FrameDamage::Rect {
        x,
        y,
        width,
        height,
    } = damage
    else {
        panic!("{label}: a small change must report a rect, not {damage:?}");
    };
    if width == 0 || height == 0 {
        return;
    }
    let [left, top, reach_width, reach_height] = reach;
    assert!(
        x as f32 >= left
            && y as f32 >= top
            && (x + width) as f32 <= left + reach_width
            && (y + height) as f32 <= top + reach_height,
        "{label}: the damage {damage:?} reaches past {reach:?}"
    );
}

/// Renders `scene` for [`STEPS`] changes after a still frame and checks
/// each frame's damage against the pixels it changed.
fn assert_damage_holds_the_changes(scene: Scene) {
    for keeper in KEEPERS {
        for scale in SCALES {
            assert_damage_holds_the_changes_at(scene, scale, keeper);
        }
    }
}

/// A headless renderer whose display format is the composition format, so
/// an image of [`DIRECT`] usages takes frames straight, or `None` without a
/// GPU.
fn renderer() -> Option<(MutexGuard<'static, ()>, WgpuRenderer)> {
    match support::headless_renderer_parts_with_display_format(wgpu::TextureFormat::Rgba8Unorm) {
        Ok(parts) => Some(parts),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

/// Draws each frame of `scene` twice: over the frame `keeper` holds, which
/// draws its damage, and straight into an image that takes the whole frame.
/// The first must equal the second byte for byte, and what the second
/// changes must lie in the damage the first reports.
fn assert_damage_holds_the_changes_at(scene: Scene, scale: f32, keeper: Keeper) {
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, scene, scale, keeper);
    let reach = scene.reach().map(|value| value * scale);
    let whole = u64::from(WIDTH * HEIGHT);
    let mut before: Option<Vec<u8>> = None;
    let (mut partial_frames, mut still_compared) = (0, 0);
    // Still frames until a swapchain's frame compares with the one before,
    // or until images that come cleared take frames from the copy.
    let still = match keeper {
        Keeper::Cleared => CLEARED_START_FRAMES as usize + 1,
        _ => SWAPCHAIN_IMAGES + START_FRAMES as usize + 1,
    };
    for step in std::iter::repeat_n(0, still).chain(1..=STEPS) {
        let frame = harness.frame(step);
        let damage = frame.stats.damage;
        let repainted = frame.stats.repainted_pixels;
        let label = format!("{scene:?} {keeper:?} at {scale}, step {step}, damage {damage:?}");
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        if damage == FrameDamage::Untracked {
            // A swapchain image that holds no frame takes the frame whole.
            assert_eq!(repainted, whole, "{label}");
            before = None;
            continue;
        }
        if let Some(before) = &before {
            assert_covers(&label, damage, before, &frame.direct);
        }
        let damaged = match damage {
            FrameDamage::Rect { width, height, .. } => u64::from(width * height),
            _ => whole,
        };
        match keeper {
            Keeper::Composition => assert_eq!(repainted, damaged, "{label}"),
            // An image holds a frame a few frames back, so it draws the
            // damages of the frames since too.
            Keeper::Swapchain(_) => assert!(repainted >= damaged, "{label}: {repainted} pixels"),
            // Drawn straight into the image, whole, until the renderer keeps
            // a copy.
            Keeper::Cleared => assert!(
                repainted == damaged || repainted == whole,
                "{label}: {repainted} pixels"
            ),
        }
        if before.is_some() {
            if step == 0 {
                // Until the target compares, a frame draws whole.
                if matches!(damage, FrameDamage::Rect { .. }) {
                    assert_eq!(
                        damage,
                        FrameDamage::Rect {
                            x: 0,
                            y: 0,
                            width: 0,
                            height: 0
                        },
                        "{label}: a frame that changes nothing reports no damage"
                    );
                    still_compared += 1;
                }
            } else if scene == Scene::MovedPage {
                assert!(
                    matches!(damage, FrameDamage::Full | FrameDamage::Skipped),
                    "{label}: a frame moving the page changes most of the window"
                );
            } else {
                assert_within(&label, damage, reach);
                partial_frames += usize::from((1..whole).contains(&repainted));
            }
        }
        before = Some(frame.direct);
    }
    assert!(
        scene == Scene::MovedPage || partial_frames > 0,
        "{scene:?} {keeper:?} at {scale}: no frame drew a part of the window"
    );
    assert!(
        still_compared > 0,
        "{scene:?} {keeper:?} at {scale}: no still frame compared with the frame before"
    );
}

#[test]
fn a_shape_changing_colour() {
    assert_damage_holds_the_changes(Scene::Shape);
}

#[test]
fn an_arc_drawn_at_a_new_angle() {
    assert_damage_holds_the_changes(Scene::Arc);
}

#[test]
fn a_gradient_changing_colour() {
    assert_damage_holds_the_changes(Scene::Gradient);
}

#[test]
fn a_path_changing_size() {
    assert_damage_holds_the_changes(Scene::Star);
}

#[test]
fn a_label_changing_text() {
    assert_damage_holds_the_changes(Scene::Label);
}

#[test]
fn an_image_changing_bitmap() {
    assert_damage_holds_the_changes(Scene::Picture);
}

#[test]
fn a_layer_turning() {
    assert_damage_holds_the_changes(Scene::TurnedLayer);
}

#[test]
fn a_translucent_layer_changing_content() {
    assert_damage_holds_the_changes(Scene::FadedLayer);
}

#[test]
fn a_shape_changing_under_a_blur_pane() {
    assert_damage_holds_the_changes(Scene::UnderBlur);
}

#[test]
fn a_shape_changing_under_a_blur_pane_another_pane_reads() {
    assert_damage_holds_the_changes(Scene::UnderBlurUnderBlur);
}

#[test]
fn a_shape_changing_under_liquid_glass() {
    assert_damage_holds_the_changes(Scene::UnderLiquidGlass);
}

#[test]
fn blurred_content_changing() {
    assert_damage_holds_the_changes(Scene::BlurredContent);
}

#[test]
fn a_blurred_shadow_moving() {
    assert_damage_holds_the_changes(Scene::Shadow);
}

#[test]
fn a_shape_appearing_and_going() {
    assert_damage_holds_the_changes(Scene::Appearing);
}

#[test]
fn the_whole_page_moving_one_pixel() {
    assert_damage_holds_the_changes(Scene::MovedPage);
}

/// The frames of a spinner that turns for as long as it shows, beside two
/// liquid glass panes over a page of cards: once its target holds a frame,
/// each draws only the spinner's part of the window, with a few draws and
/// no glass, and equals the whole frame.
fn assert_spinner_repaints_only_itself(scene: Scene) {
    for keeper in KEEPERS {
        assert_spinner_repaints_only_itself_over(scene, keeper);
    }
}

fn assert_spinner_repaints_only_itself_over(scene: Scene, keeper: Keeper) {
    const PARTIAL_FRAMES: u32 = 50;
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, scene, 1.0, keeper);
    let whole_frames = keeper.start_frames();
    let spinner_pixels = (SPINNER[2] * SPINNER[3]) as u64;
    let mut whole_draws = 0;
    for step in 0..whole_frames + PARTIAL_FRAMES {
        let frame = harness.frame(step);
        let stats = frame.stats;
        let label = format!("{scene:?} {keeper:?} frame {step}: {stats:?}");
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        if step < whole_frames {
            whole_draws = stats.draw_calls;
            continue;
        }
        assert!(
            stats.repainted_pixels <= 4 * spinner_pixels,
            "{label}: the frame drew {} pixels again for a spinner of {spinner_pixels}",
            stats.repainted_pixels
        );
        assert!(
            stats.draw_calls * 10 < whole_draws,
            "{label}: {} draws against {whole_draws} for the whole window",
            stats.draw_calls
        );
        assert_eq!(
            (stats.shader_pixels, stats.blur_pixels),
            (0, 0),
            "{label}: the glass beside the spinner must not shade again"
        );
    }
}

#[test]
fn a_small_endless_animation_drawn_again_repaints_only_its_area() {
    assert_spinner_repaints_only_itself(Scene::Spinner);
}

#[test]
fn a_small_endless_animation_turned_by_its_layer_repaints_only_its_area() {
    assert_spinner_repaints_only_itself(Scene::TurnedSpinner);
}

/// A page that moves for long enough that its target stops summarizing for
/// a while, and lets its summaries go, and then holds still: once it
/// summarizes again a frame reports no damage, and every frame equals the
/// whole frame.
#[test]
fn a_page_holding_still_after_moving_for_long_reports_its_damage_again() {
    const MOVING: u32 = 40;
    const STILL: u32 = 300;
    for keeper in KEEPERS {
        let Some((_lock, renderer)) = renderer() else {
            return;
        };
        let mut harness = Harness::new(renderer, Scene::MovedPage, 1.0, keeper);
        let mut still_damage = None;
        for index in 0..MOVING + STILL {
            let step = index.min(MOVING);
            let frame = harness.frame(step);
            let label = format!("{keeper:?} frame {index}: {:?}", frame.stats);
            support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
            if index > MOVING && matches!(frame.stats.damage, FrameDamage::Rect { .. }) {
                still_damage = Some(frame.stats.damage);
                break;
            }
        }
        assert_eq!(
            still_damage,
            Some(FrameDamage::Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0
            }),
            "{keeper:?}: a still frame after the page moved reports no damage"
        );
    }
}

/// A target that keeps losing the frame it kept, here to a screenshot
/// between its frames as to a stand-in drawn while a pipeline compiles,
/// summarizes less and less often, and every frame equals the whole frame.
#[test]
fn a_target_that_keeps_losing_its_frame_summarizes_less_and_less_often() {
    const FRAMES: u32 = 40;
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::Shape, 1.0, Keeper::Composition);
    let mut summarized = 0;
    for step in 0..FRAMES {
        let frame = harness.frame(step);
        let label = format!("frame {step}: {:?}", frame.stats.damage);
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        summarized += u32::from(frame.stats.damage != FrameDamage::Skipped);
        harness
            .shell
            .renderer()
            .capture_frame(WIDTH, HEIGHT)
            .expect("the screenshot renders");
    }
    assert!(
        summarized * 4 <= FRAMES,
        "{summarized} of {FRAMES} frames summarized"
    );
}

/// Whether the frame copied a whole window, between an image that comes
/// cleared and the renderer's copy of the frame.
fn copies_a_window(stats: &RenderStatsSnapshot) -> bool {
    stats.copy_pixels >= u64::from(WIDTH * HEIGHT)
}

/// The bytes of the renderer's copy of a window.
const COPY_BYTES: u64 = WIDTH as u64 * HEIGHT as u64 * 4;

/// Draws still frames into images that come cleared until the renderer
/// keeps a copy and a frame draws nothing over it: the pooled bytes then.
fn keep_a_copy(harness: &mut Harness) -> u64 {
    let mut pool = 0;
    for index in 0..=CLEARED_START_FRAMES {
        let frame = harness.frame(0);
        let stats = frame.stats;
        let label = format!("still frame {index}: {stats:?}");
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        assert_eq!(
            copies_a_window(&stats),
            index + 1 >= CLEARED_START_FRAMES,
            "{label}: the copy starts after {COPY_AFTER} small changes"
        );
        pool = stats.offscreen_pool_bytes;
    }
    pool
}

/// An image that comes cleared takes the frame from the renderer's copy
/// once a few frames in a row changed a small part of the window; the first
/// frame that moves the whole page draws straight into the image again, and
/// the copy's window of memory goes.
#[test]
fn an_image_that_comes_cleared_lets_the_copy_go_when_the_whole_window_changes() {
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::MovedPage, 1.0, Keeper::Cleared);
    let pool = keep_a_copy(&mut harness);
    let frame = harness.frame(1);
    let stats = frame.stats;
    let label = format!("the page moved: {stats:?}");
    support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
    assert_eq!(stats.repainted_pixels, u64::from(WIDTH * HEIGHT), "{label}");
    assert!(!copies_a_window(&stats), "{label}: drawn straight in");
    assert_eq!(stats.offscreen_pool_bytes, pool - COPY_BYTES, "{label}");
}

/// The renderer lets go of the copy it keeps for images that come cleared
/// when the runtime says the app holds still: the next frames draw whole
/// straight into their images, and a few small changes later the renderer
/// keeps a copy again.
#[test]
fn an_image_that_comes_cleared_lets_the_copy_go_while_the_app_holds_still() {
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::Shape, 1.0, Keeper::Cleared);
    let pool = keep_a_copy(&mut harness);
    harness.shell.renderer().release_kept_copy();
    for index in 0..=COPY_AFTER {
        let frame = harness.frame(0);
        let stats = frame.stats;
        let label = format!("frame {index} after the app held still: {stats:?}");
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        let whole = index < COPY_AFTER;
        let expected = if whole { u64::from(WIDTH * HEIGHT) } else { 0 };
        assert_eq!(stats.repainted_pixels, expected, "{label}");
        assert_eq!(copies_a_window(&stats), index + 1 >= COPY_AFTER, "{label}");
        if index + 1 < COPY_AFTER {
            assert_eq!(stats.offscreen_pool_bytes, pool - COPY_BYTES, "{label}");
        }
    }
}

/// A page that costs less drawn whole than copied, a spinner over the
/// background alone, draws straight into images that come cleared, keeps no
/// copy, and summarizes seldom: not in its first frames, and then two
/// frames, the one compared with and the one comparing, after each skip.
#[test]
fn a_light_page_draws_whole_into_images_that_come_cleared() {
    const FRAMES: u32 = 3 * CLEARED_START_FRAMES;
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::BareSpinner, 1.0, Keeper::Cleared);
    let mut summarized = 0;
    for step in 0..FRAMES {
        let frame = harness.frame(step);
        let stats = frame.stats;
        let label = format!("frame {step}: {stats:?}");
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        assert_eq!(stats.repainted_pixels, u64::from(WIDTH * HEIGHT), "{label}");
        assert!(!copies_a_window(&stats), "{label}: no copy");
        if step < COPY_HOLD_OFF {
            assert_eq!(stats.damage, FrameDamage::Skipped, "{label}: held off");
        }
        summarized += u32::from(stats.damage != FrameDamage::Skipped);
    }
    assert!(
        summarized <= 4,
        "{summarized} of {FRAMES} frames summarized"
    );
}

/// Once every image of a swapchain holds the frame, a frame that changes
/// nothing draws nothing.
#[test]
fn a_swapchain_image_that_holds_the_frame_draws_nothing() {
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::Shape, 1.0, Keeper::Swapchain(DIRECT));
    let still = (0..3 * SWAPCHAIN_IMAGES).map(|_| 2);
    let mut last = None;
    for step in (0..2).chain(still) {
        let frame = harness.frame(step);
        support::assert_same_bytes(&format!("step {step}"), WIDTH, &frame.kept, &frame.direct);
        last = Some(frame.stats);
    }
    let stats = last.expect("frames were drawn");
    assert_eq!(
        (stats.repainted_pixels, stats.draw_calls),
        (0, 0),
        "a still frame into an image that holds it: {stats:?}"
    );
}

/// A swapchain whose images cannot be read as textures takes the frame with
/// glass through the composition target, which copies it into its image.
#[test]
fn a_frame_copied_into_a_swapchain_image_between_frames_drawn_into_it() {
    assert_swapchain_frames_after_the_glass(UNSAMPLED, true);
}

/// The swapchain hands the image for the frame with glass out without the
/// frame it held, as after a failed present.
#[test]
fn a_swapchain_image_handed_out_without_its_frame() {
    assert_swapchain_frames_after_the_glass(DIRECT, false);
}

/// Draws [`Scene::GlassOnce`] into a swapchain of `usage` whose image for the
/// frame with glass holds the frame drawn into it before only when
/// `glass_image_kept`. No frame after the glass draws over that image as if
/// it held the frame drawn into it before the glass, and once the images
/// hold frames the history follows again, frames draw a part of the window
/// again.
fn assert_swapchain_frames_after_the_glass(usage: wgpu::TextureUsages, glass_image_kept: bool) {
    let Some((_lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, Scene::GlassOnce, 1.0, Keeper::Swapchain(usage));
    if !glass_image_kept {
        harness.unkept = Some(GLASS_STEP as usize);
    }
    let whole = u64::from(WIDTH * HEIGHT);
    let (mut partial_before, mut partial_after) = (0, 0);
    for step in 0..=GLASS_STEP + 2 * SWAPCHAIN_IMAGES as u32 {
        let frame = harness.frame(step);
        let label = format!("step {step}: {:?}", frame.stats);
        support::assert_same_bytes(&label, WIDTH, &frame.kept, &frame.direct);
        let partial = usize::from((1..whole).contains(&frame.stats.repainted_pixels));
        match step.cmp(&GLASS_STEP) {
            std::cmp::Ordering::Less => partial_before += partial,
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Greater => partial_after += partial,
        }
    }
    assert!(
        partial_before > 0 && partial_after > 0,
        "frames draw a part of the window before the glass ({partial_before}) and after it \
         ({partial_after})"
    );
}

const OVERLAY: Rect = Rect {
    x: 200.0,
    y: 300.0,
    width: 60.0,
    height: 40.0,
};
const DOT: Rect = Rect {
    x: 20.0,
    y: 20.0,
    width: 10.0,
    height: 10.0,
};

fn rect_array(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn page_with_dot(dot: Color) -> RenderGraph {
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![
            support::solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                Color(0.1, 0.1, 0.2, 1.0),
            ),
            RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(DrawCommandId {
                    node_id: 5,
                    command_index: 0,
                    placement: DrawPlacement::Behind,
                }),
                vec![DrawPrimitive::Rect {
                    rect: DOT,
                    brush: Brush::solid(dot),
                    stroke: None,
                }],
            )),
        ],
    )
}

fn overlay_graph() -> RenderGraph {
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![support::solid_rect(OVERLAY, Color(0.9, 0.9, 0.2, 1.0))],
    )
}

/// The page with a dot of `dot` under `overlay`, displayed in the composed
/// image and then in the direct one: the composed frame's damage and the
/// two images' texels.
fn overlay_frame(
    renderer: &mut support::LockedRenderer,
    images: &[Image; 2],
    overlay: Option<RenderGraph>,
    dot: Color,
) -> (FrameDamage, Vec<u8>, Vec<u8>) {
    renderer.set_inspector_overlay(overlay);
    renderer.scene_mut().graph = Some(page_with_dot(dot));
    let [composed, direct] = images.each_ref().map(|image| {
        renderer
            .display_into(&image.texture, &image.view)
            .expect("the frame renders")
    });
    (composed.0.damage, composed.1, direct.1)
}

#[test]
fn an_overlay_appearing_and_going() {
    let mut renderer =
        match support::headless_renderer_with_display_format(wgpu::TextureFormat::Rgba8Unorm) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("skipping (headless WGPU init failed): {err}");
                return;
            }
        };
    let device = renderer.try_device().expect("device").clone();
    let images = [COMPOSED, DIRECT].map(|usage| {
        let (texture, view) = support::render_target(
            &device,
            WIDTH,
            HEIGHT,
            wgpu::TextureFormat::Rgba8Unorm,
            usage,
        );
        Image { texture, view }
    });
    let red = Color(0.9, 0.2, 0.2, 1.0);
    let mut before = Vec::new();
    for _ in 0..START_FRAMES {
        (_, _, before) = overlay_frame(&mut renderer, &images, None, red);
    }
    let overlay = around(rect_array(OVERLAY), 8.0);
    let dot = around(rect_array(DOT), 8.0);
    let frames = [
        (Some(overlay_graph()), red, overlay, "overlay shown"),
        (
            Some(overlay_graph()),
            Color::WHITE,
            dot,
            "page changed under it",
        ),
        (None, Color::WHITE, overlay, "overlay gone"),
    ];
    for (shown, colour, reach, label) in frames {
        let (damage, drawn, whole) = overlay_frame(&mut renderer, &images, shown, colour);
        assert_covers(label, damage, &before, &whole);
        support::assert_same_bytes(label, WIDTH, &drawn, &whole);
        assert_within(label, damage, reach);
        before = whole;
    }
    let (damage, drawn, whole) = overlay_frame(&mut renderer, &images, None, Color::WHITE);
    assert_eq!(
        damage,
        FrameDamage::Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0
        },
        "a frame that changes nothing reports no damage"
    );
    support::assert_same_bytes("still frame", WIDTH, &drawn, &whole);
}
