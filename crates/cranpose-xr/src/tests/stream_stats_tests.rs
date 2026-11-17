use super::*;

#[test]
fn a_clone_of_the_handle_reads_the_counts_that_the_scene_published() {
    let stats = StreamStats::new();
    let reader = stats.clone();
    assert_eq!(reader.counts(), StreamCounts::default());
    let counts = StreamCounts {
        shown: 30,
        skipped: 2,
        repeated: 90,
    };
    stats.publish(counts);
    assert_eq!(reader.counts(), counts);
    assert_eq!(stats, reader);
    assert_ne!(stats, StreamStats::new());
}
