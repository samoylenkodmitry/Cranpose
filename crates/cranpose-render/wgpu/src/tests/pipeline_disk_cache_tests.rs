use super::PersistWatch;

#[test]
fn a_burst_of_pipelines_persists_once_after_it_goes_quiet() {
    let mut watch = PersistWatch::default();
    assert!(!watch.observe(0));
    assert!(!watch.observe(3), "still growing");
    assert!(!watch.observe(5), "still growing");
    assert!(watch.observe(5), "quiet for a tick with new pipelines");
    assert!(!watch.observe(5), "written already");
}

#[test]
fn a_pipeline_reached_late_in_a_session_persists_too() {
    let mut watch = PersistWatch::default();
    watch.observe(19);
    assert!(watch.observe(19));
    for _ in 0..20 {
        assert!(!watch.observe(19));
    }
    assert!(!watch.observe(20));
    assert!(watch.observe(20));
}
