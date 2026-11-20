// Which render model path holds a controller, on the lists that runtimes give.

use super::*;

fn list(paths: &[&str]) -> Vec<String> {
    paths.iter().map(ToString::to_string).collect()
}

#[test]
fn a_quest_lists_a_model_for_each_controller_and_for_its_keyboards() {
    let paths = list(&[
        "/model_fb/keyboard/local",
        "/model_fb/controller/left",
        "/model_fb/controller/right",
        "/model_fb/keyboard/remote",
    ]);
    assert_eq!(controller_path(&paths, Hand::Left), Some(1));
    assert_eq!(controller_path(&paths, Hand::Right), Some(2));
}

#[test]
fn a_runtime_that_lists_no_model_gives_no_path() {
    // Android XR and most desktop runtimes
    assert_eq!(controller_path(&[], Hand::Left), None);
    assert_eq!(controller_path(&[], Hand::Right), None);
}

#[test]
fn a_list_with_only_other_devices_gives_no_path() {
    let paths = list(&["/model_fb/keyboard/local", "/model_fb/keyboard/left"]);
    assert_eq!(controller_path(&paths, Hand::Left), None);
}

#[test]
fn a_list_with_one_side_gives_a_path_for_that_side_only() {
    let paths = list(&["/model_fb/controller/right"]);
    assert_eq!(controller_path(&paths, Hand::Left), None);
    assert_eq!(controller_path(&paths, Hand::Right), Some(0));
}

#[test]
fn the_first_path_of_a_side_wins() {
    let paths = list(&["/model_x/controller/left", "/model_fb/controller/left"]);
    assert_eq!(controller_path(&paths, Hand::Left), Some(0));
}

#[test]
fn the_side_must_follow_the_controller_segment() {
    let paths = list(&["/model_fb/left/controller"]);
    assert_eq!(controller_path(&paths, Hand::Left), None);
}
