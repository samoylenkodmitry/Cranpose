//! `AnimatedContent` frame by frame: the content that comes and the content
//! that goes slide by their own widths, nothing recomposes while they move,
//! the content that went leaves the composition, a change back brings it
//! back from where it was, and the container is as large as the largest
//! content or follows a `SizeTransform`.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, remember};
use cranpose_testing::ComposeTestRule;
use cranpose_ui::{
    AnimatedContent, Box, BoxSpec, ContentTransform, LayoutBox, LayoutEngine, Modifier, Size,
    SizeTransform, slide_in_horizontally, slide_out_horizontally,
};

const FRAME: u64 = 16_666_667;

/// Content `A` is 10 by 10 and `B` 30 by 20, so a test tells them apart by
/// width.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    A,
    B,
}

impl Page {
    fn size(self) -> Size {
        match self {
            Page::A => Size::new(10.0, 10.0),
            Page::B => Size::new(30.0, 20.0),
        }
    }
}

struct Screen {
    rule: ComposeTestRule,
    target: MutableState<Page>,
    /// Times the content closure ran, and times `A` was created afresh.
    runs: Rc<Cell<usize>>,
    a_created: Rc<Cell<usize>>,
    now: u64,
}

/// B comes in from the right and A leaves to the left, by their widths.
fn slide(sized: bool) -> ContentTransform {
    let transform = slide_in_horizontally(1.0).together_with(slide_out_horizontally(-1.0));
    if sized {
        transform.using(SizeTransform::default())
    } else {
        transform
    }
}

impl Screen {
    fn new(sized: bool) -> Self {
        let mut rule = ComposeTestRule::new();
        let target = MutableState::with_runtime(Page::A, rule.runtime_handle());
        let runs = Rc::new(Cell::new(0));
        let a_created = Rc::new(Cell::new(0));
        let (ran, created) = (Rc::clone(&runs), Rc::clone(&a_created));
        rule.set_content(move || {
            let ran = Rc::clone(&ran);
            let created = Rc::clone(&created);
            AnimatedContent(
                target.get(),
                move |_, _| slide(sized),
                move |page: Page| {
                    ran.set(ran.get() + 1);
                    if page == Page::A {
                        let created = Rc::clone(&created);
                        remember(move || created.set(created.get() + 1));
                    }
                    Box(
                        Modifier::empty().size(page.size()),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        })
        .expect("compose");
        Self {
            rule,
            target,
            runs,
            a_created,
            now: 0,
        }
    }

    /// Runs frames with a layout after each, as an app does.
    fn frames(&mut self, count: u64) {
        for _ in 0..count {
            self.now += FRAME;
            self.rule.advance_frame(self.now).expect("frame");
            self.layout();
        }
    }

    fn layout(&mut self) -> LayoutBox {
        let root = self.rule.root_id().expect("root");
        self.rule
            .applier_mut()
            .compute_layout(root, Size::new(200.0, 200.0))
            .expect("layout")
            .root()
            .clone()
    }

    /// The horizontal translation of the layer around `page`, or `None`
    /// when that content is not composed.
    fn shift(&mut self, page: Page) -> Option<f32> {
        layer_shift(&self.layout(), page.size().width)
    }

    fn container(&mut self) -> Size {
        let root = self.layout();
        Size::new(root.rect.width, root.rect.height)
    }
}

fn layer_shift(node: &LayoutBox, width: f32) -> Option<f32> {
    if let Some(layer) = node.node_data.modifier_slices().graphics_layer()
        && node
            .children
            .first()
            .is_some_and(|child| child.rect.width == width)
    {
        return Some(layer.translation_x);
    }
    node.children
        .iter()
        .find_map(|child| layer_shift(child, width))
}

#[test]
fn both_contents_slide_by_their_widths_and_nothing_recomposes_on_the_way() {
    let mut screen = Screen::new(false);
    assert_eq!(screen.shift(Page::A), Some(0.0), "the first content rests");
    screen.target.set(Page::B);
    screen.frames(1);
    let runs = screen.runs.get();
    screen.frames(6);
    let b = screen.shift(Page::B).expect("B comes in");
    let a = screen.shift(Page::A).expect("A goes out");
    assert!(
        b > 0.0 && b < 30.0,
        "B is part of its width to the right: {b}"
    );
    assert!(
        a < 0.0 && a > -10.0,
        "A is part of its width to the left: {a}"
    );
    assert_eq!(screen.runs.get(), runs, "the frames moved layers only");
    screen.frames(30);
    assert_eq!(screen.shift(Page::A), None, "A left after its exit");
    assert_eq!(screen.shift(Page::B), Some(0.0));
}

#[test]
fn a_change_back_brings_the_leaving_content_back_from_where_it_was() {
    let mut screen = Screen::new(false);
    screen.target.set(Page::B);
    screen.frames(6);
    let leaving = screen.shift(Page::A).expect("A goes out");
    screen.target.set(Page::A);
    screen.frames(2);
    let back = screen.shift(Page::A).expect("A comes back");
    // The spring keeps A's momentum for a moment; A stays on the side it
    // left by and does not jump to the side B came from.
    assert!(
        back < 0.0,
        "A turns back the way it went: {back} from {leaving}"
    );
    screen.frames(30);
    assert_eq!(screen.shift(Page::A), Some(0.0));
    assert_eq!(screen.shift(Page::B), None);
    assert_eq!(screen.a_created.get(), 1, "A kept its composition");
}

#[test]
fn the_container_holds_the_largest_content_during_a_change() {
    let mut screen = Screen::new(false);
    assert_eq!(screen.container(), Page::A.size());
    screen.target.set(Page::B);
    screen.frames(2);
    assert_eq!(screen.container(), Page::B.size());
    screen.target.set(Page::A);
    screen.frames(2);
    assert_eq!(screen.container(), Page::B.size(), "B is still leaving");
    screen.frames(30);
    assert_eq!(screen.container(), Page::A.size());
}

#[test]
fn a_size_transform_animates_the_container_to_the_new_content() {
    let mut screen = Screen::new(true);
    assert_eq!(screen.container(), Page::A.size());
    screen.target.set(Page::B);
    screen.frames(6);
    let size = screen.container();
    assert!(
        size.width > 10.0 && size.width < 30.0 && size.height > 10.0 && size.height < 20.0,
        "the size is on its way: {size:?}"
    );
    screen.frames(40);
    assert_eq!(screen.container(), Page::B.size());
}
