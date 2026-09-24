use std::cell::RefCell;

use super::*;

fn text_with_links() -> AnnotatedString {
    AnnotatedString::builder()
        .append("Read the ")
        .with_link(
            LinkAnnotation::Url("https://example.test/docs".into()),
            |builder| builder.append("docs"),
        )
        .append(" or ")
        .with_link(
            LinkAnnotation::Clickable {
                tag: "help".into(),
                handler: Rc::new(|| {}),
            },
            |builder| builder.append("ask"),
        )
        .to_annotated_string()
}

#[test]
fn a_reader_opens_each_link_from_the_actions_menu() {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let open_url: Rc<dyn Fn(&str)> = {
        let opened = Rc::clone(&opened);
        Rc::new(move |url: &str| opened.borrow_mut().push(url.to_owned()))
    };
    let mut config = cranpose_foundation::SemanticsConfiguration::default();

    link_actions(Rc::new(text_with_links()), open_url)(&mut config);
    let labels: Vec<_> = config
        .custom_actions
        .iter()
        .map(|action| action.label.as_str())
        .collect();
    config.custom_actions[0].invoke();

    assert_eq!(labels, ["Open docs", "Open ask"]);
    assert_eq!(
        *opened.borrow(),
        vec!["https://example.test/docs".to_owned()]
    );
}

#[test]
fn a_link_with_no_shown_text_is_named_by_its_target() {
    let text = AnnotatedString::builder()
        .with_link(
            LinkAnnotation::Url("https://example.test/".into()),
            |builder| builder,
        )
        .to_annotated_string();
    let mut config = cranpose_foundation::SemanticsConfiguration::default();

    link_actions(Rc::new(text), Rc::new(|_: &str| {}))(&mut config);

    assert_eq!(config.custom_actions[0].label, "Open https://example.test/");
}
