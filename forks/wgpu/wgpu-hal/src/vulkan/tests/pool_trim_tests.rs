use super::PoolTrim;

/// Records `passes` render passes and resets, returning whether the reset
/// gave the pool's memory back.
fn frame(pool: &mut PoolTrim, passes: u32) -> bool {
    for _ in 0..passes {
        pool.begin_render_pass();
    }
    pool.reset()
}

#[test]
fn a_steady_workload_keeps_its_memory() {
    for passes in [3, 40] {
        let mut pool = PoolTrim::default();
        assert!((0..200).all(|_| !frame(&mut pool, passes)), "{passes} passes a frame");
    }
}

#[test]
fn a_burst_gives_its_memory_back_once_light_frames_follow() {
    let mut pool = PoolTrim::default();
    assert!((0..30).all(|_| !frame(&mut pool, 40)));
    assert!((0..3).all(|_| !frame(&mut pool, 5)));
    assert!(frame(&mut pool, 5), "the fourth light frame releases");
    assert!((0..200).all(|_| !frame(&mut pool, 5)), "the memory goes back once");
}

#[test]
fn heavy_frames_between_light_ones_keep_the_memory() {
    let mut pool = PoolTrim::default();
    for _ in 0..100 {
        assert!(!frame(&mut pool, 40));
        assert!((0..3).all(|_| !frame(&mut pool, 5)));
    }
}

#[test]
fn a_small_pool_keeps_its_memory() {
    let mut pool = PoolTrim::default();
    assert!(!frame(&mut pool, 7));
    assert!((0..200).all(|_| !frame(&mut pool, 1)));
}
