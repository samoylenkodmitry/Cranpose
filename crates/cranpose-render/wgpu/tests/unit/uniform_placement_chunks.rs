use super::*;

#[test]
fn uniform_chunks_stop_at_the_windows_webgl_placement_budget() {
    let mut staging = ArenaStaging::default();
    let uniform = RunBufferMode { storage: false };
    let storage = RunBufferMode { storage: true };
    for _ in 0..4 {
        assert!(staging.fits(uniform, 1, 0, 0));
        staging.placements.push(PlacementData::zeroed());
    }
    assert!(!staging.fits(uniform, 1, 0, 0));
    assert!(staging.fits(storage, 1, 0, 0));
    staging.clear();
    assert!(staging.fits(uniform, 1, 0, 0));
}

#[test]
fn uniform_binding_covers_the_entire_placement_chunk() {
    let tables = ArenaTables::new(RunBufferMode { storage: false }, 256);
    assert_eq!(
        tables.bindings[PLACEMENT_BUFFER],
        (4 * std::mem::size_of::<PlacementData>()) as u64,
    );
}
