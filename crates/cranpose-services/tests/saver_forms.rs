//! `Saver::list` and `Saver::map` store a value as strings of any text and
//! read back only what they stored.

use std::collections::HashMap;

use cranpose_services::Saver;

#[derive(Clone, Debug, PartialEq)]
struct Draft {
    title: String,
    tags: Vec<String>,
}

fn list() -> Saver<Draft> {
    Saver::list(
        |draft: &Draft| {
            let mut items = vec![draft.title.clone()];
            items.extend(draft.tags.iter().cloned());
            items
        },
        |items| {
            let (title, tags) = items.split_first()?;
            Some(Draft {
                title: title.clone(),
                tags: tags.to_vec(),
            })
        },
    )
}

fn map() -> Saver<Draft> {
    Saver::map(
        |draft: &Draft| {
            vec![
                ("title".to_owned(), draft.title.clone()),
                ("tags".to_owned(), list().save(draft)),
            ]
        },
        |fields: &HashMap<String, String>| {
            let nested = list().restore(fields.get("tags")?)?;
            Some(Draft {
                title: fields.get("title")?.clone(),
                tags: nested.tags,
            })
        },
    )
}

#[test]
fn a_list_saver_keeps_items_of_any_text() {
    let draft = Draft {
        title: "12:3 a b".to_owned(),
        tags: vec![String::new(), "é:ü".to_owned(), "5:x".to_owned()],
    };
    let saver = list();
    assert_eq!(saver.restore(&saver.save(&draft)), Some(draft));
}

#[test]
fn a_map_saver_keeps_its_fields_and_nests_another_saver() {
    let draft = Draft {
        title: "notes".to_owned(),
        tags: vec!["work".to_owned(), "home".to_owned()],
    };
    let saver = map();
    assert_eq!(saver.restore(&saver.save(&draft)), Some(draft));
}

#[test]
fn a_form_the_saver_did_not_write_reads_as_nothing() {
    for stored in ["5:abc", "x:abc", "3abc", "2:ab3"] {
        assert_eq!(list().restore(stored), None, "{stored:?}");
    }
    assert_eq!(map().restore("5:title"), None, "a name without its text");
}
