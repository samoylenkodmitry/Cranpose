use super::*;

#[test]
fn version_comparison_is_numeric_per_component() {
    assert!(
        is_newer_version("v0.1.10", "v0.1.9"),
        "10 must sort after 9 numerically, not before it as strings would"
    );
    assert!(
        is_newer_version("0.1.10", "0.1.9"),
        "the comparison works the same without a leading v"
    );
    assert!(!is_newer_version("v0.1.9", "v0.1.10"));
    assert!(
        !is_newer_version("v1.2.3", "v1.2.3"),
        "identical versions are not newer than themselves"
    );
    assert!(is_newer_version("v1.3.0", "v1.2.9"));
    assert!(
        !is_newer_version("v1.2", "v1.2.0"),
        "a missing trailing component reads as zero, not as older"
    );
}
