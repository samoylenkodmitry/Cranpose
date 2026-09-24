use super::*;

fn ticks(values: &[u64]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn accumulate_frame_attributes_ticks_by_label() {
    let mut totals = vec![LabelTotal::default(); 2];
    let mapped = ticks(&[1_000, 1_100, 1_100, 1_150, 1_150, 1_160]);
    accumulate_frame(&mut totals, &[(0, 0), (1, 2), (0, 4)], &mapped, 2.0);
    assert_eq!(totals[0].nanoseconds, 220);
    assert_eq!(totals[0].passes, 2);
    assert_eq!(totals[1].nanoseconds, 100);
    assert_eq!(totals[1].passes, 1);
}

#[test]
fn accumulate_frame_skips_backwards_and_out_of_range_pairs() {
    let mut totals = vec![LabelTotal::default(); 1];
    let mapped = ticks(&[500, 400]);
    accumulate_frame(&mut totals, &[(0, 0)], &mapped, 1.0);
    assert_eq!(totals[0].passes, 0, "an end before its begin is skipped");

    accumulate_frame(&mut totals, &[(0, 6)], &mapped, 1.0);
    assert_eq!(totals[0].passes, 0, "indices past the mapping are skipped");

    let mapped = ticks(&[100, 250]);
    accumulate_frame(&mut totals, &[(9, 0)], &mapped, 1.0);
    assert_eq!(totals[0].passes, 0, "an unknown label id is skipped");
}
