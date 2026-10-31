use bytemuck::Zeroable;

use super::*;

#[test]
fn uniform_chunks_stop_at_the_windows_webgl_placement_budget() {
    let mut staging = ArenaStaging::default();
    let uniform = RunBufferMode {
        storage: false,
        trig_fill: false,
    };
    let storage = RunBufferMode {
        storage: true,
        trig_fill: true,
    };
    for _ in 0..PLACEMENT_CHUNK {
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
    let tables = ArenaTables::new(
        RunBufferMode {
            storage: false,
            trig_fill: false,
        },
        UploadMode::Copied,
        256,
    );
    assert_eq!(
        tables.bindings[PLACEMENT_BUFFER],
        (PLACEMENT_CHUNK * size_of::<PlacementData>()) as u64,
    );
}
