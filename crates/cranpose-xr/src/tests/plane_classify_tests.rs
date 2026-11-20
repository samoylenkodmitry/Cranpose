use super::*;

#[test]
fn a_normal_near_the_vertical_is_a_horizontal_plane_that_faces_that_way() {
    assert_eq!(
        type_of_normal([0.0, 1.0, 0.0]),
        PlaneType::HorizontalUpwardFacing
    );
    assert_eq!(
        type_of_normal([0.0, -1.0, 0.0]),
        PlaneType::HorizontalDownwardFacing
    );
    // 20 degrees off the vertical is still horizontal, 40 degrees is not
    let tilt = |degrees: f32| [degrees.to_radians().sin(), degrees.to_radians().cos(), 0.0];
    assert_eq!(
        type_of_normal(tilt(20.0)),
        PlaneType::HorizontalUpwardFacing
    );
    assert_eq!(type_of_normal(tilt(40.0)), PlaneType::Vertical);
}

#[test]
fn a_level_normal_is_a_wall_whichever_way_it_points() {
    assert_eq!(type_of_normal([1.0, 0.0, 0.0]), PlaneType::Vertical);
    assert_eq!(type_of_normal([0.0, 0.0, -1.0]), PlaneType::Vertical);
}

#[test]
fn the_scene_labels_that_jetpack_names_map_and_every_other_label_is_unknown() {
    for (labels, expected) in [
        ("FLOOR", PlaneLabel::Floor),
        ("CEILING", PlaneLabel::Ceiling),
        ("WALL_FACE", PlaneLabel::Wall),
        ("INVISIBLE_WALL_FACE", PlaneLabel::Wall),
        ("TABLE", PlaneLabel::Table),
        ("DESK", PlaneLabel::Table),
        ("COUCH", PlaneLabel::Unknown),
        ("DOOR_FRAME", PlaneLabel::Unknown),
        ("OTHER", PlaneLabel::Unknown),
        ("", PlaneLabel::Unknown),
    ] {
        assert_eq!(label_of_meta(labels), expected, "{labels:?}");
    }
}

#[test]
fn a_list_of_labels_gives_the_first_one_that_jetpack_names() {
    assert_eq!(label_of_meta("COUCH,TABLE"), PlaneLabel::Table);
    assert_eq!(label_of_meta("OTHER, WALL_FACE"), PlaneLabel::Wall);
}
