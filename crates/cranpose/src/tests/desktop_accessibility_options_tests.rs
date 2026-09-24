use super::*;

#[test]
fn a_settings_tool_answer_reads_as_a_switch() {
    assert!(flag_is_on("1"));
    assert!(flag_is_on("true"));
    assert!(flag_is_on(" 'on' "));
    assert!(!flag_is_on("0"));
    assert!(!flag_is_on("false"));
    assert!(!flag_is_on(""));
}

#[test]
fn a_settings_tool_answer_reads_as_a_number() {
    assert_eq!(number_in("1.25"), Some(1.25));
    assert_eq!(number_in("uint32 2"), Some(2.0));
    assert_eq!(number_in("'Adwaita'"), None);
    assert_eq!(number_in("0"), None);
}

#[test]
fn a_registry_line_reads_as_a_number() {
    let text = "\r\nHKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics\r\n    MinAnimate    REG_SZ    0\r\n";
    assert_eq!(registry_value(text), Some(0.0));
    let flags = "HKEY_CURRENT_USER\\Control Panel\\Accessibility\\HighContrast\n    Flags    REG_DWORD    0x7f\n";
    assert_eq!(registry_value(flags), Some(127.0));
    assert_eq!(
        registry_value("ERROR: The system was unable to find the specified registry key or value."),
        None
    );
}

#[test]
fn the_environment_wins_over_the_system() {
    let options = AccessibilityOptions::default();
    let mut set = options;
    for (name, field) in SWITCH_VARS {
        let _ = name;
        *field(&mut set) = true;
    }
    assert!(set.reduce_motion && set.reduce_transparency && set.increase_contrast && set.bold_text);
}
