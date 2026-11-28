// A label's blurred shadow blurs once: the frames after it draw the kept
// blur while the page under the label changes, as a keyboard's labels sit
// over a live background.

use std::{cell::RefCell, rc::Rc, sync::MutexGuard};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, mutableStateOf};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Point,
    text::{Shadow, SpanStyle, TextUnit},
};
use support::page::*;

use crate::support;

const FRAME: u32 = 160;

fn page_color(shade: i32) -> Color {
    if shade == 0 {
        Color(0.85, 0.85, 0.9, 1.0)
    } else {
        Color(0.6, 0.75, 0.85, 1.0)
    }
}

// A black label with a shadow blurred `blur` pixels, as dark as `tint`
// makes it and moved `drop` pixels down and right past the glyphs.
fn shadowed(blur: f32, tint: i32, drop: f32) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color::BLACK),
        font_size: TextUnit::Sp(32.0),
        shadow: Some(Shadow {
            color: Color(0.0, 0.0, 0.0, 0.8 - tint as f32 * 0.02),
            offset: Point::new(drop, drop),
            blur_radius: blur,
        }),
        ..SpanStyle::default()
    })
}

// Two labels over a page whose color `shade` picks, their shadows blurred
// `blur` pixels. The first shows `text` `nudge` pixels right of its place,
// its shadow takes `tint` and `drop`, and it lies in a wide box while
// `wide` holds.
#[composable]
fn Labels(states: States) {
    let page = page_color(states.shade.get());
    FramePage(FRAME, FRAME, page, move || {
        let blur = states.blur.get();
        let label = Modifier::empty().offset(24.0 + states.nudge.get(), 20.0);
        Text(
            states.text.get(),
            if states.wide.get() {
                label.width(130.0)
            } else {
                label
            },
            shadowed(blur, states.tint.get(), states.drop.get()),
        );
        Text(
            "Key",
            Modifier::empty().offset(24.0, 80.0),
            shadowed(blur, 0, 2.0),
        );
    });
}

// The states the page draws from.
#[derive(Clone, Copy, PartialEq)]
struct States {
    shade: MutableState<i32>,
    text: MutableState<&'static str>,
    tint: MutableState<i32>,
    wide: MutableState<bool>,
    drop: MutableState<f32>,
    blur: MutableState<f32>,
    nudge: MutableState<f32>,
}

struct Page {
    shell: AppShell<WgpuRenderer>,
    states: States,
}

impl Page {
    // The page on a headless renderer, behind the GPU test lock; `None`
    // where no GPU renders.
    fn open() -> Option<(MutexGuard<'static, ()>, Self)> {
        match support::headless_renderer_parts() {
            Ok((lock, renderer)) => Some((lock, Self::new(renderer))),
            Err(err) => {
                eprintln!("skipping (headless WGPU init failed): {err}");
                None
            }
        }
    }

    fn new(renderer: WgpuRenderer) -> Self {
        let remembered: Rc<RefCell<Option<States>>> = Rc::new(RefCell::new(None));
        let remembered_by_app = Rc::clone(&remembered);
        let mut shell = AppShell::new(
            renderer,
            location_key(file!(), line!(), column!()),
            move || {
                let states = cranpose_core::remember(|| States {
                    shade: mutableStateOf(0),
                    text: mutableStateOf("Hey"),
                    tint: mutableStateOf(0),
                    wide: mutableStateOf(false),
                    drop: mutableStateOf(2.0),
                    blur: mutableStateOf(2.0),
                    nudge: mutableStateOf(0.0),
                })
                .with(|states| *states);
                *remembered_by_app.borrow_mut() = Some(states);
                Labels(states);
            },
        );
        shell.set_viewport(FRAME as f32, FRAME as f32);
        shell.set_buffer_size(FRAME, FRAME);
        shell.update();
        let states = remembered.borrow().expect("the page remembers its states");
        Self { shell, states }
    }

    fn frame(&mut self) -> (RenderStatsSnapshot, CapturedFrame) {
        support::update_and_capture(&mut self.shell, FRAME, FRAME)
    }
}

// The labels, their shadows blurred `blur` pixels, blur on the frame that
// first draws them, then the page under them changes twice. Neither later
// frame blurs or creates a texture, and the frame back on the first page
// color matches the first frame byte for byte.
fn still_labels_blur_once(blur: f32) {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    page.states.blur.set(blur);
    let (first_stats, first) = page.frame();
    assert!(
        first_stats.blur_passes >= 2,
        "the first frame blurs both label shadows: {} blur passes",
        first_stats.blur_passes
    );

    page.states.shade.set(1);
    let (changed_stats, changed) = page.frame();
    assert_ne!(
        first.pixels, changed.pixels,
        "the page under the labels changes color"
    );
    assert_eq!(
        (
            changed_stats.blur_passes,
            changed_stats.shadow_text_blur_fallbacks,
            changed_stats.offscreen_news,
        ),
        (0, 0, 0),
        "the still labels draw their kept shadows over the changed page"
    );

    page.states.shade.set(0);
    let (back_stats, back) = page.frame();
    assert_eq!(back_stats.blur_passes, 0, "the kept shadows draw again");
    support::assert_same_bytes("the kept shadows", FRAME, &first.pixels, &back.pixels);
}

#[test]
fn a_still_label_shadow_blurs_once_while_the_page_under_it_changes() {
    still_labels_blur_once(2.0);
}

// A blur of 7 pixels runs on a scratch of half the surface's size and its
// result stays there, read back bilinearly by the composite.
#[test]
fn a_still_label_shadow_blurred_at_its_scratch_size_blurs_once() {
    still_labels_blur_once(7.0);
}

// A label moved by a whole pixel draws its kept shadow at its new place.
#[test]
fn a_label_moved_by_a_whole_pixel_draws_its_kept_shadow() {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    let (_, before) = page.frame();
    page.states.nudge.set(1.0);
    let (stats, after) = page.frame();
    assert_eq!(
        stats.blur_passes, 0,
        "the moved label finds its kept shadow"
    );
    // The rows above the second label: the first label and its shadow.
    let row = FRAME as usize * 4;
    for y in 0..70 {
        let line = y * row;
        assert_eq!(
            &after.pixels[line + 4..line + row],
            &before.pixels[line..line + row - 4],
            "row {y} of the moved label is the row before it, one pixel right"
        );
    }
}

// A label whose text changes blurs its new glyphs: the kept blur of the old
// text does not stand in for them.
#[test]
fn a_label_with_new_text_blurs_its_new_shadow() {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    let (_, first) = page.frame();
    page.states.text.set("Wow");
    let (stats, changed) = page.frame();
    assert_eq!(
        stats.blur_passes, 1,
        "the changed label blurs, the still one draws its kept shadow"
    );
    assert_ne!(first.pixels, changed.pixels, "the new label draws");

    page.states.text.set("Hey");
    let (_, back) = page.frame();
    support::assert_same_bytes(
        "the label back on its text",
        FRAME,
        &first.pixels,
        &back.pixels,
    );
}

// A label whose shadow changes on every frame blurs on every frame into the
// surface it drew two frames before: the frames create no texture, so the
// renderer keeps one surface for the label and not one for each change.
#[test]
fn a_label_shadow_that_changes_every_frame_creates_no_textures() {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    page.frame();
    for tint in 1..=12 {
        page.states.tint.set(tint);
        let (stats, _) = page.frame();
        assert_eq!(
            stats.blur_passes, 1,
            "frame {tint} blurs the changed shadow"
        );
        if tint > 2 {
            assert_eq!(
                stats.offscreen_news, 0,
                "frame {tint} creates {} textures",
                stats.offscreen_news
            );
        }
    }
}

// A label's shadow surface covers the label's ink and the blur's reach: the
// label laid in a box wider than its text blurs a surface the size of the
// one it blurs in a box that fits it.
#[test]
fn a_label_shadow_surface_does_not_grow_with_its_box() {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    page.frame();
    page.states.wide.set(true);
    let (wide, _) = page.frame();
    page.states.wide.set(false);
    let (fitting, _) = page.frame();
    assert_eq!(
        (wide.blur_passes, fitting.blur_passes),
        (1, 1),
        "the label blurs its shadow in either box"
    );
    assert_eq!(
        wide.offscreen_total_bytes, fitting.offscreen_total_bytes,
        "the label's shadow surface in the wide box holds {} bytes, in the fitting box {}",
        wide.offscreen_total_bytes, fitting.offscreen_total_bytes
    );
}

// A label's shadow surface covers the shadow's glyphs and the blur's reach,
// wherever the shadow lies: a shadow dropped 8 pixels blurs a surface the
// size of one dropped 2.
#[test]
fn a_label_shadow_surface_does_not_grow_with_its_offset() {
    let Some((_lock, mut page)) = Page::open() else {
        return;
    };
    page.frame();
    page.states.drop.set(8.0);
    let (far, _) = page.frame();
    page.states.drop.set(2.0);
    let (near, _) = page.frame();
    assert_eq!(
        (far.blur_passes, near.blur_passes),
        (1, 1),
        "the label blurs its moved shadow"
    );
    assert_eq!(
        far.offscreen_total_bytes, near.offscreen_total_bytes,
        "the shadow dropped 8 pixels blurs a surface of {} bytes, dropped 2 of {}",
        far.offscreen_total_bytes, near.offscreen_total_bytes
    );
}
