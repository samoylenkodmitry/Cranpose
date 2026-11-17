use super::*;

fn with_space(test: impl FnOnce(Space)) {
    cranpose_testing::run_test_composition(|_| test(Space::new()));
}

#[test]
fn panels_take_parts_of_the_atlas_one_under_the_other() {
    with_space(|space| {
        let (first, second) = (space.new_panel_id(), space.new_panel_id());
        assert_eq!(
            space.place(first, [800.0, 500.0], Pose::IDENTITY),
            [0.0, 0.0, 800.0, 500.0]
        );
        assert_eq!(
            space.place(second, [300.0, 200.0], Pose::IDENTITY),
            [0.0, 508.0, 300.0, 200.0]
        );
        assert_eq!(space.atlas_size(), [800.0, 708.0]);
    });
}

#[test]
fn a_panel_keeps_its_part_while_its_size_stays() {
    with_space(|space| {
        let id = space.new_panel_id();
        let part = space.place(id, [400.0, 300.0], Pose::IDENTITY);
        let moved = Pose::at([0.1, 0.0, -1.0]);
        assert_eq!(space.place(id, [400.0, 300.0], moved), part);
        assert_eq!(space.panels()[0].pose, moved);
        assert_eq!(
            space.place(id, [400.0, 320.0], moved),
            [0.0, 308.0, 400.0, 320.0]
        );
        space.remove(id);
        assert!(space.panels().is_empty());
        let again = space.new_panel_id();
        assert_eq!(
            space.place(again, [10.0, 10.0], Pose::IDENTITY),
            [0.0, 0.0, 10.0, 10.0]
        );
    });
}
