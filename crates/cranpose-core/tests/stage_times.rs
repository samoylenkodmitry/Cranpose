use cranpose_core::StageTimes;

#[test]
fn stage_times_are_kept_only_while_telemetry_is_on() {
    let mut off = StageTimes::<3>::start(false);
    off.reached();
    off.reached();
    assert!(off.times().is_none());

    let mut on = StageTimes::<3>::start(true);
    on.reached();
    on.reached();
    // A pass that reaches more stages than it declared keeps the last time.
    on.reached();
    let [start, middle, end] = on.times().expect("telemetry on keeps the times");
    assert!(start <= middle && middle <= end);
}
