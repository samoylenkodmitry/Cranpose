use crate::reference;

#[test]
fn reference_configuration_rejects_invalid_values_and_cannot_change_after_initialization() {
    for content in [
        r#"{"titles":["A","B","C","D"],"icons":[0,1,2,4]}"#,
        r#"{"titles":["A","B","C","D"],"icons":[0,1,2,3],"palette":[]}"#,
        r#"{"titles":["A","B","C","D"],"icons":[0,1,2,3],"accent":[0,2,0]}"#,
        r#"{"titles":["A","B","C","D"],"icons":[0,1,2,3],"palette":[[-1,0,0]]}"#,
        "invalid json",
    ] {
        assert!(reference::configure_reference_content(content).is_err());
    }
    let content =
        r#"{"titles":["Inbox","WWW","II","Settings"],"icons":[1,3,2,0],"accent":[0.2,0.65,0.1]}"#;
    assert!(reference::configure_reference_content(content).is_ok());
    assert!(reference::configure_reference_content(content).is_err());
}
