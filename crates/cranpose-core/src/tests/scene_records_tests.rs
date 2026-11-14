use super::*;

#[test]
fn an_update_counted_past_the_wrap_is_after_the_one_before_it() {
    let last = SceneUpdate(u32::MAX);
    let wrapped = last.next();
    assert!(wrapped.is_after(last));
    assert!(!last.is_after(wrapped));
    assert_eq!(wrapped.since(last), 1);
    assert!(!wrapped.is_after(wrapped));
}

#[test]
fn the_last_row_a_store_holds_has_an_id_and_the_one_past_it_none() {
    let last = u32::MAX as usize - 1;
    assert_eq!(RecordId::from_index(last).map(RecordId::index), Some(last));
    assert_eq!(RecordId::from_index(last + 1), None);
}
