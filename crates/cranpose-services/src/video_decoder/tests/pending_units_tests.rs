use super::*;

fn unit(pts_us: u64) -> PendingUnit {
    PendingUnit {
        pts_us,
        timestamp: Duration::from_micros(pts_us),
        metadata: pts_us * 10,
        submitted: Instant::now(),
    }
}

#[test]
fn a_picture_finds_its_unit_and_skips_the_units_before() {
    let mut units = PendingUnits::new();
    for pts in [100, 200, 300] {
        units.push(unit(pts));
    }
    // The unit at 100 carried parameter sets only: no picture came out of it
    assert_eq!(units.take(200).map(|unit| unit.metadata), Some(2000));
    assert_eq!(units.take(100), None);
    assert_eq!(units.take(300).map(|unit| unit.metadata), Some(3000));
}

#[test]
fn an_unknown_timestamp_takes_nothing() {
    let mut units = PendingUnits::new();
    units.push(unit(100));
    units.push(unit(300));
    assert_eq!(units.take(200), None);
    assert_eq!(units.take(100).map(|unit| unit.metadata), Some(1000));
}

#[test]
fn a_full_list_keeps_the_newest_units() {
    let mut units = PendingUnits::new();
    for pts in 1..=(CAPACITY as u64 + 10) {
        units.push(unit(pts));
    }
    assert_eq!(units.take(10), None);
    assert_eq!(units.take(11).map(|unit| unit.pts_us), Some(11));
}

#[test]
fn a_removed_unit_leaves_the_units_before_it() {
    let mut units = PendingUnits::new();
    for pts in [100, 200, 300] {
        units.push(unit(pts));
    }
    assert_eq!(units.remove(200).map(|unit| unit.metadata), Some(2000));
    assert_eq!(units.len(), 2);
    assert_eq!(units.remove(100).map(|unit| unit.metadata), Some(1000));
    assert_eq!(units.remove(200), None);
}
