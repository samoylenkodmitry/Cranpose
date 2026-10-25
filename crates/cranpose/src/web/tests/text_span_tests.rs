use super::{ChangedSpan, changed_span};

fn span(start: usize, removed_end: usize, inserted_end: usize) -> Option<ChangedSpan> {
    Some(ChangedSpan {
        start,
        removed_end,
        inserted_end,
    })
}

#[test]
fn the_same_text_changed_nothing() {
    assert_eq!(changed_span("", ""), None);
    assert_eq!(changed_span("日本", "日本"), None);
}

#[test]
fn a_letter_typed_into_a_run_of_the_same_letter_is_one_insertion() {
    assert_eq!(changed_span("aa", "aaa"), span(2, 2, 3));
    assert_eq!(changed_span("aaa", "aa"), span(2, 3, 2));
}

#[test]
fn a_change_between_two_letters_that_share_a_first_byte_starts_at_the_letter() {
    // 'é' is C3 A9 and 'è' is C3 A8: a byte comparison would start inside them.
    assert_eq!(changed_span("café", "cafè"), span(3, 5, 5));
}

#[test]
fn an_emoji_typed_before_the_same_emoji_keeps_whole_characters() {
    assert_eq!(changed_span("A🌍Z", "A🌍🌍Z"), span(5, 5, 9));
}

#[test]
fn a_composition_replaced_in_the_middle_is_replaced_once() {
    assert_eq!(changed_span("AにZ", "A日本Z"), span(1, 4, 7));
}

#[test]
fn clearing_or_filling_a_field_replaces_all_of_it() {
    assert_eq!(changed_span("abc", ""), span(0, 3, 0));
    assert_eq!(changed_span("", "abc"), span(0, 0, 3));
}
