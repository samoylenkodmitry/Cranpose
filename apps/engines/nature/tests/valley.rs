use nature::{RIVER_HALF_WIDTH, Stride, Valley, Walker};

#[test]
fn the_valley_has_a_river_and_plants_on_dry_ground() {
    let valley = Valley::new(7);
    let terrain = valley.terrain;
    let x = 10.0;
    let center = terrain.river_center(x);
    assert!(terrain.height(x, center) < terrain.water_level);
    assert!(terrain.height(x, center + RIVER_HALF_WIDTH * 4.0) > terrain.water_level);
    let mesh = terrain.mesh();
    assert_eq!(mesh.positions.len(), mesh.colors.len());
    assert_eq!(
        mesh.triangle_count() as u32,
        terrain.resolution * terrain.resolution * 2
    );
    println!(
        "instances {} groups {:?}",
        valley.instance_count(),
        valley
            .groups
            .iter()
            .map(|group| group.placements.len())
            .collect::<Vec<_>>()
    );
    assert!(
        valley
            .groups
            .iter()
            .all(|group| !group.placements.is_empty())
    );
}

#[test]
fn walking_follows_the_ground_and_stays_in_the_valley() {
    let terrain = Valley::new(7).terrain;
    let mut walker = Walker::start(&terrain);
    let stride = Stride {
        forward: 1.0,
        right: 0.0,
        run: true,
    };
    for _ in 0..2_000 {
        walker.step(&terrain, stride, 0.0, 1.0 / 60.0);
    }
    let [x, y, z] = walker.position;
    assert!(x <= terrain.half_size - 8.0);
    assert!((y - terrain.height(x, z).max(terrain.water_level - 0.9)).abs() < 0.5);
    assert!(walker.stamina < 1.0);
}
