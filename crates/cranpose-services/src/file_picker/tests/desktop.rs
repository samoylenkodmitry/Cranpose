use super::save_filter;

#[test]
fn a_save_dialog_knows_the_type_of_its_default_name() {
    assert_eq!(
        save_filter("Receipt.txt"),
        Some(("TXT".to_string(), "txt".to_string()))
    );
    assert_eq!(
        save_filter("items 2026.07.csv"),
        Some(("CSV".to_string(), "csv".to_string()))
    );
    assert_eq!(save_filter("Receipt"), None);
    assert_eq!(save_filter("Receipt."), None);
}
