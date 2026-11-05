use super::*;

const RADIUS: f32 = 40.0;

// The first cell on a ring around the origin whose east side is `wanted`.
fn cell_with_east(wanted: impl Fn(Side) -> bool) -> Cell {
    (0..64)
        .flat_map(|x| (0..64).map(move |z| Cell { x, z }))
        .find(|cell| wanted(cell.side(Toward::East)))
        .expect("the maze holds every kind of side near the origin")
}

// The eye `x` and `z` from the corner of `cell`.
fn eye_at(cell: Cell, x: f32, z: f32) -> Spot {
    Spot {
        cell,
        at: Vec3::new(x, -165.0, z),
    }
}

#[test]
fn a_wall_stops_the_eye_its_radius_before_the_wall() {
    let cell = cell_with_east(|side| side == Side::Wall);
    let from = eye_at(cell, CELL * 0.5, CELL * 0.5);
    let to = walk(from, Vec3::new(CELL, 0.0, 0.0), RADIUS);
    let gap = CELL - WALL_HALF - to.at.x;
    assert!(
        (gap - RADIUS).abs() < 0.5,
        "the eye stops {gap} short of the wall's face"
    );
    assert_eq!(to.cell, cell);
}

#[test]
fn the_eye_walks_through_a_doorway() {
    let cell = cell_with_east(|side| matches!(side, Side::Door { .. }));
    let Side::Door { middle } = cell.side(Toward::East) else {
        unreachable!("the cell was picked for its doorway");
    };
    let from = eye_at(cell, CELL * 0.5, middle);
    let to = walk(from, Vec3::new(CELL, 0.0, 0.0), RADIUS);
    assert_eq!(
        to.cell,
        cell.next(Toward::East),
        "the eye walked from {from:?} to {to:?}"
    );
}

#[test]
fn a_long_walk_never_passes_through_a_wall() {
    let mut eye = eye_at(spawn(), CELL * 0.5, CELL * 0.5);
    let mut seed = 0x1234_5678_u32;
    for _ in 0..20_000 {
        seed = mix32(seed.wrapping_add(0x9e37_79b9));
        let angle = (seed >> 8) as f32 / 16_777_216.0 * std::f32::consts::TAU;
        let length = (seed & 0xff) as f32 / 255.0 * 3.0 * RADIUS;
        let step = Vec3::new(angle.cos() * length, 0.0, angle.sin() * length);
        let from = eye.cell;
        eye = walk(eye, step, RADIUS);
        let to = eye.cell;
        let (dx, dz) = (to.x - from.x, to.z - from.z);
        let x_way = if dx > 0 { Toward::East } else { Toward::West };
        let z_way = if dz > 0 { Toward::North } else { Toward::South };
        let through = |cell: Cell, way: Toward| cell.side(way).passes(Sight::Walk);
        let passed = match (dx.abs(), dz.abs()) {
            (0, 0) => true,
            (1, 0) => through(from, x_way),
            (0, 1) => through(from, z_way),
            (1, 1) => {
                (through(from, x_way) && through(from.next(x_way), z_way))
                    || (through(from, z_way) && through(from.next(z_way), x_way))
            }
            _ => false,
        };
        assert!(
            passed,
            "the eye passed from {from:?} to {to:?} through a wall"
        );
        let pushed = push_out(eye, RADIUS);
        assert!(
            pushed.cell == eye.cell
                && (pushed.at.x - eye.at.x).abs() + (pushed.at.z - eye.at.z).abs() < 0.01,
            "the eye at {eye:?} overlaps a wall or a pillar"
        );
    }
}

#[test]
fn the_eye_starts_lit_and_free_to_walk_far() {
    let cell = spawn();
    assert_eq!(cell.lamp(), Lamp::Lit);
    assert!(cell.reaches(SPAWN_REACH));
}
