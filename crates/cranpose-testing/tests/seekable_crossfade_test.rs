//! A gesture moves a seekable crossfade part of the way toward another state;
//! letting go finishes or reverses the fade from where it stood, keeping the
//! content the gesture showed.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, remember};
use cranpose_testing::ComposeTestRule;
use cranpose_ui::{
    Box, BoxSpec, ContentSeek, LayoutBox, LayoutEngine, Modifier, SeekableCrossfade, Size,
};

const FRAME: u64 = 16_666_667;

/// The two states: content `A` is 10 wide and `B` 20, so a test tells them
/// apart by size.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    A,
    B,
}

struct Fade {
    rule: ComposeTestRule,
    target: MutableState<Page>,
    seek: MutableState<Option<f32>>,
    /// How many times content `B` was composed afresh.
    b_created: Rc<Cell<usize>>,
    now: u64,
}

impl Fade {
    fn new() -> Self {
        let mut rule = ComposeTestRule::new();
        let runtime = rule.runtime_handle();
        let target = MutableState::with_runtime(Page::A, runtime.clone());
        let seek = MutableState::with_runtime(None, runtime);
        let b_created = Rc::new(Cell::new(0));
        let created = Rc::clone(&b_created);
        rule.set_content(move || {
            let current = target.get();
            let toward = if current == Page::A { Page::B } else { Page::A };
            SeekableCrossfade(
                current,
                seek.get().map(|fraction| ContentSeek { toward, fraction }),
                // The default tween: 300 ms.
                Default::default(),
                {
                    let created = Rc::clone(&created);
                    move |page| {
                        if page == Page::B {
                            let created = Rc::clone(&created);
                            remember(move || created.set(created.get() + 1));
                        }
                        let width = if page == Page::A { 10.0 } else { 20.0 };
                        Box(
                            Modifier::empty().size_points(width, 10.0),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        })
        .expect("compose");
        Self {
            rule,
            target,
            seek,
            b_created,
            now: 0,
        }
    }

    fn frames(&mut self, count: u64) {
        for _ in 0..count {
            self.now += FRAME;
            self.rule.advance_frame(self.now).expect("frame");
        }
    }

    /// The alpha of the layer around the content of `page`, or `None` when
    /// that content is not composed.
    fn alpha(&mut self, page: Page) -> Option<f32> {
        let root = self.rule.root_id().expect("root");
        let layout = self
            .rule
            .applier_mut()
            .compute_layout(root, Size::new(100.0, 100.0))
            .expect("layout");
        let width = if page == Page::A { 10.0 } else { 20.0 };
        layer_alpha(layout.root(), width)
    }
}

fn layer_alpha(node: &LayoutBox, width: f32) -> Option<f32> {
    if let Some(layer) = node.node_data.modifier_slices().graphics_layer()
        && node
            .children
            .first()
            .is_some_and(|child| child.rect.width == width)
    {
        return Some(layer.alpha);
    }
    node.children
        .iter()
        .find_map(|child| layer_alpha(child, width))
}

fn close(actual: Option<f32>, expected: f32) -> bool {
    actual.is_some_and(|alpha| (alpha - expected).abs() < 0.01)
}

#[test]
fn a_seek_shows_the_state_it_moves_toward_at_its_fraction() {
    let mut fade = Fade::new();
    assert_eq!(fade.alpha(Page::B), None);
    fade.seek.set(Some(0.25));
    fade.frames(1);
    assert!(close(fade.alpha(Page::A), 0.75));
    assert!(close(fade.alpha(Page::B), 0.25));
    fade.seek.set(Some(0.6));
    fade.frames(1);
    assert!(close(fade.alpha(Page::A), 0.4));
    assert!(close(fade.alpha(Page::B), 0.6));
}

#[test]
fn letting_go_back_fades_the_seeked_state_out_from_where_it_was() {
    let mut fade = Fade::new();
    fade.seek.set(Some(0.6));
    fade.frames(1);
    fade.seek.set(None);
    fade.frames(3);
    let b = fade.alpha(Page::B).expect("still fading");
    assert!(b < 0.6 && b > 0.3, "B fades out from 0.6, read {b}");
    fade.frames(30);
    assert_eq!(fade.alpha(Page::B), None, "B leaves once faded out");
    assert!(close(fade.alpha(Page::A), 1.0));
}

#[test]
fn committing_finishes_the_fade_from_where_the_gesture_left_it() {
    let mut fade = Fade::new();
    fade.seek.set(Some(0.5));
    fade.frames(1);
    assert_eq!(fade.b_created.get(), 1);
    fade.target.set(Page::B);
    fade.seek.set(None);
    fade.frames(3);
    let b = fade.alpha(Page::B).expect("B shows");
    assert!(
        b > 0.5,
        "B goes on from 0.5 instead of starting at 0, read {b}"
    );
    let a = fade.alpha(Page::A).expect("A still fading");
    assert!(a < 0.5, "A goes on fading from 0.5, read {a}");
    fade.frames(30);
    assert_eq!(fade.alpha(Page::A), None);
    assert!(close(fade.alpha(Page::B), 1.0));
    assert_eq!(
        fade.b_created.get(),
        1,
        "B keeps the content the gesture showed"
    );
}
