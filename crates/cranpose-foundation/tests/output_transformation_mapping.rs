//! How an output transformation's shown text lines up with the text, at the
//! edges where a caret or a tap could land on the wrong side.

use cranpose_foundation::text::OutputTransformation;

/// A space after every third character, as a grouped number shows.
fn grouped() -> OutputTransformation {
    OutputTransformation::new(|text, changes| {
        for at in (3..text.len()).step_by(3) {
            changes.insert(at, " ");
        }
    })
}

#[test]
fn inserted_text_shows_between_the_characters_around_it() {
    let shown = grouped().apply("1234567");
    assert_eq!(shown.text, "123 456 7");
    let map = &shown.mapping;
    assert_eq!(
        map.to_shown(3),
        3,
        "the caret after 3 stands before the space"
    );
    assert_eq!(map.to_shown(4), 5, "and after 4 past it");
    assert_eq!(map.to_shown(7), 9, "the end maps to the end");
    assert_eq!(
        map.to_source(4),
        3,
        "a tap after the space stands for the caret after 3"
    );
    assert_eq!(map.to_source(5), 4);
    assert_eq!(map.to_source(9), 7);
}

#[test]
fn a_masked_character_maps_as_a_whole() {
    let shown = OutputTransformation::mask('\u{2022}').apply("a\u{1F1F7}\u{1F1F8}b");
    assert_eq!(
        shown.text, "\u{2022}\u{2022}\u{2022}",
        "a flag is one character"
    );
    let map = &shown.mapping;
    assert_eq!(map.to_shown(1), 3);
    assert_eq!(map.to_shown(9), 6, "after the flag");
    assert_eq!(map.to_shown(10), 9);
    assert_eq!(
        map.to_source(4),
        1,
        "inside the second dot, nearer its start"
    );
    assert_eq!(map.to_source(5), 9, "nearer its end");
}

#[test]
fn an_empty_text_and_an_unchanged_one_map_offset_for_offset() {
    let empty = grouped().apply("");
    assert_eq!(empty.text, "");
    assert_eq!(empty.mapping.to_shown(0), 0);
    assert_eq!(empty.mapping.to_source(0), 0);

    let short = grouped().apply("12");
    assert_eq!(short.text, "12");
    assert_eq!(short.mapping.to_shown(1), 1);
    assert_eq!(short.mapping.to_source(2), 2);
}

#[test]
fn a_change_that_overlaps_an_earlier_one_is_dropped() {
    let overlapping = OutputTransformation::new(|_, changes| {
        changes.replace(1..3, "X");
        changes.replace(2..4, "Y");
    });
    let shown = overlapping.apply("abcde");
    assert_eq!(shown.text, "aXde");
    assert_eq!(shown.mapping.to_shown(3), 2);
    assert_eq!(shown.mapping.to_source(2), 3);
}
