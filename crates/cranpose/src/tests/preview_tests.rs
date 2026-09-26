use super::*;

#[crate::preview(name = "Compact", group = "Tests", width = 320, height = 240)]
#[crate::composable]
fn Fixture() {}

#[crate::preview(name = "Wide", width = 800, height = 600, dark = true)]
#[crate::composable]
fn WideFixture() {}

#[test]
fn registered_preview_metadata_comes_from_the_compiler() {
    let value = find("Fixture").expect("fixture registered");
    assert_eq!(value.name, "Compact");
    assert_eq!((value.width, value.height), (320, 240));
    assert!(value.file.ends_with("preview_tests.rs"));
    assert!(value.line > 0);
    assert!(!value.dark);
    assert!(find("Wide").expect("variant name works").dark);
    assert!(registered().iter().any(|preview| preview.id == value.id));
    assert_eq!(find(value.id).expect("exact lookup").id, value.id);
}

#[test]
fn ambiguous_and_missing_names_are_reported() {
    let a = find("Fixture").expect("registered");
    let b = Box::leak(Box::new(Preview {
        id: "other",
        name: a.name,
        group: "",
        function: "Other",
        file: "",
        line: 1,
        width: 1,
        height: 1,
        dark: false,
        render: || {},
    }));
    assert!(matches!(
        select(&[a, b], a.name),
        Err(PreviewError::Ambiguous(_))
    ));
    assert!(matches!(
        find("NoSuchPreview"),
        Err(PreviewError::Missing(_))
    ));
}
