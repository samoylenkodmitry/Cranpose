use super::*;

#[test]
fn memory_preferences_round_trip() {
    let store = MemoryPreferences::new();
    assert!(store.get("theme").is_none());
    store.set("theme", "dark").expect("set");
    assert_eq!(store.get("theme").as_deref(), Some("dark"));
    assert_eq!(store.keys(), vec!["theme".to_string()]);
    store.remove("theme").expect("remove");
    assert!(store.keys().is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn encoding_round_trips_separators_and_escapes() {
    let mut entries = BTreeMap::new();
    entries.insert("a=b".to_string(), "line1\nline2".to_string());
    entries.insert("percent".to_string(), "100%".to_string());
    let text = encode(&entries);
    assert!(!text.trim_end().contains('\n') || text.lines().count() == 2);
    assert_eq!(parse(&text), entries);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_corrupt_entry_falls_back_to_the_raw_text() {
    assert_eq!(unescape("50%"), "50%");
    assert_eq!(unescape("%ZZ"), "%ZZ");
}

#[test]
fn display_savers_round_trip_and_reject_junk() {
    let saver = Saver::<u32>::of_display();
    assert_eq!(saver.save(&42), "42");
    assert_eq!(saver.restore("42"), Some(42));
    assert_eq!(saver.restore("not a number"), None);
}

#[test]
fn a_custom_saver_states_its_own_stored_form() {
    let saver = Saver::new(
        |value: &Vec<u8>| {
            value
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        |stored: &str| {
            stored
                .split(',')
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().ok())
                .collect()
        },
    );
    assert_eq!(saver.save(&vec![1, 2, 3]), "1,2,3");
    assert_eq!(saver.restore("1,2,3"), Some(vec![1, 2, 3]));
    assert_eq!(saver.restore("1,x"), None);
}
