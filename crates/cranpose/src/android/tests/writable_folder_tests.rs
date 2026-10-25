use super::*;

#[test]
fn listing_rows_carry_size_and_modified_time() {
    let entry = parse_entry("sync.json\t128\t1700000000000").expect("a row parses");
    assert_eq!(entry.name, "sync.json");
    assert_eq!(entry.len, 128);
    assert_eq!(entry.modified_millis, Some(1_700_000_000_000));
}

#[test]
fn a_provider_without_metadata_still_lists_the_name() {
    let entry = parse_entry("plain.bin\t\t").expect("a row parses");
    assert_eq!(entry.len, 0);
    assert_eq!(entry.modified_millis, None);
    assert!(parse_entry("").is_none());
}
