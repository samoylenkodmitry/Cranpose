// `LinkedText` through the headless robot: a click on a link that the text
// wrapped onto a later line opens that link, and a click on a link runs the
// handler of the text shown, also when an equal text came before it.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, LinkedText, Modifier, TextStyle,
    text::{AnnotatedString, LinkAnnotation, get_offset_for_position, offset_for_position_wrapped},
};

const WORDS: usize = 24;

// Words, each a link to its own number.
fn words() -> AnnotatedString {
    let mut builder = AnnotatedString::builder();
    for word in 0..WORDS {
        builder = builder
            .push_link(LinkAnnotation::Url(format!("https://example.test/{word}")))
            .append(&format!("w{word:02}"))
            .pop()
            .append(" ");
    }
    builder.to_annotated_string()
}

// The link of the word at byte `offset`.
fn link_at(text: &AnnotatedString, offset: usize) -> Option<String> {
    text.get_link_annotations(offset, offset + 1)
        .into_iter()
        .find(|link| link.range.contains(&offset))
        .and_then(|link| match &link.item {
            LinkAnnotation::Url(url) => Some(url.clone()),
            LinkAnnotation::Clickable { .. } => None,
        })
}

#[test]
fn a_click_on_a_link_on_a_wrapped_line_opens_that_link() {
    let text = words();
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = Rc::clone(&opened);
    let shown = text.clone();
    let mut robot = create_headless_robot_test(400, 400, move || {
        let sink = Rc::clone(&sink);
        let shown = shown.clone();
        Box(
            Modifier::empty().width(120.0),
            BoxSpec::default(),
            move || {
                let sink = Rc::clone(&sink);
                LinkedText(
                    shown.clone(),
                    Modifier::empty(),
                    TextStyle::default(),
                    move |url| sink.borrow_mut().push(url.to_owned()),
                );
            },
        );
    });
    let bounds = robot
        .find_by_text(text.text())
        .bounds()
        .expect("the paragraph shows");
    // The start of the last line.
    let (x, y) = (2.0, bounds.height - 3.0);
    let style = TextStyle::default();
    let (wrapped, unwrapped) = robot.shell_mut().app_context().enter(|| {
        (
            offset_for_position_wrapped(text.text(), &style, Some(bounds.width), x, y),
            get_offset_for_position(&text, &style, x, y),
        )
    });
    let expected = link_at(&text, wrapped).expect("a word starts the last line");
    assert_ne!(
        link_at(&text, unwrapped).as_ref(),
        Some(&expected),
        "the text must wrap for this test to tell the layouts apart"
    );

    robot.click_at(bounds.x + x, bounds.y + y);
    robot.wait_for_idle();

    assert_eq!(*opened.borrow(), [expected]);
}

// A text whose one link, tagged alike in every generation, notes
// `generation` when clicked.
fn tagged(clicks: &Rc<RefCell<Vec<u32>>>, generation: u32) -> AnnotatedString {
    let clicks = Rc::clone(clicks);
    AnnotatedString::builder()
        .push_link(LinkAnnotation::Clickable {
            tag: "same-tag".into(),
            handler: Rc::new(move || clicks.borrow_mut().push(generation)),
        })
        .append("payload")
        .pop()
        .to_annotated_string()
}

fn click_payload(robot: &mut RobotTestRule<TestRenderer>) {
    let bounds = robot
        .find_by_text("payload")
        .bounds()
        .expect("the link shows");
    robot.click_at(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    robot.wait_for_idle();
}

#[test]
fn a_click_on_a_link_runs_the_handler_of_the_text_shown() {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let generation_slot = Rc::new(RefCell::new(None::<MutableState<u32>>));
    let app_clicks = Rc::clone(&clicks);
    let app_slot = Rc::clone(&generation_slot);
    let mut robot = create_headless_robot_test(400, 400, move || {
        let generation = rememberMutableStateOf(|| 0u32);
        *app_slot.borrow_mut() = Some(generation);
        LinkedText(
            tagged(&app_clicks, generation.get()),
            Modifier::empty(),
            TextStyle::default(),
            |_url| {},
        );
    });
    let generation = generation_slot.borrow().expect("the app composed");

    click_payload(&mut robot);
    generation.set(1);
    robot.wait_for_idle();
    click_payload(&mut robot);

    assert_eq!(*clicks.borrow(), [0, 1]);
}
